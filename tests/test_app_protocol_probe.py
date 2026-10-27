"""Pure negative controls for C4 acceptance; no native process, model, or GUI."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import stat
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location(
    "app_protocol_probe", Path(__file__).parent / "acceptance/app_protocol_probe.py")
probe = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(probe)


class AcceptanceAssertions(unittest.TestCase):
    def test_conflicting_text_and_structured_receipts_fail(self):
        value = {"version": 1, "receipt": {"outcome": {"dispatch": "unknown"}}}
        result = {"structuredContent": value, "content": [{"type": "text", "text": json.dumps(value)}]}
        self.assertEqual(probe.Wire.payload({"result": result}), value)
        result["content"][0]["text"] = json.dumps({"version": 1, "receipt": {"outcome": {"dispatch": "dispatched"}}})
        with self.assertRaises(AssertionError):
            probe.Wire.payload({"result": result})
        del result["structuredContent"]
        with self.assertRaises(AssertionError):
            probe.Wire.payload({"result": result})

    def test_unknown_dispatch_cannot_pass_success_or_rejection(self):
        for dispatch in ("unknown", "running", "failed", None):
            reply = {"receipt": {"outcome": {"dispatch": dispatch, "evidence": {"kind": "unverified"}}}}
            with self.subTest(dispatch=dispatch):
                with self.assertRaises(AssertionError):
                    probe.Wire.dispatched(reply)
                with self.assertRaises(AssertionError):
                    probe.Wire.rejected(reply)
        for status in ("failed", "cancelled", "unknown"):
            with self.subTest(status=status), self.assertRaises(AssertionError):
                probe.Wire.terminal({"status": status})

    def test_polling_uses_original_request_without_resubmission(self):
        session = object.__new__(probe.Session)
        submissions, queries = [], []

        def start(target, command, ident):
            submissions.append((target, command, ident))
            return {"version": 1, "status": "running", "request_id": ident}

        def finish(ident):
            queries.append(ident)
            return {"version": 1, "status": "completed", "request_id": ident, "receipt": {"outcome": {"dispatch": "unknown"}}}

        session.start, session.finish = start, finish
        result = session.run({"app_ref": "app"}, {"kind": "press"})
        self.assertEqual(len(submissions), 1)
        self.assertEqual(queries, [submissions[0][2]])
        self.assertEqual(result["receipt"]["outcome"]["dispatch"], "unknown")

    @unittest.skipUnless(os.name == "posix", "macOS Host file authority uses POSIX permissions")
    def test_host_denial_never_writes_grant_and_approval_is_private(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            app = {"identity": "owned-test-app", "pid": 123, "generation": "456"}
            host = probe.Host(root, app, "deny")
            self.assertIsNone(host.grant("denied"))
            self.assertEqual(list(root.glob("*.grant.json")), [])
            host = probe.Host(root, app, "approve")
            self.assertIsNone(host.grant("different-app", app={**app, "generation": "457"}))
            self.assertIsNone(host.grant("empty-capabilities", capabilities=[]))
            self.assertIsNone(host.grant("unsupported-capability", capabilities=["launch"]))
            path = host.grant("approved", capabilities=["observe"])
            info = path.stat()
            self.assertEqual(stat.S_IMODE(info.st_mode), 0o600)
            self.assertEqual(info.st_uid, os.getuid())
            self.assertEqual(json.loads(path.read_text())["app"], app)
            with self.assertRaises(AssertionError):
                host.grant("approved")
            decisions = [json.loads(line) for line in (root / "host-decisions.jsonl").read_text().splitlines()]
            self.assertEqual(sum(item["event"] == "grant_written" for item in decisions), 1)
            self.assertEqual(sum(item["event"] == "decision" and item["approved"] for item in decisions), 1)

    def test_caelis_credentials_and_remote_overrides_are_rejected(self):
        config = {"schema_version": 2, "runtime": {},
                  "models": {"provider_endpoints": [{"id": "local", "auth_type": "none", "base_url": "http://127.0.0.1:1234/v1"}]},
                  "model_profiles": {"default_profile_id": "provider:test", "profiles": [
                      {"id": "provider:test", "backend": {"provider": {"model_config_id": "test"}}},
                      {"id": "acp:excluded", "backend": {"acp": {"agent_id": "excluded"}}}]}}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "config.json"
            probe.write_json(path, config)
            selected = probe.isolated_caelis_config(path)
            self.assertEqual([profile["id"] for profile in selected["model_profiles"]["profiles"]], ["provider:test"])
            self.assertEqual(json.loads(path.read_text()), config, "source configuration changed")
            for field in ("token", "credential_ref", "extra_headers"):
                rejected = copy.deepcopy(config)
                rejected["models"]["provider_endpoints"][0][field] = "synthetic credential"
                probe.write_json(path, rejected)
                with self.subTest(field=field), self.assertRaises(AssertionError):
                    probe.isolated_caelis_config(path)
            for override in ({"base_url": "https://remote.example/v1"}, {"auth_type": "api_key"}):
                rejected = copy.deepcopy(config)
                rejected["models"]["configs"] = [override]
                probe.write_json(path, rejected)
                with self.subTest(override=override), self.assertRaises(AssertionError):
                    probe.isolated_caelis_config(path)


if __name__ == "__main__":
    unittest.main()
