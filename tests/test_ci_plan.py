import importlib.util
import io
import json
from pathlib import Path
import unittest
import zipfile

spec = importlib.util.spec_from_file_location('ci_plan', Path(__file__).parent / 'ci/plan.py')
plan = importlib.util.module_from_spec(spec)
spec.loader.exec_module(plan)


class PlanTests(unittest.TestCase):
    def test_platform_selection(self):
        cases = [(['README.md'], (False, False, False)), (['tests/acceptance/package_store_probe.py'], (True, True, True)),
                 # Both package layouts are covered by portable Python/Node
                 # contracts; manifest generation does not compile a native adapter.
                 (['scripts/plugin_package.py', 'plugins/cordis/index.mjs'], (True, True, True)),
                 (['cmd/dtw/src/backends/macos/Native.swift', 'docs/old-native.txt'], (True, False, False)),
                 (['cmd/dtw/src/script/mod.rs'], (True, True, True)),
                 (['cmd/dtw/Cargo.lock'], (True, True, True)),
                 (['skills/dtw/API.md'], (True, True, True)),
                 (['.cargo/config.toml'], (True, True, True)),
                 (['cmd/dtw/src/backends/macos/Input.swift'], (True, False, False)),
                 (['tests/fixtures/app-scene/test_telemetry.py'], (True, False, False)),
                 (['cmd/dtw/src/backends/windows/native.rs'], (False, True, False)),
                 (['tests/acceptance/windows/smoke.ps1'], (False, True, False)),
                 (['install/bootstrap.ps1'], (False, True, False)),
                 (['install/bootstrap.sh'], (True, False, False)),
                 (['cmd/dtw/src/backends/linux/inventory.rs'], (False, False, True)),
                 (['.github/workflows/ci.yml'], (True, True, True)),
                 (['tests/ci/plan.py'], (True, True, True))]
        for paths, expected in cases:
            with self.subTest(paths=paths):
                self.assertEqual(plan.native_checks(paths), expected)

    def test_select_checks_gates_package_upgrade_and_linux_gui(self):
        cases = [
            (['version.txt', 'cmd/dtw/Cargo.toml', 'cmd/dtw/Cargo.lock', 'CHANGELOG.md', '.release-please-manifest.json'],
             (True, True, True, False, False)),
            (['cmd/dtw/src/backends/macos/Input.swift'], (True, False, False, False, False)),
            (['cmd/dtw/src/backends/linux/inventory.rs'], (False, False, True, False, True)),
            (['cmd/dtw/src/package/setup.rs'], (True, True, True, True, False)),
            (['cmd/dtw/src/app_protocol/mcp.rs'], (True, True, True, False, True)),
            (['.github/workflows/rewrite-linux-candidate.yml', 'tests/ci/install_linux_x11_deps.sh'],
             (False, False, True, False, True)),
            (['scripts/check-linux-candidate.py'], (False, False, True, False, False)),
            (['.github/workflows/rewrite-macos-candidate.yml'], (True, False, False, False, False)),
            (['.github/workflows/rewrite-candidate.yml'], (False, True, False, False, False)),
            (['.github/workflows/ci.yml'], (True, True, True, True, True)),
        ]
        for paths, expected in cases:
            with self.subTest(paths=paths):
                self.assertEqual(plan.select_checks(paths), expected)

    def evidence(self, *, tree='tree', event='pull_request', expired=False, conclusion='success', attempt=1, archive_name='verified.json'):
        record = {'schema': 1, 'tree': tree, 'run_id': 42, 'attempt': attempt, 'event': event, 'checks': 'passed'}
        data = io.BytesIO()
        with zipfile.ZipFile(data, 'w') as archive:
            archive.writestr(archive_name, json.dumps(record))
        def api(path, raw=False):
            if path.endswith('/pulls'):
                return [{'merged_at': 'now', 'merge_commit_sha': 'merged', 'head': {'sha': 'head'}}]
            if '/workflows/' in path:
                return {'workflow_runs': [{'event': event, 'head_sha': 'head', 'conclusion': conclusion, 'id': 42, 'run_attempt': 1}]}
            if path.endswith('/artifacts'):
                return {'artifacts': [{'name': 'ci-verification-42-1', 'expired': expired, 'id': 7}]}
            self.assertTrue(raw)
            return data.getvalue()
        return api

    def test_reuses_only_successful_exact_tree(self):
        self.assertEqual(plan.reusable_run('owner/repo', 'merged', 'tree', self.evidence()), '42')

    def test_changed_base_or_tree_requires_fresh_checks(self):
        self.assertEqual(plan.reusable_run('owner/repo', 'merged', 'different-tree', self.evidence()), '')

    def test_wrong_commit_cannot_reuse(self):
        self.assertEqual(plan.reusable_run('owner/repo', 'other', 'tree', self.evidence()), '')

    def test_rejects_failed_wrong_attempt_expired_or_non_pr_evidence(self):
        for options in [{'event': 'workflow_dispatch'}, {'conclusion': 'failure'}, {'expired': True},
                        {'attempt': 2}, {'archive_name': '../verified.json'}]:
            with self.subTest(options=options):
                self.assertEqual(plan.reusable_run('owner/repo', 'merged', 'tree', self.evidence(**options)), '')


if __name__ == '__main__':
    unittest.main()
