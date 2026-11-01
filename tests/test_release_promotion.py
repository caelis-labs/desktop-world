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

    def test_candidate_run_must_be_successful_correct_workflow_and_same_repository(self):
        run = {'conclusion': 'success', 'path': p.WORKFLOWS[self.target], 'head_sha': '1' * 40,
               'repository': {'full_name': 'owner/repo'}, 'head_repository': {'full_name': 'owner/repo'}}
        p.validate_run(run, 'owner/repo', p.WORKFLOWS[self.target])
        for key, value in [('conclusion', 'failure'), ('path', 'other.yml'), ('head_sha', 'bad'),
                           ('head_repository', {'full_name': 'foreign/repo'})]:
            with self.subTest(key=key), self.assertRaises(ValueError):
                p.validate_run(dict(run, **{key: value}), 'owner/repo', p.WORKFLOWS[self.target])

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
                          'new notes must not replace existing evidence', 'operator')
        for path in self.assets.iterdir():
            self.assertEqual(path.read_bytes(), (out / path.name).read_bytes())
        (self.assets / self.candidate['archive']).write_bytes(b'changed remotely')
        with self.assertRaisesRegex(ValueError, 'replacement is forbidden'):
            p.prepare_release(out, self.assets, self.tag, self.fixture.tag_commit, self.candidates(), {}, 'scope notes', 'operator')

    def test_wrong_existing_manifest_identity_cannot_be_overwritten(self):
        release = self.fixture.release
        release['tag_commit'] = 'b' * 40
        (self.assets / 'RELEASE.json').write_text(json.dumps(release))
        with self.assertRaisesRegex(ValueError, 'identity mismatch'):
            p.prepare_release(self.prepared(), self.assets, self.tag, self.fixture.tag_commit, self.candidates(), {}, 'scope', 'operator')

    def test_explicit_acceptance_is_required_before_api_or_filesystem_mutation(self):
        with patch.object(p, 'gh') as api, self.assertRaises(ValueError):
            p.promote('owner/repo', self.tag, {}, 'not accepted', 'actor', False, self.root / 'output')
        api.assert_not_called()
        self.assertFalse((self.root / 'output').exists())

    def exercise(self, *, bad_signature=False, already_published=False, corrupt_after_publish=False):
        remote = self.root / 'remote'; remote.mkdir()
        if already_published:
            for path in self.assets.iterdir():
                shutil.copyfile(path, remote / path.name)
        state = {'draft': not already_published, 'mutations': []}
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
                    path = Path(name); shutil.copyfile(path, remote / path.name)
            elif args[2] == 'edit':
                state['draft'] = False
                if corrupt_after_publish:
                    (remote / self.candidate['archive']).write_bytes(b'corrupted after publication')
            else:
                self.fail(args)
        with patch.object(p, 'fetch_candidate', side_effect=candidate), patch.object(p, 'release_record', side_effect=record), \
                patch.object(p, 'gh', return_value={'sha': self.fixture.tag_commit}), patch.object(p, 'download', side_effect=download), \
                patch.object(p.subprocess, 'run', side_effect=command), patch.object(p.verifier.builder, 'binary_format'), \
                patch.object(p.verifier, 'signature', side_effect=ValueError('bad signature') if bad_signature else None):
            try:
                report = p.promote('owner/repo', self.tag, dict(zip(p.verifier.TARGETS, [1, 2])),
                                   'Both exact candidate ZIPs passed GUI acceptance.', 'operator', True, self.root / 'output')
            except ValueError:
                if bad_signature:
                    self.assertEqual(state['mutations'], [])
                elif corrupt_after_publish:
                    self.assertEqual(state['mutations'], ['upload', 'edit'])
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

    def test_postpublication_change_is_reported(self):
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            self.exercise(corrupt_after_publish=True)


if __name__ == '__main__':
    unittest.main()
