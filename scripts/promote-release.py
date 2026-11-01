#!/usr/bin/env python3
"""Promote explicitly accepted Actions candidates. Never build or replace assets."""
import argparse
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import zipfile

spec = importlib.util.spec_from_file_location('release_verifier', Path(__file__).with_name('verify-release.py'))
verifier = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verifier)
require = verifier.require
MAX_FILE = 256 * 1024 * 1024
WORKFLOWS = {'aarch64-apple-darwin': '.github/workflows/rewrite-macos-candidate.yml',
             'x86_64-pc-windows-msvc': '.github/workflows/rewrite-candidate.yml'}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def gh(*args, binary=False):
    result = subprocess.run(['gh', *args], check=True, capture_output=True, timeout=180)
    return result.stdout if binary else json.loads(result.stdout)


def asset_names(tag):
    names = {'RELEASE.json', 'SHA256SUMS', *verifier.RECEIPTS}
    for target in verifier.TARGETS:
        archive = f'desktop-world-{tag[1:]}-{target}.zip'
        names.update({archive, archive + '.sha256', f'CANDIDATE-{target}.json', f'BUILD-{target}.json'})
    return names


def validate_run(run, repository, workflow):
    require(run.get('conclusion') == 'success' and run.get('path') == workflow, 'Candidate workflow did not pass')
    require(run.get('repository', {}).get('full_name') == repository
            and run.get('head_repository', {}).get('full_name') == repository, 'Foreign candidate repository')
    require(re.fullmatch(r'[0-9a-f]{40}', run.get('head_sha', '')), 'Invalid candidate source')


def member(archive, name):
    matches = [item for item in archive.infolist() if item.filename == name]
    require(len(matches) == 1 and not matches[0].is_dir()
            and matches[0].file_size <= MAX_FILE, 'Missing, duplicate or oversized artifact member: ' + name)
    return archive.read(matches[0])


def stage_candidate(data, artifact_digest, source, tag, target, output):
    require(len(data) <= MAX_FILE and artifact_digest == 'sha256:' + digest(data), 'Actions artifact digest mismatch')
    with zipfile.ZipFile(io.BytesIO(data)) as outer:
        candidates = [item.filename for item in outer.infolist()
                      if PurePosixPath(item.filename).name == 'CANDIDATE.json']
        require(len(candidates) == 1, 'Ambiguous candidate manifest')
        path = candidates[0]
        require('\\' not in path and not path.startswith('/')
                and '..' not in PurePosixPath(path).parts, 'Unsafe candidate path')
        prefix = path[:-len('CANDIDATE.json')]
        manifest = member(outer, path)
        candidate = json.loads(manifest)
        require(candidate.get('version') == tag[1:] and candidate.get('target') == target
                and candidate.get('source', {}).get('commit') == source, 'Candidate version, target or source mismatch')
        name = f'desktop-world-{tag[1:]}-{target}.zip'
        require(candidate.get('archive') == name, 'Candidate archive name mismatch')
        package = member(outer, prefix + name)
        require(digest(package) == candidate.get('archive_sha256'), 'Candidate archive digest mismatch')
        with zipfile.ZipFile(io.BytesIO(package)) as inner:
            build = member(inner, name[:-4] + '/BUILD.json')
        require(digest(build) == candidate.get('build_manifest_sha256'), 'Candidate BUILD digest mismatch')
        files = {name: package, name + '.sha256': member(outer, prefix + name + '.sha256'),
                 f'CANDIDATE-{target}.json': manifest, f'BUILD-{target}.json': build}
        if target == verifier.TARGETS[0]:
            for receipt in verifier.RECEIPTS:
                files[receipt] = member(outer, prefix + receipt)
        for filename, content in files.items():
            (output / filename).write_bytes(content)
    return candidate


def fetch_candidate(repository, run_id, tag, target, output):
    run = gh('api', f'repos/{repository}/actions/runs/{run_id}')
    validate_run(run, repository, WORKFLOWS[target])
    artifacts = gh('api', f'repos/{repository}/actions/runs/{run_id}/artifacts?per_page=100')['artifacts']
    name = ('dtw-macos-candidate-' if target == verifier.TARGETS[0] else 'dtw-windows-candidate-') + run['head_sha']
    artifacts = [a for a in artifacts if a['name'] == name and not a['expired']]
    require(len(artifacts) == 1, 'Missing or ambiguous fixed candidate artifact')
    artifact = artifacts[0]
    data = gh('api', f'repos/{repository}/actions/artifacts/{artifact["id"]}/zip', binary=True)
    candidate = stage_candidate(data, artifact.get('digest'), run['head_sha'], tag, target, output)
    return candidate, {'run_id': run_id, 'artifact_id': artifact['id'], 'artifact_digest': artifact['digest']}


def release_record(repository, tag):
    # GitHub's by-tag endpoint does not reliably expose drafts. Writers can list
    # them; do not mistake a hidden or missing draft for permission to create one.
    pages = gh('api', '--paginate', '--slurp', f'repos/{repository}/releases?per_page=100')
    matches = [r for page in pages for r in page if r['tag_name'] == tag]
    require(len(matches) == 1, 'Expected one existing Release Please draft or published release')
    return matches[0]


def download(repository, tag, output):
    output.mkdir()
    subprocess.run(['gh', 'release', 'download', tag, '--repo', repository, '--dir', str(output)],
                   check=True, timeout=180)


def prepare_release(output, existing, tag, commit, candidates, origins, notes, actor):
    platforms = [{k: c[k] for k in ('target', 'source', 'archive', 'archive_sha256', 'binary_sha256', 'notarization')}
                 for c in candidates]
    release = {'schema': 1, 'version': tag[1:], 'tag': tag, 'tag_commit': commit, 'platforms': platforms,
               'acceptance': {'status': 'maintainer_attested', 'actor': actor, 'notes': notes,
                              'bound_to': {c['target']: c['archive_sha256'] for c in candidates},
                              'qualification': 'Explicit operator attestation; CI does not infer GUI acceptance.'},
               'candidate_workflows': origins,
               'promotion': {'rebuild': False, 'resign': False, 'recompress': False}}
    original = existing / 'RELEASE.json'
    if original.exists():
        # Preserve the original acceptance evidence, including a partially
        # uploaded draft. Never rewrite a manifest just to make a rerun pass.
        saved = json.loads(original.read_bytes())
        require(saved.get('version') == tag[1:] and saved.get('tag') == tag
                and saved.get('tag_commit') == commit, 'Existing release identity mismatch')
        saved_platforms = saved.get('platforms', [])
        require(len(saved_platforms) == 2, 'Existing platform matrix mismatch')
        for expected in platforms:
            rows = [p for p in saved_platforms if p.get('target') == expected['target']]
            require(len(rows) == 1 and all(rows[0].get(k) == v for k, v in expected.items()),
                    'Existing release does not match accepted candidate')
        content = original.read_bytes()
    else:
        content = (json.dumps(release, indent=2) + '\n').encode()
    (output / 'RELEASE.json').write_bytes(content)
    checksums = {p.name: digest(p.read_bytes()) for p in sorted(output.iterdir()) if p.name != 'SHA256SUMS'}
    original_sums = existing / 'SHA256SUMS'
    if original_sums.exists():
        sums = original_sums.read_bytes()
        require(verifier.checksums(sums) == checksums, 'Existing checksum inventory mismatch')
    else:
        sums = ''.join(sha + '  ' + name + '\n' for name, sha in checksums.items()).encode()
    (output / 'SHA256SUMS').write_bytes(sums)
    require({p.name for p in output.iterdir()} == asset_names(tag), 'Unexpected prepared asset set')
    for path in existing.iterdir():
        require(path.is_file() and path.name in asset_names(tag), 'Unexpected existing asset')
        require(path.read_bytes() == (output / path.name).read_bytes(), 'Existing asset differs; replacement is forbidden: ' + path.name)


def promote(repository, tag, runs, notes, actor, accepted, output):
    require(accepted is True and len(notes.strip()) >= 20, 'Explicit GUI acceptance and substantive scope notes are required')
    require(re.fullmatch(r'[\w.-]+/[\w.-]+', repository) and re.fullmatch(r'v\d+\.\d+\.\d+', tag), 'Invalid repository or tag')
    require(set(runs) == set(verifier.TARGETS) and all(type(n) is int and n > 0 for n in runs.values()), 'Invalid candidate run IDs')
    output.mkdir(parents=True)
    prepared = output / 'prepared'; prepared.mkdir()
    candidates = []; origins = {}
    for target in verifier.TARGETS:
        candidate, origin = fetch_candidate(repository, runs[target], tag, target, prepared)
        candidates.append(candidate); origins[target] = origin
    release = release_record(repository, tag)
    names = [a['name'] for a in release['assets']]
    require(len(names) == len(set(names)) and set(names) <= asset_names(tag), 'Unexpected existing release asset names')
    commit = gh('api', f'repos/{repository}/commits/{tag}')['sha']
    require(re.fullmatch(r'[0-9a-f]{40}', commit), 'Missing immutable release tag')
    existing = output / 'existing'
    if release['assets']:
        download(repository, tag, existing)
    else:
        existing.mkdir()
    prepare_release(prepared, existing, tag, commit, candidates, origins, notes, actor)
    # Native macOS signature and real archive verification before any upload.
    verifier.verify(prepared, tag, commit)
    missing = sorted(p for p in prepared.iterdir() if not (existing / p.name).exists())
    require(release['draft'] or not missing, 'Published release is incomplete; no repair or replacement is allowed')
    if missing:
        subprocess.run(['gh', 'release', 'upload', tag, '--repo', repository, *map(str, missing)],
                       check=True, timeout=180)  # Deliberately no --clobber.
    remote = output / 'preflight'
    download(repository, tag, remote)
    preflight = verifier.verify(remote, tag, commit)
    for path in prepared.iterdir():
        require(path.read_bytes() == (remote / path.name).read_bytes(), 'Uploaded asset changed: ' + path.name)
    if release['draft']:
        subprocess.run(['gh', 'release', 'edit', tag, '--repo', repository, '--draft=false', '--latest'],
                       check=True, timeout=180)
    published = release_record(repository, tag)
    require(not published['draft'], 'Release is still a draft')
    final = output / 'published'
    download(repository, tag, final)
    audit = verifier.verify(final, tag, commit)
    for path in remote.iterdir():
        require(path.read_bytes() == (final / path.name).read_bytes(), 'Asset changed across publication')
    report = {'tag': tag, 'tag_commit': commit, 'status': 'published' if release['draft'] else 'already_published',
              'release_id': published['id'], 'preflight': preflight, 'published_audit': audit,
              'archives': {c['target']: c['archive_sha256'] for c in candidates}, 'origins': origins,
              'acceptance_attestation': {'actor': actor, 'notes': notes}}
    (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    return report


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output', type=Path, required=True)
    args = p.parse_args()
    report = promote(os.environ['GH_REPO'], os.environ['RELEASE_TAG'],
                     {verifier.TARGETS[0]: int(os.environ['MACOS_RUN_ID']),
                      verifier.TARGETS[1]: int(os.environ['WINDOWS_RUN_ID'])},
                     os.environ['ACCEPTANCE_NOTES'], os.environ['GITHUB_ACTOR'],
                     os.environ.get('GUI_ACCEPTED') == 'true', args.output)
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
