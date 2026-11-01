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
        cases = [(['README.md'], (False, False)),
                 # Both package layouts are covered by portable Python/Node
                 # contracts; manifest generation does not compile a native adapter.
                 (['scripts/plugin_package.py', 'plugins/cordis/index.mjs'], (False, False)),
                 (['cmd/dtw/src/backends/macos/Native.swift', 'docs/old-native.txt'], (True, False)),
                 (['cmd/dtw/src/script/mod.rs'], (True, True)),
                 (['cmd/dtw/Cargo.lock'], (True, True)),
                 (['.cargo/config.toml'], (True, True)),
                 (['cmd/dtw/src/backends/macos/Input.swift'], (True, False)),
                 (['cmd/dtw/src/backends/windows/native.rs'], (False, True)),
                 (['tests/acceptance/windows/smoke.ps1'], (False, True)),
                 (['cmd/dtw/src/backends/linux/inventory.rs'], (False, False)),
                 (['.github/workflows/ci.yml'], (True, True)),
                 (['tests/ci/plan.py'], (True, True))]
        for paths, expected in cases:
            with self.subTest(paths=paths):
                self.assertEqual(plan.native_checks(paths), expected)

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
