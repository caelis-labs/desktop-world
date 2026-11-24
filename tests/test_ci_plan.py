import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
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
            (['README.md', 'docs/install-agy.md', 'version.txt', 'CHANGELOG.md', '.release-please-manifest.json'],
             (False, False, False, False, False)),
            (['cmd/dtw/Cargo.toml'], (True, True, True, True, True)),
            (['cmd/dtw/Cargo.lock'], (True, True, True, True, True)),
            (['cmd/dtw/build.rs'], (True, True, True, True, True)),
            (['cmd/dtw/src/backends/input_sequence.rs'], (True, True, True, True, True)),
            (['cmd/dtw/src/backends/macos/Input.swift'], (True, False, False, False, False)),
            (['cmd/dtw/src/backends/linux/inventory.rs'], (False, False, True, True, True)),
            (['cmd/dtw/src/backends/linux/worker.rs'], (False, False, True, True, True)),
            (['cmd/dtw/src/package/setup.rs'], (True, True, True, True, False)),
            (['cmd/dtw/src/runtime/mod.rs'], (True, True, True, False, True)),
            (['cmd/dtw/src/app_protocol/mcp.rs'], (True, True, True, True, True)),
            (['.github/workflows/rewrite-linux-candidate.yml', 'tests/ci/install_linux_x11_deps.sh'],
             (False, False, True, False, True)),
            (['scripts/check-linux-candidate.py'], (False, False, True, False, False)),
            (['.github/workflows/rewrite-macos-candidate.yml'], (True, False, False, False, False)),
            (['.github/workflows/rewrite-candidate.yml'], (False, True, False, False, False)),
            (['.github/workflows/ci.yml'], (True, True, True, True, True)),
        ]
        for paths, expected in cases:
            with self.subTest(paths=paths):
                self.assertEqual(plan.select_checks(paths)[:5], expected)

    def test_linux_candidate_selection(self):
        candidate_inputs = [
            '.gitattributes',
            '.github/workflows/ci.yml',
            '.github/workflows/rewrite-linux-candidate.yml',
            '.github/workflows/release-please.yml',
            '.cargo/config.toml',
            'cmd/dtw/.cargo/config.toml',
            'rust-toolchain.toml',
            'cmd/dtw/Cargo.lock',
            'cmd/dtw/build.rs',
            'cmd/dtw/src/package/files.rs',
            'packaging/third-party/overrides.json',
            'plugins/pi/index.mjs',
            'scripts/build-rewrite-candidate.py',
            'scripts/check-linux-candidate.py',
            'scripts/plugin_package.py',
            'scripts/verify-release.py',
            'tests/ci/plan.py',
            'tests/ci/run_packaged_mcp.py',
            'version.txt',
        ]
        other_inputs = [
            'README.md',
            'docs/building.md',
            'docs/install-pi.md',
            'docs/examples/mcp.json',
            'install.md',
            'skills/dtw/SKILL.md',
            '.github/workflows/rewrite-candidate.yml',
            '.github/workflows/rewrite-macos-candidate.yml',
            'cmd/dtw/src/runtime/mod.rs',
            'cmd/dtw/src/backends/linux/worker.rs',
            'cmd/dtw/src/backends/macos/Input.swift',
            'tests/acceptance/linux_atspi_probe.py',
            'tests/ci/install_linux_x11_deps.sh',
            'tests/ci/check_package_upgrade.py',
        ]
        for paths, selected in ((candidate_inputs, True), (other_inputs, False)):
            for path in paths:
                with self.subTest(path=path):
                    self.assertEqual(plan.select_checks([path])[5], selected)

    def test_linux_candidate_runs_for_dispatch_and_push_without_reuse(self):
        for event, reuse, expected in (
            ('workflow_dispatch', '', 'true'),
            ('push', '', 'true'),
            ('push', '42', 'false'),
        ):
            with self.subTest(event=event, reuse=reuse), tempfile.TemporaryDirectory() as temp:
                output = Path(temp) / 'output'
                env = {'GITHUB_EVENT_NAME': event, 'GITHUB_OUTPUT': str(output),
                       'GITHUB_REPOSITORY': 'owner/repo', 'GITHUB_SHA': 'a' * 40}
                with patch.dict(os.environ, env), patch.object(plan, 'git', return_value='b' * 40), \
                        patch.object(plan, 'reusable_run', return_value=reuse), patch('sys.stdout', new_callable=io.StringIO):
                    plan.main()
                options = dict(line.split('=', 1) for line in output.read_text().splitlines())
                self.assertEqual(options['linux_candidate'], expected)

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
