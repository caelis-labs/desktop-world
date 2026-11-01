#!/usr/bin/env python3
"""Automatically publish a successful fixed construction. Never build or replace assets."""
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
plan_spec = importlib.util.spec_from_file_location('release_plan', Path(__file__).with_name('release-plan.py'))
planner = importlib.util.module_from_spec(plan_spec)
plan_spec.loader.exec_module(planner)
MAX_FILE = 256 * 1024 * 1024


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


def fetch_candidate(repository, plan, tag, target, output):
    expected = plan['artifacts'][target]
    artifact = gh('api', f'repos/{repository}/actions/artifacts/{expected["id"]}')
    require(all(artifact.get(key) == expected[key] for key in ('id', 'name', 'digest'))
            and not artifact['expired'] and artifact['workflow_run']['id'] == plan['build_run_id'],
            'Candidate artifact no longer matches its successful construction')
    prefix = 'dtw-macos-candidate-' if target == verifier.TARGETS[0] else 'dtw-windows-candidate-'
    require(re.fullmatch(re.escape(prefix + plan['source']) + r'-attempt-[1-9]\d*', artifact['name']), 'Wrong candidate artifact source')
    data = gh('api', f'repos/{repository}/actions/artifacts/{artifact["id"]}/zip', binary=True)
    candidate = stage_candidate(data, artifact.get('digest'), plan['source'], tag, target, output)
    require(candidate['source'].get('tree') == plan['tree'], 'Candidate source tree differs from successful CI')
    return candidate, {'run_id': plan['build_run_id'], 'artifact_id': artifact['id'], 'artifact_digest': artifact['digest']}


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


def prepare_release(output, existing, tag, commit, candidates, origins, ci, actor):
    platforms = [{k: c[k] for k in ('target', 'source', 'archive', 'archive_sha256', 'binary_sha256', 'notarization')}
                 for c in candidates]
    release = {'schema': 1, 'version': tag[1:], 'tag': tag, 'tag_commit': commit, 'platforms': platforms,
               'acceptance': {'status': 'automated_ci', 'actor': actor, 'ci': ci,
                              'bound_to': {c['target']: c['archive_sha256'] for c in candidates},
                              'qualification': 'Fixed-source headless contracts, package metadata/EOF, integrity, signing and notarization; GUI checks are optional.'},
               'candidate_workflows': origins,
               'promotion': {'rebuild': False, 'resign': False, 'recompress': False}}
    original = existing / 'RELEASE.json'
    if original.exists():
        # Preserve the original construction evidence, including a partially
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


def latest_allowed(repository, tag):
    pages = gh('api', '--paginate', '--slurp', f'repos/{repository}/releases?per_page=100')
    version = tuple(map(int, tag[1:].split('.')))
    return not any(tuple(map(int, r['tag_name'][1:].split('.'))) > version
                   for page in pages for r in page if not r['draft'] and not r.get('prerelease')
                   and re.fullmatch(r'v\d+\.\d+\.\d+', r['tag_name']))


def promote(repository, plan, actor, output):
    tag = plan.get('tag', '')
    require(re.fullmatch(r'[\w.-]+/[\w.-]+', repository) and re.fullmatch(r'v\d+\.\d+\.\d+', tag), 'Invalid repository or tag')
    require(plan.get('schema') == 1 and plan.get('release') is True and set(plan.get('artifacts', {})) == set(verifier.TARGETS)
            and re.fullmatch(r'[0-9a-f]{40}', plan.get('source', ''))
            and plan.get('ci', {}).get('source') == plan['source'] and plan['ci'].get('tree') == plan.get('tree'),
            'Incomplete fixed construction plan')
    output.mkdir(parents=True)
    prepared = output / 'prepared'; prepared.mkdir()
    candidates = []; origins = {}
    for target in verifier.TARGETS:
        candidate, origin = fetch_candidate(repository, plan, tag, target, prepared)
        candidates.append(candidate); origins[target] = origin
    release = release_record(repository, tag)
    names = [a['name'] for a in release['assets']]
    require(len(names) == len(set(names)) and set(names) <= asset_names(tag), 'Unexpected existing release asset names')
    commit = gh('api', f'repos/{repository}/commits/{tag}')['sha']
    require(re.fullmatch(r'[0-9a-f]{40}', commit), 'Missing immutable release tag')
    require(commit == plan['source'], 'Release tag moved after its successful construction')
    existing = output / 'existing'
    if release['assets']:
        download(repository, tag, existing)
    else:
        existing.mkdir()
    prepare_release(prepared, existing, tag, commit, candidates, origins, plan['ci'], actor)
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
    require(gh('api', f'repos/{repository}/commits/{tag}')['sha'] == commit, 'Release tag moved before publication')
    if release['draft']:
        latest = '--latest' if latest_allowed(repository, tag) else '--latest=false'
        subprocess.run(['gh', 'release', 'edit', tag, '--repo', repository, '--draft=false', latest],
                       check=True, timeout=180)
    published = release_record(repository, tag)
    require(not published['draft'], 'Release is still a draft')
    require(gh('api', f'repos/{repository}/commits/{tag}')['sha'] == commit, 'Release tag moved during publication')
    final = output / 'published'
    download(repository, tag, final)
    audit = verifier.verify(final, tag, commit)
    for path in remote.iterdir():
        require(path.read_bytes() == (final / path.name).read_bytes(), 'Asset changed across publication')
    report = {'tag': tag, 'tag_commit': commit, 'status': 'published' if release['draft'] else 'already_published',
              'release_id': published['id'], 'preflight': preflight, 'published_audit': audit,
              'archives': {c['target']: c['archive_sha256'] for c in candidates}, 'origins': origins,
              'automated_ci': plan['ci'], 'build_run_id': plan['build_run_id']}
    (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    return report


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output', type=Path, required=True)
    args = p.parse_args()
    repository = os.environ['GH_REPO']
    plan = planner.load(repository, int(os.environ['RELEASE_BUILD_RUN_ID']), int(os.environ['RELEASE_BUILD_ATTEMPT']))
    if plan is None:
        args.output.mkdir(parents=True)
        report = {'status': 'no_release', 'reason': 'Ordinary version-PR maintenance had no completed construction.'}
        (args.output / 'report.json').write_text(json.dumps(report), encoding='utf-8')
    else:
        report = promote(repository, plan, os.environ['GITHUB_ACTOR'], args.output)
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
