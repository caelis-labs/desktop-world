#!/usr/bin/env python3
"""Bind automatic release construction to successful main CI and immutable artifacts."""
import argparse
import base64
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import zipfile

BUILD_WORKFLOW = '.github/workflows/release-please.yml'
CI_WORKFLOW = '.github/workflows/ci.yml'
TARGETS = ('aarch64-apple-darwin', 'x86_64-pc-windows-msvc')


def require(condition, message):
    if not condition:
        raise ValueError(message)


def gh(*args, binary=False):
    data = subprocess.check_output(['gh', *args], timeout=180)
    return data if binary else json.loads(data)


def trusted_run(run, repository, workflow):
    require(run.get('path') == workflow and run.get('event') in ('push', 'workflow_run', 'workflow_dispatch'), 'Unexpected workflow origin')
    require(run.get('repository', {}).get('full_name') == repository
            and run.get('head_repository', {}).get('full_name') == repository, 'Foreign workflow repository')
    require(re.fullmatch(r'[0-9a-f]{40}', run.get('head_sha', '')), 'Invalid workflow source')
    require(run.get('head_branch') == 'main', 'Release workflows must originate on main')
    allowed = ('push',) if workflow == CI_WORKFLOW else ('workflow_run', 'workflow_dispatch')
    require(run.get('event') in allowed, 'Unexpected release workflow event')
    require(run.get('status') == 'completed' and run.get('conclusion') == 'success', 'Workflow has not succeeded')


def artifact_json(repository, artifact, filename, limit=16384):
    require(not artifact.get('expired'), 'Workflow evidence expired')
    data = gh('api', f'repos/{repository}/actions/artifacts/{artifact["id"]}/zip', binary=True)
    require(len(data) <= 1024 * 1024
            and artifact.get('digest') == 'sha256:' + hashlib.sha256(data).hexdigest(), 'Evidence artifact digest mismatch')
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        require(archive.namelist() == [filename] and archive.getinfo(filename).file_size <= limit, 'Unexpected evidence members')
        record = json.loads(archive.read(filename))
        require(isinstance(record, dict), 'Evidence must be a JSON object')
        return record


def successful_ci(repository, source, tree):
    runs = gh('api', f'repos/{repository}/actions/workflows/ci.yml/runs?event=push&branch=main&head_sha={source}&status=success&per_page=100')
    for run in runs['workflow_runs']:
        if run.get('head_sha') != source or run.get('head_branch') != 'main':
            continue
        trusted_run(run, repository, CI_WORKFLOW)
        require(run['event'] == 'push', 'Release CI must be a main push')
        identifier, attempt = run['id'], run['run_attempt']
        artifacts = gh('api', f'repos/{repository}/actions/runs/{identifier}/artifacts?per_page=100')['artifacts']
        matches = [a for a in artifacts if a['name'] == f'ci-verification-{identifier}-{attempt}' and not a['expired']]
        if not matches:
            continue
        require(len(matches) == 1, 'Ambiguous CI receipt')
        record = artifact_json(repository, matches[0], 'verified.json')
        require(record == {'schema': 1, 'tree': tree, 'run_id': identifier, 'attempt': attempt,
                           'event': 'push', 'checks': 'passed'}, 'CI receipt does not prove the release source tree')
        return {'run_id': identifier, 'attempt': attempt, 'source': source, 'tree': tree,
                'artifact_id': matches[0]['id'], 'artifact_digest': matches[0]['digest']}
    raise ValueError('No successful main CI receipt for the exact release source')


def gate(repository, event, payload, source):
    require(re.fullmatch(r'[\w.-]+/[\w.-]+', repository), 'Invalid repository')
    if event == 'workflow_run':
        trigger = payload['workflow_run']
        trusted_run(trigger, repository, CI_WORKFLOW)
        require(trigger['event'] == 'push' and trigger['head_branch'] == 'main', 'Only successful main-push CI may trigger releases')
        source = trigger['head_sha']
    else:
        require(event == 'workflow_dispatch', 'Unsupported release trigger')
    main = gh('api', f'repos/{repository}/commits/main')
    if main['sha'] != source:
        return {'eligible': False, 'reason': 'The main branch advanced; its own successful CI will maintain the release.'}
    ci = successful_ci(repository, source, main['commit']['tree']['sha'])
    if gh('api', f'repos/{repository}/commits/main')['sha'] != source:
        return {'eligible': False, 'reason': 'Main advanced while verifying CI; wait for that commit\'s own successful run.'}
    return {'eligible': True, 'source': source, 'ci': ci}


def releases(repository):
    return [release for page in gh('api', '--paginate', '--slurp', f'repos/{repository}/releases?per_page=100') for release in page]


def pending_plan(repository, gate_record, run_id):
    require(gate_record.get('eligible') is True, 'Release maintenance was not CI admitted')
    content = gh('api', f'repos/{repository}/contents/version.txt?ref={gate_record["source"]}')
    version = base64.b64decode(content['content']).decode().strip()
    require(re.fullmatch(r'\d+\.\d+\.\d+', version), 'Invalid stable version')
    tag = 'v' + version
    matches = [r for r in releases(repository) if r['tag_name'] == tag]
    require(len(matches) <= 1, 'Ambiguous release tag')
    if not matches or not matches[0]['draft']:
        return {'release': False, 'reason': 'No pending version release.'}
    require(not matches[0].get('prerelease'), 'Stable publication cannot consume a prerelease')
    commit = gh('api', f'repos/{repository}/commits/{tag}')
    source, tree = commit['sha'], commit['commit']['tree']['sha']
    if source != gate_record['source']:
        tagged_version = gh('api', f'repos/{repository}/contents/version.txt?ref={source}')
        require(base64.b64decode(tagged_version['content']).decode().strip() == version, 'Tag source version mismatch')
    # Release Please reads main. If main moves during that API operation, never
    # transfer the triggering commit's green status to the actual tag source.
    ci = successful_ci(repository, source, tree)
    name = 'release-plan-' + source
    artifacts = gh('api', f'repos/{repository}/actions/artifacts?name={name}&per_page=100')['artifacts']
    for artifact in artifacts:
        previous = artifact.get('workflow_run', {}).get('id')
        if artifact['name'] != name or artifact['expired'] or not previous or previous == run_id:
            continue
        run = gh('api', f'repos/{repository}/actions/runs/{previous}')
        if run.get('conclusion') == 'success':
            trusted_run(run, repository, BUILD_WORKFLOW)
            prior = artifact_json(repository, artifact, 'release-plan.json')
            require(prior['source'] == source and prior['tag'] == tag, 'Previous construction identity mismatch')
            return {'release': False, 'reason': f'Immutable construction already exists in run {previous}; retry its publication workflow without rebuilding.'}
    return {'schema': 1, 'release': True, 'tag': tag, 'source': source, 'tree': tree, 'ci': ci,
            'maintenance_ci': gate_record['ci']}


def collect(repository, plan, run_id, attempt, artifacts):
    require(plan.get('release') is True and set(artifacts) == set(TARGETS), 'Missing platform construction')
    result = dict(plan, build_run_id=run_id, build_attempt=attempt, artifacts={})
    for target, identifier in artifacts.items():
        require(type(identifier) is int and identifier > 0, 'Invalid platform artifact ID')
        artifact = gh('api', f'repos/{repository}/actions/artifacts/{identifier}')
        prefix = 'dtw-macos-candidate-' if target == TARGETS[0] else 'dtw-windows-candidate-'
        require(re.fullmatch(re.escape(prefix + plan['source']) + r'-attempt-[1-9]\d*', artifact['name']), 'Wrong platform artifact source')
        require(not artifact['expired'] and artifact['workflow_run']['id'] == run_id
                and re.fullmatch(r'sha256:[0-9a-f]{64}', artifact.get('digest', '')), 'Invalid immutable platform artifact')
        result['artifacts'][target] = {k: artifact[k] for k in ('id', 'name', 'digest')}
    return result


def load(repository, run_id, attempt):
    run = gh('api', f'repos/{repository}/actions/runs/{run_id}/attempts/{attempt}')
    trusted_run(run, repository, BUILD_WORKFLOW)
    require(run.get('id') == run_id and run.get('run_attempt') == attempt, 'Workflow run attempt identity mismatch')
    artifacts = gh('api', f'repos/{repository}/actions/runs/{run_id}/artifacts?per_page=100')['artifacts']
    plans = [a for a in artifacts if a['name'].startswith('release-plan-')]
    if not plans:
        return None  # An ordinary Release Please PR-maintenance run.
    require(len(plans) == 1, 'Ambiguous construction plan')
    require(not plans[0]['expired'], 'Construction plan expired; publication cannot infer missing evidence')
    plan = artifact_json(repository, plans[0], 'release-plan.json')
    require(plan.get('schema') == 1 and plan.get('release') is True
            and plan.get('build_run_id') == run_id and plan.get('build_attempt') == attempt
            and plans[0]['name'] == 'release-plan-' + plan['source'], 'Construction plan identity mismatch')
    require(re.fullmatch(r'v\d+\.\d+\.\d+', plan['tag']) and re.fullmatch(r'[0-9a-f]{40}', plan['source']), 'Invalid release identity')
    require(set(plan['artifacts']) == set(TARGETS), 'Incomplete release platform matrix')
    require(plan.get('ci', {}).get('source') == plan['source']
            and plan.get('ci', {}).get('tree') == plan.get('tree'), 'CI evidence belongs to another source tree')
    return plan


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['gate', 'plan', 'collect', 'inspect'])
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    repository = os.environ['GITHUB_REPOSITORY']
    run_id = int(os.environ['GITHUB_RUN_ID'])
    if args.command == 'gate':
        record = gate(repository, os.environ['GITHUB_EVENT_NAME'], json.loads(Path(os.environ['GITHUB_EVENT_PATH']).read_text()), os.environ['GITHUB_SHA'])
    elif args.command == 'plan':
        record = pending_plan(repository, json.loads(Path(os.environ['RELEASE_GATE']).read_text()), run_id)
    elif args.command == 'collect':
        record = collect(repository, json.loads(Path(os.environ['RELEASE_PLAN']).read_text()), run_id, int(os.environ['GITHUB_RUN_ATTEMPT']),
                         dict(zip(TARGETS, map(int, [os.environ['MACOS_ARTIFACT_ID'], os.environ['WINDOWS_ARTIFACT_ID']]))))
    else:
        record = load(repository, int(os.environ['RELEASE_BUILD_RUN_ID']), int(os.environ['RELEASE_BUILD_ATTEMPT']))
        if record is None:
            record = {'release': False, 'reason': 'Version PR maintenance has no completed release construction.'}
    args.output.write_text(json.dumps(record, indent=2) + '\n', encoding='utf-8')
    if os.environ.get('GITHUB_OUTPUT'):
        with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as output:
            for key in ('eligible', 'release', 'source', 'tag'):
                if key in record:
                    value = str(record[key]).lower() if isinstance(record[key], bool) else record[key]
                    output.write(f'{key}={value}\n')
    print(json.dumps(record))


if __name__ == '__main__':
    main()
