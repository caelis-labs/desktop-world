"""Automatic release admission uses real ZIP receipts; only GitHub is mocked."""
import base64
import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import unittest
from unittest.mock import patch
import zipfile

spec = importlib.util.spec_from_file_location('release_plan', Path(__file__).parents[1] / 'scripts/release-plan.py')
plan = importlib.util.module_from_spec(spec)
spec.loader.exec_module(plan)

REPO = 'owner/repo'
SOURCE = 'a' * 40
LATER = 'b' * 40
TREE = '1' * 40
LATER_TREE = '2' * 40


def zipped(filename, value, *, extra=False):
    output = io.BytesIO()
    with zipfile.ZipFile(output, 'w') as archive:
        archive.writestr(filename, json.dumps(value))
        if extra:
            archive.writestr('unexpected.txt', 'not part of the receipt')
    return output.getvalue()


def run(identifier=10, source=SOURCE, workflow=plan.CI_WORKFLOW, event='push', attempt=1):
    return {'id': identifier, 'run_attempt': attempt, 'path': workflow, 'event': event,
            'head_sha': source, 'head_branch': 'main', 'status': 'completed', 'conclusion': 'success',
            'repository': {'full_name': REPO}, 'head_repository': {'full_name': REPO}}


class ReleasePlanTests(unittest.TestCase):
    def setUp(self):
        self.routes, self.calls = {}, []
        self.api = patch.object(plan, 'gh', side_effect=self.gh)
        self.api.start()
        self.addCleanup(self.api.stop)

    def gh(self, *arguments, binary=False):
        self.calls.append((arguments, binary))
        key = arguments[-1]
        if key not in self.routes:
            self.fail('Unexpected GitHub request: ' + repr(arguments))
        result = self.routes[key]
        self.assertEqual(isinstance(result, bytes), binary)
        return copy.deepcopy(result)

    def route(self, path, value):
        self.routes['repos/' + REPO + '/' + path] = value
        return value

    def evidence(self, identifier, name, filename, record, *, expired=False, extra=False):
        data = zipped(filename, record, extra=extra)
        artifact = {'id': identifier, 'name': name, 'expired': expired,
                    'digest': 'sha256:' + hashlib.sha256(data).hexdigest()}
        self.route(f'actions/artifacts/{identifier}/zip', data)
        return artifact

    def ci(self, source=SOURCE, tree=TREE, identifier=10, attempt=1):
        record = {'schema': 1, 'tree': tree, 'run_id': identifier, 'attempt': attempt,
                  'event': 'push', 'checks': 'passed'}
        artifact = self.evidence(1000 + identifier, f'ci-verification-{identifier}-{attempt}', 'verified.json', record)
        self.route(f'actions/workflows/ci.yml/runs?event=push&branch=main&head_sha={source}&status=success&per_page=100',
                   {'workflow_runs': [run(identifier, source, attempt=attempt)]})
        self.route(f'actions/runs/{identifier}/artifacts?per_page=100', {'artifacts': [artifact]})
        return record, artifact

    def gate(self):
        return {'eligible': True, 'source': SOURCE,
                'ci': {'source': SOURCE, 'tree': TREE, 'run_id': 10, 'attempt': 1}}

    def pending(self, *, published=False, missing=False, source=LATER, tree=LATER_TREE):
        self.route(f'contents/version.txt?ref={SOURCE}', {'content': base64.b64encode(b'0.2.3\n').decode()})
        self.route('releases?per_page=100', [[] if missing else [
            {'tag_name': 'v0.2.3', 'draft': not published, 'prerelease': False}]])
        self.route('commits/v0.2.3', {'sha': source, 'commit': {'tree': {'sha': tree}}})
        self.route(f'contents/version.txt?ref={source}', {'content': base64.b64encode(b'0.2.3\n').decode()})
        self.route(f'actions/artifacts?name=release-plan-{source}&per_page=100', {'artifacts': []})

    def complete_plan(self):
        return {'schema': 1, 'release': True, 'tag': 'v0.2.3', 'source': SOURCE, 'tree': TREE,
                'ci': {'source': SOURCE, 'tree': TREE, 'run_id': 10, 'attempt': 1},
                'build_run_id': 50, 'build_attempt': 2,
                'artifacts': {target: {'id': index + 100, 'name': 'owned', 'digest': 'sha256:' + '3' * 64}
                              for index, target in enumerate(plan.TARGETS)}}

    def install_plan(self, record=None, *, expired=False, extra=False):
        record = self.complete_plan() if record is None else record
        self.route('actions/runs/50/attempts/2', run(50, workflow=plan.BUILD_WORKFLOW, event='workflow_run', attempt=2))
        artifact = self.evidence(700, 'release-plan-' + SOURCE, 'release-plan.json', record,
                                 expired=expired, extra=extra)
        self.route('actions/runs/50/artifacts?per_page=100', {'artifacts': [artifact]})
        return artifact

    def test_stale_trigger_never_looks_up_green_ci_or_admits_release_please(self):
        self.route('commits/main', {'sha': LATER, 'commit': {'tree': {'sha': LATER_TREE}}})
        result = plan.gate(REPO, 'workflow_run', {'workflow_run': run()}, SOURCE)
        self.assertFalse(result['eligible'])
        self.assertEqual([call[0][-1] for call in self.calls], ['repos/' + REPO + '/commits/main'])

    def test_foreign_wrong_branch_failed_or_wrong_trigger_rejected_before_api(self):
        for update in [{'head_repository': {'full_name': 'foreign/repo'}},
                       {'repository': {'full_name': 'foreign/repo'}}, {'head_branch': 'topic'},
                       {'event': 'pull_request'}, {'conclusion': 'failure'}, {'status': 'in_progress'},
                       {'path': '.github/workflows/other.yml'}]:
            with self.subTest(update=update), self.assertRaises(ValueError):
                plan.gate(REPO, 'workflow_run', {'workflow_run': dict(run(), **update)}, SOURCE)
        with self.assertRaises(ValueError):
            plan.gate(REPO, 'pull_request', {}, SOURCE)
        self.assertEqual(self.calls, [])

    def test_exact_main_source_and_real_receipt_admit_release_maintenance(self):
        self.route('commits/main', {'sha': SOURCE, 'commit': {'tree': {'sha': TREE}}})
        _, artifact = self.ci()
        result = plan.gate(REPO, 'workflow_run', {'workflow_run': run()}, SOURCE)
        self.assertTrue(result['eligible'])
        self.assertEqual(result['ci']['artifact_id'], artifact['id'])
        self.assertEqual(result['ci']['artifact_digest'], artifact['digest'])
        self.assertEqual((result['source'], result['ci']['tree']), (SOURCE, TREE))

    def test_main_advancing_during_ci_lookup_does_not_admit_release_please(self):
        self.route('commits/main', {'sha': SOURCE, 'commit': {'tree': {'sha': TREE}}})
        def verified(*_):
            self.route('commits/main', {'sha': LATER, 'commit': {'tree': {'sha': LATER_TREE}}})
            return {'source': SOURCE, 'tree': TREE}
        with patch.object(plan, 'successful_ci', side_effect=verified):
            self.assertFalse(plan.gate(REPO, 'workflow_run', {'workflow_run': run()}, SOURCE)['eligible'])

    def test_wrong_digest_expired_receipt_extra_member_and_unverified_tree_rejected(self):
        for corruption in ['digest', 'expired', 'tree', 'checks', 'members']:
            with self.subTest(corruption=corruption):
                record, artifact = self.ci()
                if corruption == 'digest':
                    artifact['digest'] = 'sha256:' + '0' * 64
                elif corruption == 'expired':
                    artifact['expired'] = True
                else:
                    if corruption == 'tree': record['tree'] = LATER_TREE
                    if corruption == 'checks': record['checks'] = 'failed'
                    replacement = self.evidence(artifact['id'], artifact['name'], 'verified.json', record,
                                                extra=corruption == 'members')
                    artifact.update(replacement)
                with self.assertRaises(ValueError):
                    plan.successful_ci(REPO, SOURCE, TREE)

    def test_ci_source_and_head_branch_are_not_inferred_from_list_query(self):
        for update in [{'head_sha': LATER}, {'head_branch': 'topic'},
                       {'head_repository': {'full_name': 'fork/repo'}}]:
            self.ci()
            endpoint = f'actions/workflows/ci.yml/runs?event=push&branch=main&head_sha={SOURCE}&status=success&per_page=100'
            self.route(endpoint, {'workflow_runs': [dict(run(), **update)]})
            with self.subTest(update=update), self.assertRaises(ValueError):
                plan.successful_ci(REPO, SOURCE, TREE)

    def test_post_release_please_tag_source_gets_its_own_independent_ci(self):
        self.pending()
        self.ci(LATER, LATER_TREE, 22)
        result = plan.pending_plan(REPO, self.gate(), 50)
        self.assertTrue(result['release'])
        self.assertEqual((result['source'], result['tree'], result['ci']['run_id']), (LATER, LATER_TREE, 22))
        queried = [call[0][-1] for call in self.calls]
        self.assertTrue(any('head_sha=' + LATER in path for path in queried))
        self.assertFalse(any('head_sha=' + SOURCE in path for path in queried))

    def test_unverified_new_tag_cannot_inherit_old_gate_green(self):
        self.pending()
        self.route(f'actions/workflows/ci.yml/runs?event=push&branch=main&head_sha={LATER}&status=success&per_page=100',
                   {'workflow_runs': []})
        with self.assertRaises(ValueError):
            plan.pending_plan(REPO, self.gate(), 50)

    def test_tag_source_version_mismatch_rejects_before_ci_or_construction(self):
        self.pending()
        self.route(f'contents/version.txt?ref={LATER}', {'content': base64.b64encode(b'0.2.4\n').decode()})
        with self.assertRaisesRegex(ValueError, 'Tag source version mismatch'):
            plan.pending_plan(REPO, self.gate(), 50)
        self.assertFalse(any('head_sha=' in call[0][-1] for call in self.calls))

    def test_ordinary_published_or_no_release_does_not_start_construction(self):
        for options in [{'published': True}, {'missing': True}]:
            self.pending(**options)
            before = len(self.calls)
            result = plan.pending_plan(REPO, self.gate(), 50)
            self.assertFalse(result['release'])
            self.assertEqual(len(self.calls) - before, 2)

    def test_previous_successful_construction_preserves_its_immutable_origin(self):
        self.pending()
        self.ci(LATER, LATER_TREE, 22)
        previous = self.evidence(800, 'release-plan-' + LATER, 'release-plan.json', {'source': LATER, 'tag': 'v0.2.3'})
        previous['workflow_run'] = {'id': 49}
        self.route(f'actions/artifacts?name=release-plan-{LATER}&per_page=100', {'artifacts': [previous]})
        self.route('actions/runs/49', run(49, LATER, plan.BUILD_WORKFLOW, 'workflow_run'))
        result = plan.pending_plan(REPO, self.gate(), 50)
        self.assertFalse(result['release'])
        self.assertIn('retry its publication', result['reason'])

    def test_expired_prior_successful_construction_cannot_silently_rebuild_pending_release(self):
        self.pending()
        self.ci(LATER, LATER_TREE, 22)
        previous = {'id': 800, 'name': 'release-plan-' + LATER, 'expired': True,
                    'workflow_run': {'id': 49}}
        self.route(f'actions/artifacts?name=release-plan-{LATER}&per_page=100', {'artifacts': [previous]})
        self.route('actions/runs/49', run(49, LATER, plan.BUILD_WORKFLOW, 'workflow_run'))
        with self.assertRaisesRegex(ValueError, 'Previous construction plan expired'):
            plan.pending_plan(REPO, self.gate(), 50)
        self.assertFalse(any(call[0][-1].endswith('/800/zip') for call in self.calls))

    def test_rerunning_completed_construction_never_resigns_before_plan_name_conflict(self):
        self.pending()
        self.ci(LATER, LATER_TREE, 22)
        previous = {'id': 800, 'name': 'release-plan-' + LATER, 'expired': False,
                    'workflow_run': {'id': 50}}
        self.route(f'actions/artifacts?name=release-plan-{LATER}&per_page=100', {'artifacts': [previous]})
        with self.assertRaisesRegex(ValueError, 'retry its original publication'):
            plan.pending_plan(REPO, self.gate(), 50)

    def platform(self, target, identifier, attempt):
        prefix = 'dtw-macos-candidate-' if target == plan.TARGETS[0] else 'dtw-windows-candidate-'
        return self.route(f'actions/artifacts/{identifier}',
                          {'id': identifier, 'name': prefix + SOURCE + f'-attempt-{attempt}',
                           'digest': 'sha256:' + str(identifier % 10) * 64, 'expired': False,
                           'workflow_run': {'id': 50}})

    def test_collect_accepts_mixed_successful_job_attempts_with_exact_artifact_ids(self):
        mac = self.platform(plan.TARGETS[0], 101, 2)
        win = self.platform(plan.TARGETS[1], 102, 1)
        result = plan.collect(REPO, self.complete_plan(), 50, 2,
                              {plan.TARGETS[0]: mac['id'], plan.TARGETS[1]: win['id']})
        self.assertEqual(result['build_attempt'], 2)
        self.assertEqual(result['artifacts'][plan.TARGETS[0]], {key: mac[key] for key in ('id', 'name', 'digest')})
        self.assertEqual(result['artifacts'][plan.TARGETS[1]], {key: win[key] for key in ('id', 'name', 'digest')})

    def test_collect_rejects_foreign_run_source_expiry_or_digest(self):
        for update in [{'workflow_run': {'id': 99}}, {'expired': True}, {'digest': 'bad'},
                       {'name': 'dtw-macos-candidate-' + LATER + '-attempt-2'}]:
            self.platform(plan.TARGETS[0], 101, 2).update(update)
            self.platform(plan.TARGETS[1], 102, 1)
            with self.subTest(update=update), self.assertRaises(ValueError):
                plan.collect(REPO, self.complete_plan(), 50, 2, dict(zip(plan.TARGETS, [101, 102])))

    def test_load_requires_successful_same_repository_main_build_workflow(self):
        self.install_plan()
        self.assertEqual(plan.load(REPO, 50, 2), self.complete_plan())
        for update in [{'head_repository': {'full_name': 'fork/repo'}}, {'head_branch': 'topic'},
                       {'path': plan.CI_WORKFLOW}, {'status': 'in_progress'}, {'conclusion': 'failure'}]:
            self.route('actions/runs/50/attempts/2', dict(run(50, workflow=plan.BUILD_WORKFLOW,
                       event='workflow_run', attempt=2), **update))
            with self.subTest(update=update), self.assertRaises(ValueError):
                plan.load(REPO, 50, 2)

    def test_maintenance_run_without_plan_returns_none(self):
        self.route('actions/runs/50/attempts/2', run(50, workflow=plan.BUILD_WORKFLOW, event='workflow_run', attempt=2))
        self.route('actions/runs/50/artifacts?per_page=100', {'artifacts': []})
        self.assertIsNone(plan.load(REPO, 50, 2))

    def test_expired_construction_is_not_mistaken_for_pr_maintenance(self):
        self.install_plan()
        self.routes['repos/' + REPO + '/actions/runs/50/artifacts?per_page=100']['artifacts'][0]['expired'] = True
        with self.assertRaisesRegex(ValueError, 'expired'):
            plan.load(REPO, 50, 2)

    def test_load_rejects_malicious_plan_and_wrong_attempt_or_ci_binding(self):
        for update in [{'schema': 2}, {'release': False}, {'build_run_id': 99}, {'build_attempt': 1},
                       {'tag': 'v0.2.3; echo bad'}, {'source': 'not-a-sha'},
                       {'artifacts': {plan.TARGETS[0]: {}}},
                       {'ci': {'source': LATER, 'tree': TREE}},
                       {'ci': {'source': SOURCE, 'tree': LATER_TREE}}]:
            self.install_plan(dict(self.complete_plan(), **update))
            with self.subTest(update=update), self.assertRaises(ValueError):
                plan.load(REPO, 50, 2)

    def test_load_rejects_tampered_or_ambiguous_plan_zip(self):
        artifact = self.install_plan(extra=True)
        with self.assertRaises(ValueError): plan.load(REPO, 50, 2)
        artifact = self.install_plan()
        artifact['digest'] = 'sha256:' + '0' * 64
        with self.assertRaises(ValueError): plan.load(REPO, 50, 2)
        artifact = self.install_plan()
        self.route('actions/runs/50/artifacts?per_page=100', {'artifacts': [artifact, dict(artifact, id=701)]})
        with self.assertRaises(ValueError): plan.load(REPO, 50, 2)


if __name__ == '__main__':
    unittest.main()
