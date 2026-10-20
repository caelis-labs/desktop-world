import copy
import importlib.util
from pathlib import Path
import unittest

path = Path(__file__).resolve().parents[2] / "scripts/source-check-proof.py"
spec = importlib.util.spec_from_file_location("source_check_proof", path)
proof = importlib.util.module_from_spec(spec)
spec.loader.exec_module(proof)


class SourceProof(unittest.TestCase):
    def setUp(self):
        self.sha = "1" * 40
        self.repo = "caelis-labs/desktop-world"
        self.run = dict(head_sha=self.sha, event="push", head_branch="main",
                        head_repository={"full_name": self.repo}, path=".github/workflows/ci.yml",
                        status="completed", conclusion="success")
        self.jobs = [dict(name=name, status="completed", conclusion="success") for name in proof.REQUIRED]

    def test_exact_success_can_be_reused(self):
        self.assertTrue(proof.proven(self.run, self.jobs, self.repo, self.sha))

    def test_wrong_sha_event_branch_repo_or_workflow_never_reused(self):
        for key, value in [("head_sha", "2" * 40), ("event", "pull_request"),
                           ("head_branch", "other"), ("head_repository", {"full_name": "other/repo"}),
                           ("path", ".github/workflows/other.yml"), ("status", "in_progress"),
                           ("conclusion", "failure")]:
            with self.subTest(key=key):
                run = {**self.run, key: value}
                self.assertFalse(proof.proven(run, self.jobs, self.repo, self.sha))

    def test_missing_failed_skipped_pending_duplicate_jobs_never_reused(self):
        self.assertFalse(proof.proven(self.run, [], self.repo, self.sha))
        for name in proof.REQUIRED:
            self.assertFalse(proof.proven(self.run, [j for j in self.jobs if j["name"] != name], self.repo, self.sha))
            pending = copy.deepcopy(self.jobs)
            next(j for j in pending if j["name"] == name)["status"] = "in_progress"
            self.assertFalse(proof.proven(self.run, pending, self.repo, self.sha))
            for status in ("failure", "cancelled", "skipped", "neutral", None):
                with self.subTest(name=name, status=status):
                    jobs = copy.deepcopy(self.jobs)
                    next(j for j in jobs if j["name"] == name)["conclusion"] = status
                    self.assertFalse(proof.proven(self.run, jobs, self.repo, self.sha))
        self.assertFalse(proof.proven(self.run, self.jobs * 2, self.repo, self.sha))

    def test_short_sha_never_reused(self):
        self.assertFalse(proof.proven({**self.run, "head_sha": "1234"}, self.jobs, self.repo, "1234"))


if __name__ == "__main__":
    unittest.main()
