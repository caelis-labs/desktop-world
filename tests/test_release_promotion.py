"""Promotion gates use real ZIPs/checksums; only OS signing and GitHub are mocked."""
import importlib.util
import io
import json
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import test_release_verification as fixtures

spec = importlib.util.spec_from_file_location('release_promoter', Path(__file__).parents[1] / 'scripts/promote-release.py')
p = importlib.util.module_from_spec(spec)
spec.loader.exec_module(p)


class PromotionTests(unittest.TestCase):
    def setUp(self):
        self.fixture = fixtures.ReleaseVerificationTests()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.assets = self.fixture.root
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.tag = 'v0.2.0'
        self.target = p.verifier.TARGETS[0]
        self.candidate = json.loads((self.assets / f'CANDIDATE-{self.target}.json').read_bytes())

    def outer(self):
        data = io.BytesIO()
        with zipfile.ZipFile(data, 'w') as archive:
            archive.writestr('bundle/CANDIDATE.json', (self.assets / f'CANDIDATE-{self.target}.json').read_bytes())
            for name in [self.candidate['archive'], self.candidate['archive'] + '.sha256', *p.verifier.RECEIPTS]:
                archive.writestr('bundle/' + name, (self.assets / name).read_bytes())
        return data.getvalue()

    def test_artifact_staging_preserves_original_bytes_and_notary(self):
        out = self.root / 'staged'; out.mkdir()
        data = self.outer()
        candidate = p.stage_candidate(data, 'sha256:' + p.digest(data), '1' * 40, self.tag, self.target, out)
        self.assertEqual(candidate, self.candidate)
        for path in out.iterdir():
            self.assertEqual(path.read_bytes(), (self.assets / path.name).read_bytes())

    def test_unaccepted_wrong_source_version_target_or_digest_cannot_stage(self):
        data = self.outer()
        for digest, source, tag, target in [('sha256:bad', '1' * 40, self.tag, self.target),
                                          ('sha256:' + p.digest(data), '2' * 40, self.tag, self.target),
                                          ('sha256:' + p.digest(data), '1' * 40, 'v0.2.1', self.target),
                                          ('sha256:' + p.digest(data), '1' * 40, self.tag, p.verifier.TARGETS[1])]:
            with self.subTest(tag=tag, target=target, source=source), self.assertRaises(ValueError):
                p.stage_candidate(data, digest, source, tag, target, self.root)

    def plan(self):
        return {'schema': 1, 'release': True, 'tag': self.tag, 'source': self.fixture.tag_commit,
                'tree': 'b' * 40, 'build_run_id': 10,
                'ci': {'source': self.fixture.tag_commit, 'tree': 'b' * 40, 'run_id': 4},
                'artifacts': {target: {'id': i + 7} for i, target in enumerate(p.verifier.TARGETS)}}

    def test_fetch_requires_exact_original_artifact_id_digest_run_and_source(self):
        data = self.outer()
        artifact = {'id': 7, 'name': 'dtw-macos-candidate-' + '1' * 40 + '-attempt-1',
                    'digest': 'sha256:' + p.digest(data), 'expired': False, 'workflow_run': {'id': 10}}
        plan = dict(self.plan(), source='1' * 40, artifacts={self.target: {key: artifact[key] for key in ('id', 'name', 'digest')}})
        def api(*args, **kwargs):
            return data if kwargs.get('binary') else artifact
        out = self.root / 'fetched'; out.mkdir()
        with patch.object(p, 'gh', side_effect=api):
            candidate, origin = p.fetch_candidate('owner/repo', plan, self.tag, self.target, out)
        self.assertEqual(candidate, self.candidate)
        self.assertEqual(origin['artifact_id'], 7)
        for field, value in [('expired', True), ('digest', 'sha256:' + 'f' * 64), ('workflow_run', {'id': 11})]:
            with self.subTest(field=field), patch.object(p, 'gh', return_value=dict(artifact, **{field: value})), self.assertRaises(ValueError):
                p.fetch_candidate('owner/repo', plan, self.tag, self.target, out)

    def prepared(self):
        out = self.root / 'prepared'; out.mkdir()
        for path in self.assets.iterdir():
            if path.name not in ('RELEASE.json', 'SHA256SUMS'):
                shutil.copyfile(path, out / path.name)
        return out

    def candidates(self):
        return [json.loads((self.assets / f'CANDIDATE-{target}.json').read_bytes()) for target in p.verifier.TARGETS]

    def test_resume_keeps_original_manifest_and_does_not_replace_any_existing_asset(self):
        out = self.prepared()
        p.prepare_release(out, self.assets, self.tag, self.fixture.tag_commit, self.candidates(), {},
                          {'source': self.fixture.tag_commit}, 'github-actions')
        for path in self.assets.iterdir():
            self.assertEqual(path.read_bytes(), (out / path.name).read_bytes())
        (self.assets / self.candidate['archive']).write_bytes(b'changed remotely')
        with self.assertRaisesRegex(ValueError, 'replacement is forbidden'):
            p.prepare_release(out, self.assets, self.tag, self.fixture.tag_commit, self.candidates(), {}, {}, 'github-actions')

    def test_wrong_existing_manifest_identity_cannot_be_overwritten(self):
        release = self.fixture.release
        release['tag_commit'] = 'b' * 40
        (self.assets / 'RELEASE.json').write_text(json.dumps(release))
        with self.assertRaisesRegex(ValueError, 'identity mismatch'):
            p.prepare_release(self.prepared(), self.assets, self.tag, self.fixture.tag_commit, self.candidates(), {}, {}, 'github-actions')

    def test_incomplete_construction_is_rejected_before_api_or_filesystem_mutation(self):
        with patch.object(p, 'gh') as api, self.assertRaises(ValueError):
            p.promote('owner/repo', {'tag': self.tag}, 'github-actions', self.root / 'output')
        api.assert_not_called()
        self.assertFalse((self.root / 'output').exists())

    def exercise(self, *, bad_signature=False, already_published=False, corrupt_after_publish=False, newer_latest=False,
                 partial_upload=False, incomplete_published=False, tag_change_at=None):
        remote = self.root / 'remote'; remote.mkdir()
        if already_published or partial_upload:
            for path in self.assets.iterdir():
                if already_published or path.name in ('RELEASE.json', 'SHA256SUMS', self.candidate['archive']):
                    shutil.copyfile(path, remote / path.name)
        if incomplete_published:
            (remote / self.candidate['archive']).unlink()
        state = {'draft': not already_published, 'mutations': []}
        commit_reads = 0
        def api(*args, **kwargs):
            nonlocal commit_reads
            self.assertIn('/commits/' + self.tag, args[-1])
            commit_reads += 1
            return {'sha': 'f' * 40 if tag_change_at and commit_reads >= tag_change_at else self.fixture.tag_commit}
        def candidate(repo, run, tag, target, out):
            c = json.loads((self.assets / f'CANDIDATE-{target}.json').read_bytes())
            names = [c['archive'], c['archive'] + '.sha256', f'BUILD-{target}.json', f'CANDIDATE-{target}.json']
            if target == self.target:
                names += list(p.verifier.RECEIPTS)
            for name in names:
                shutil.copyfile(self.assets / name, out / name)
            return c, {'run_id': run}
        def record(*_):
            return {'id': 3, 'draft': state['draft'], 'assets': [{'name': f.name} for f in remote.iterdir()]}
        def download(repo, tag, out):
            out.mkdir()
            for path in remote.iterdir():
                shutil.copyfile(path, out / path.name)
        def command(args, **_):
            state['mutations'].append(args[2])
            self.assertNotIn('--clobber', args)
            if args[2] == 'upload':
                for name in args[6:]:
                    path = Path(name)
                    self.assertFalse((remote / path.name).exists(), 'Existing asset must never be replaced')
                    shutil.copyfile(path, remote / path.name)
            elif args[2] == 'edit':
                self.assertIn('--latest=false' if newer_latest else '--latest', args)
                state['draft'] = False
                if corrupt_after_publish:
                    (remote / self.candidate['archive']).write_bytes(b'corrupted after publication')
            else:
                self.fail(args)
        with patch.object(p, 'fetch_candidate', side_effect=candidate), patch.object(p, 'release_record', side_effect=record), \
                patch.object(p, 'gh', side_effect=api), patch.object(p, 'download', side_effect=download), \
                patch.object(p.subprocess, 'run', side_effect=command), patch.object(p.verifier.builder, 'binary_format'), \
                patch.object(p, 'latest_allowed', return_value=not newer_latest), \
                patch.object(p.verifier, 'signature', side_effect=ValueError('bad signature') if bad_signature else None):
            try:
                report = p.promote('owner/repo', self.plan(), 'github-actions', self.root / 'output')
            except ValueError:
                if bad_signature:
                    self.assertEqual(state['mutations'], [])
                elif corrupt_after_publish:
                    self.assertEqual(state['mutations'], ['upload', 'edit'])
                elif incomplete_published:
                    self.assertEqual(state['mutations'], [])
                elif tag_change_at:
                    self.assertEqual(state['mutations'], {1: [], 2: ['upload'], 3: ['upload', 'edit']}[tag_change_at])
                raise
        return report, state

    def test_signature_gate_precedes_upload_and_publication(self):
        with self.assertRaisesRegex(ValueError, 'bad signature'):
            self.exercise(bad_signature=True)

    def test_draft_is_verified_uploaded_preflighted_published_and_audited_again(self):
        report, state = self.exercise()
        self.assertEqual(state['mutations'], ['upload', 'edit'])
        self.assertEqual(report['status'], 'published')
        self.assertEqual(report['preflight']['assets_verified'], 13)
        self.assertEqual(report['published_audit']['assets_verified'], 13)

    def test_published_rerun_is_audit_only(self):
        report, state = self.exercise(already_published=True)
        self.assertEqual(state['mutations'], [])
        self.assertEqual(report['status'], 'already_published')

    def test_partial_upload_resumes_without_replacing_existing_bytes(self):
        report, state = self.exercise(partial_upload=True)
        self.assertEqual(state['mutations'], ['upload', 'edit'])
        self.assertEqual(report['published_audit']['assets_verified'], 13)

    def test_incomplete_published_release_is_never_repaired(self):
        with self.assertRaisesRegex(ValueError, 'Published release is incomplete'):
            self.exercise(already_published=True, incomplete_published=True)

    def test_older_complete_release_does_not_replace_latest(self):
        report, state = self.exercise(newer_latest=True)
        self.assertEqual(state['mutations'], ['upload', 'edit'])
        self.assertEqual(report['status'], 'published')

    def test_latest_selection_ignores_drafts_prereleases_and_rejects_downgrade(self):
        releases = [[{'tag_name': 'v0.4.0', 'draft': False, 'prerelease': False},
                     {'tag_name': 'v9.0.0', 'draft': True, 'prerelease': False}]]
        with patch.object(p, 'gh', return_value=releases):
            self.assertFalse(p.latest_allowed('owner/repo', 'v0.3.0'))
            self.assertTrue(p.latest_allowed('owner/repo', 'v0.4.0'))
            self.assertTrue(p.latest_allowed('owner/repo', 'v0.5.0'))

    def test_new_release_records_automated_ci_without_operator_attestation(self):
        existing = self.root / 'empty'; existing.mkdir()
        output = self.prepared()
        p.prepare_release(output, existing, self.tag, self.fixture.tag_commit, self.candidates(), {}, self.plan()['ci'], 'github-actions')
        record = json.loads((output / 'RELEASE.json').read_text())
        self.assertEqual(record['acceptance']['status'], 'automated_ci')
        self.assertEqual(record['acceptance']['ci'], self.plan()['ci'])
        self.assertNotIn('notes', record['acceptance'])

    def test_postpublication_change_is_reported(self):
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            self.exercise(corrupt_after_publish=True)

    def test_tag_changes_are_rejected_before_upload_before_publish_and_after_publish(self):
        for position in (1, 2, 3):
            with self.subTest(position=position), self.assertRaisesRegex(ValueError, 'tag moved'):
                # Each run has its own evidence directories and remote assets.
                independent = PromotionTests()
                independent.setUp()
                try:
                    independent.exercise(tag_change_at=position)
                finally:
                    independent.doCleanups()


if __name__ == '__main__':
    unittest.main()
