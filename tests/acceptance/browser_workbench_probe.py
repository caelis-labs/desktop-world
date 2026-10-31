#!/usr/bin/env python3
"""Real Chrome business UI + TextEdit, through standard DTW only.

Development-artifact usability evidence, or opt-in installed candidate evidence.
An unpublished candidate is never reported as a formal release.
Each invocation owns a fresh local dataset/document. Agent actions are never
replayed. Raw transcripts remain private; only derived metrics belong in reports.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import shlex
import subprocess
import time
import traceback
import uuid

from standard_agent_probe import Mcp, require, successful, wait_until, write_json, stop_owned

REPORT = 'Inventory review\nDTW-0088 | target 37 | reviewed\nDTW-0240 | target 52 | reviewed\n备注：已核对 🐦\n下一步：安排补货\n'
NOTES = {'DTW-0088': '第一批：已核对 🐦\n待补货', 'DTW-0240': '第二批：已核对 🐦\n待补货'}
QUANTITIES = {'DTW-0088': 37, 'DTW-0240': 52}


def file_sha256(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def verify_installed_package(package, dtw):
    """Verify an extracted candidate without executing or changing any file."""
    package = package.resolve(strict=True)
    require(package.is_dir(), '--installed-package must be an extracted package directory')
    binary = package/'bin/dtw'
    require(dtw.resolve(strict=True) == binary, '--dtw must be this installed package\'s bin/dtw')
    files = set()
    for path in package.rglob('*'):
        require(not path.is_symlink() and (path.is_file() or path.is_dir()),
                'Installed package contains a symlink or special file')
        if path.is_file(): files.add(path.relative_to(package).as_posix())
    require('SHA256SUMS' in files, 'Installed package lacks SHA256SUMS')
    expected = {}
    for line in (package/'SHA256SUMS').read_text(encoding='utf-8').splitlines():
        require(re.fullmatch(r'[0-9a-f]{64}  .+', line), 'Invalid installed SHA256SUMS entry')
        checksum, name = line.split('  ', 1)
        relative = PurePosixPath(name)
        require(not relative.is_absolute() and '..' not in relative.parts
                and relative.as_posix() == name and '\\' not in name and ':' not in name,
                'Unsafe installed SHA256SUMS path')
        require(name not in expected, 'Duplicate installed checksum entry')
        expected[name] = checksum
    require(set(expected) == files - {'SHA256SUMS'}, 'Installed package inventory differs from SHA256SUMS')
    for name, checksum in expected.items():
        require(file_sha256(package/name) == checksum, 'Installed package hash mismatch: '+name)
    require({'BUILD.json','bin/dtw','skills/desktop-world/SKILL.md','.claude-plugin/plugin.json'} <= files,
            'Installed package lacks required candidate assets')
    build = json_object((package/'BUILD.json').read_text(encoding='utf-8'))
    require(build.get('schema') == 1 and build.get('kind') == 'unpublished-rewrite-candidate',
            'Expected an unpublished rewrite candidate BUILD manifest')
    require(build.get('target') == 'aarch64-apple-darwin', 'This probe requires the macOS arm64 candidate')
    require(isinstance(build.get('binary'), dict) and build['binary'].get('path') == 'bin/dtw'
            and build['binary'].get('sha256') == expected['bin/dtw'], 'BUILD binary identity mismatch')
    require(build.get('skill_sha256') == expected['skills/desktop-world/SKILL.md'], 'BUILD Skill identity mismatch')
    source = build.get('source')
    require(isinstance(source, dict)
            and all(isinstance(source.get(key), str) and re.fullmatch(r'(?:[0-9a-f]{40}|[0-9a-f]{64})', source[key])
                    for key in ('commit','tree'))
            and isinstance(source.get('cargo_lock_sha256'), str)
            and re.fullmatch(r'[0-9a-f]{64}', source['cargo_lock_sha256'])
            and isinstance(build.get('source_archive_sha256'), str)
            and re.fullmatch(r'[0-9a-f]{64}', build['source_archive_sha256']), 'BUILD source identity missing')
    require(binary.is_file() and os.access(binary, os.X_OK), 'Installed bin/dtw is not executable')
    return {'artifact_kind':build['kind'], 'release_artifact':False, 'installed_artifact':True,
            'binary_path':str(binary), 'binary_sha256':expected['bin/dtw'],
            'skill_sha256':build['skill_sha256'],
            'installed_package':{'path':str(package), 'build_manifest_sha256':expected['BUILD.json'],
                'manifest_evidence':'installed-BUILD.json', 'checksums_evidence':'installed-SHA256SUMS',
                'sha256sums_sha256':file_sha256(package/'SHA256SUMS'), 'file_count':len(files),
                'source':source, 'source_archive_sha256':build['source_archive_sha256'],
                'version':build.get('version'), 'target':build['target']}}


def prepare_runtime(repo, root, dtw, installed_package=None):
    if installed_package is not None:
        identity = verify_installed_package(installed_package, dtw)
        package = Path(identity['installed_package']['path'])
        require(not root.resolve().is_relative_to(package), 'Output must be outside the installed package')
        # Keep the original manifests as evidence even if final revalidation fails.
        for name in ('BUILD.json','SHA256SUMS'):
            shutil.copy2(package/name, root/('installed-'+name))
        return Path(identity['binary_path']), identity
    binary = root/'dtw'
    shutil.copy2(dtw.resolve(strict=True), binary)
    return binary, {'artifact_kind':'local-development-artifact', 'release_artifact':False,
                    'installed_artifact':False, 'binary_path':str(binary),
                    'binary_sha256':file_sha256(binary),
                    'skill_sha256':file_sha256(repo/'skills/desktop-world/SKILL.md')}


def stage_agent_plugin(repo, root, binary, name, installed_package=None):
    stage = root/'plugin'
    if installed_package is None:
        with (root/'stage.json').open('w') as log:
            subprocess.run(['python3',str(repo/'scripts/stage-agent-plugin.py'),'--binary',str(binary),
                            '--output',str(stage)],check=True,stdout=log)
    else:
        # The isolated plugin supplies only package-owned metadata/Skill. Both
        # Agents register MCP explicitly against the original installed binary.
        stage.mkdir()
        shutil.copytree(installed_package/'.claude-plugin', stage/'.claude-plugin')
        shutil.copytree(installed_package/'skills/desktop-world', stage/'skills/desktop-world')
    manifest = stage/'.claude-plugin/plugin.json'
    data = json_object(manifest.read_text()); data['name'] = name; write_json(manifest, data)
    skill = stage/'skills/desktop-world'
    skill.rename(stage/'skills'/name)
    skill = stage/'skills'/name/'SKILL.md'
    text = skill.read_text()
    require(text.count('\nname: desktop-world\n') == 1, 'Canonical Skill name is missing or ambiguous')
    skill.write_text(text.replace('\nname: desktop-world\n', '\nname: '+name+'\n', 1))
    return stage, skill


def recheck_installed_package(identity):
    current = verify_installed_package(Path(identity['installed_package']['path']), Path(identity['binary_path']))
    require(current == identity, 'Installed package identity changed during acceptance')
    return {'unchanged':True, **current}


class SupervisorMcp(Mcp):
    """Remember lost identity continuity even after later calls report no reset."""
    def __init__(self, *args, **kwargs):
        self.context_epochs = {}
        super().__init__(*args, **kwargs)

    def execute(self, app, case, source, **options):
        try:
            record, response = super().execute(app, case, source, **options)
        except BaseException:
            self.context_epochs[app] = self.context_epochs.get(app, 0) + 1
            raise
        if record.get('context_reset') is not False or record.get('state_reset') is not False:
            self.context_epochs[app] = self.context_epochs.get(app, 0) + 1
        return record, response


def initialize(binary, path):
    path.mkdir()
    client = SupervisorMcp(binary, path)
    client.rpc('initialize', {'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'business-task-supervisor','version':'1'}})
    client.notify('notifications/initialized')
    return client


def json_object(value):
    if isinstance(value, str):
        def invalid_constant(value):
            raise ValueError('Non-JSON constant: '+value)
        value = json.loads(value, parse_constant=invalid_constant)
    require(isinstance(value, dict), 'Expected a JSON object')
    return value


def json_bytes(value):
    # Count one logical DTW payload, not duplicate MCP text/structured copies or
    # base64 image content. This definition also covers object-valued rawOutput.
    return len(json.dumps(value, ensure_ascii=False, separators=(',', ':'), allow_nan=False).encode('utf-8'))


def image_size(blocks):
    total = 0
    for block in blocks:
        if block.get('type') == 'content': block = block.get('content', {})
        if block.get('type') == 'image':
            data = block.get('data')
            if 'source' in block:
                source = block['source']
                require(data is None and isinstance(source, dict) and source.get('type') == 'base64'
                        and source.get('media_type') == 'image/png', 'Unknown Anthropic image source')
                data = source.get('data')
            require(isinstance(data, str), 'Image bytes unavailable')
            decoded = base64.b64decode(data, validate=True)
            require(decoded, 'Empty image payload')
            total += len(decoded)
    return total


def claude_image_caption(text, previous, scope):
    """Recognize only the observed adapter caption; never open its path."""
    if not isinstance(text, str) or not text.startswith('[Image: source: '):
        return False
    require(isinstance(scope, dict), 'Image caption has no verified ACP session')
    require(previous.get('type') == 'image' and 'source' in previous
            and image_size([previous]) > 0, 'Image caption has no preceding inline image')
    project = re.sub(r'[^A-Za-z0-9]', '-', scope['cwd'])
    directory = Path.home()/'.claude/projects'/project/scope['session_id']/'tool-results'
    pattern = (r'\[Image: source: '+re.escape(str(directory))+'/mcp-'
               +re.escape(scope['namespace'])+r'-blob-[0-9]{13}-[a-z0-9]{6}\.png\]')
    require(re.fullmatch(pattern, text) is not None, 'Image caption is outside this ACP session or namespace')
    return True


def decode_output(output, image_scope=None):
    """Return DTW payload, envelope error flag and exposed image byte count."""
    if isinstance(output, list): output = {'content': output}
    value = json_object(output)
    if value.get('jsonrpc') == '2.0':
        require(not value.get('error'), 'JSON-RPC tool error')
        value = json_object(value.get('result'))
    envelope = any(key in value for key in ('content', 'structuredContent', 'isError'))
    if not envelope:
        return value, False, None
    flag = value.get('isError', False)
    require(type(flag) is bool, 'Invalid MCP isError flag')
    blocks = value.get('content', [])
    require(isinstance(blocks, list), 'Invalid MCP content list')
    texts = []
    previous = {}
    for block in blocks:
        require(isinstance(block, dict), 'Invalid MCP content block')
        if block.get('type') == 'content': block = block.get('content', {})
        require(isinstance(block, dict), 'Invalid ACP content block')
        if block.get('type') == 'text':
            if not claude_image_caption(block.get('text'), previous, image_scope):
                texts.append(json_object(block.get('text')))
        else: require(block.get('type') == 'image', 'Unknown MCP content block')
        previous = block
    require(len(texts) <= 1, 'Expected at most one DTW JSON text block')
    payload = value.get('structuredContent', texts[0] if texts else None)
    payload = json_object(payload)
    if texts:
        require(json.dumps(payload, sort_keys=True, allow_nan=False) == json.dumps(texts[0], sort_keys=True, allow_nan=False),
                'MCP structured and text payloads disagree')
    return payload, flag, image_size(blocks)


DSH_UNADMITTED_IMAGE = ('[image unavailable: image/png; this result was not admitted to durable model context; '
                        'raw image data remains available to programmatic callers]')
DSH_MODEL_IMAGE = re.compile(
    r'\[image unavailable: image/png; model "(?:deepseek-v4-flash|deepseek-flash)" '
    r'does not declare image input; raw image data remains available to programmatic callers\]')


def dsh_output(output, status):
    """Unwrap only observed ACP text formats, retaining errors and missing images."""
    if not isinstance(output, list): return None, None
    blocks = []
    for block in output:
        require(isinstance(block, dict), 'Invalid DSH content block')
        if block.get('type') == 'content': block = block.get('content', {})
        require(isinstance(block, dict), 'Invalid DSH wrapped content')
        blocks.append(block)
    # Normal inline images need no DSH text adaptation; the ordinary decoder
    # validates their bytes without treating them as Claude caption paths.
    if any(block.get('type') == 'image' for block in blocks): return None, None
    require(1 <= len(blocks) <= 2 and all(block.get('type') == 'text' for block in blocks),
            'Unknown DSH MCP content shape')
    texts = [block.get('text') for block in blocks]
    require(all(isinstance(text, str) for text in texts), 'Invalid DSH MCP text')
    prefixed_error = texts[0].startswith('Error: ')
    text = texts[0][len('Error: '):] if prefixed_error else texts[0]
    _, end = json.JSONDecoder().raw_decode(text)
    payload = json_object(text[:end])
    trailer = text[end:]
    unavailable = None
    if trailer.strip():
        require(prefixed_error and len(texts) == 1 and trailer == '\n'+DSH_UNADMITTED_IMAGE,
                'Unknown DSH MCP output trailer')
        unavailable = DSH_UNADMITTED_IMAGE
    if len(texts) == 2:
        require(not prefixed_error and DSH_MODEL_IMAGE.fullmatch(texts[1]) is not None,
                'Unknown DSH image availability text')
        unavailable = texts[1]
    return {'structuredContent': payload, 'isError': prefixed_error or status == 'failed'}, unavailable


def normalize_acp(path, client='claude'):
    calls = {}
    requests = {}; sessions = {}
    terminal = None
    for line in path.read_text().splitlines():
        e = json.loads(line)
        if e.get('method') == 'session/new' and 'id' in e:
            cwd = e.get('params', {}).get('cwd')
            if isinstance(cwd, str) and Path(cwd).is_absolute() and Path(cwd).resolve() == path.parent.resolve():
                requests[json.dumps(e['id'])] = cwd
        if isinstance(e.get('result'), dict) and 'sessionId' in e['result'] and 'id' in e:
            cwd = requests.get(json.dumps(e['id']))
            session = e['result']['sessionId']
            # ACP session IDs are opaque. Only Claude's known UUID form can
            # establish the adapter-specific inline-image caption namespace.
            if client == 'claude' and cwd and isinstance(session, str):
                try:
                    if str(uuid.UUID(session)) == session: sessions[session] = cwd
                except ValueError:
                    pass
        update = e.get('params', {}).get('update', {})
        if update.get('sessionUpdate') in ('tool_call', 'tool_call_update'):
            key = (e.get('params', {}).get('sessionId'), update['toolCallId'])
            calls.setdefault(key, {}).update(update)
        if 'stopReason' in e.get('result', {}): terminal = e['result']
        if 'id' in e and isinstance(e.get('error'), dict):
            terminal = {'error': e['error'], 'response_id': e['id']}
    rows = []
    for (session, _), c in calls.items():
        try: images = image_size(c['content']) if 'content' in c else None
        except (AssertionError, ValueError, TypeError): images = None
        name = c.get('name', c.get('title'))
        row = {'name':name, 'arguments':c.get('rawInput'),
               'output':c.get('rawOutput', c.get('content')), 'status':c.get('status'), 'image_bytes':images}
        tool = re.fullmatch(r'mcp__([A-Za-z0-9_.-]+)__(apps|api|exec|result)', name or '')
        if client == 'dsh' and tool:
            try:
                payload, unavailable = dsh_output(row['output'], row['status'])
                if payload is not None: row['payload_output'] = payload
                if unavailable is not None:
                    row['image_unavailable'] = unavailable
                    row['image_bytes'] = None
            except (AssertionError, ValueError, TypeError) as error:
                row['normalization_error'] = str(error)
        if session in sessions and tool:
            row['acp_image_scope'] = {'cwd':sessions[session], 'session_id':session, 'namespace':tool.group(1)}
        rows.append(row)
    return rows, terminal


def prepare_dsh_agent(root, skill, adapter, name, mcp, timeout):
    """Use an isolated DSH wrapper with only MCP, Skill and read-only read tools."""
    require(adapter is not None, 'DSH requires --acp-adapter pointing to an isolated wrapper')
    local = root/'.agents/skills'/name
    local.mkdir(parents=True)
    shutil.copytree(skill.parent, local, dirs_exist_ok=True)
    config = root/'mcp.json'
    write_json(config, {'mcpServers': [{'name': name, 'command': mcp[0], 'args': mcp[1:], 'env': []}]})
    return ['acpx', '--agent', str(adapter), '--cwd', str(root), '--mcp-config', str(config),
            '--format', 'json', '--timeout', str(timeout), '--approve-all', '--no-fs',
            '--no-terminal', 'exec']


def agy_output(output, directory):
    """Separate agy's observed image trailer, only for this exact owned step."""
    if not isinstance(output, str) or '\n[Resource offloaded to ' not in output:
        return output, None
    value, end = json.JSONDecoder().raw_decode(output)
    json_object(output[:end])
    pattern = re.compile(r'\[Resource offloaded to '+re.escape(directory.as_uri())+r'/(media_[0-9]+\.png)\]')
    files = []
    for line in output[end:].strip().splitlines():
        match = pattern.fullmatch(line)
        require(match is not None, 'Unrecognized agy output trailer')
        filename = match.group(1)
        require(filename not in files, 'Duplicate agy image trailer')
        files.append(filename)
    require(files, 'Empty agy image trailer')
    sizes = []
    for filename in files:
        image = directory/filename
        try:
            # Inspect only files named by this invocation's exact step format.
            sizes.append(image.stat().st_size if image.is_file() and not image.is_symlink() else None)
        except OSError:
            sizes.append(None)
    return output[:end], sum(sizes) if all(size is not None for size in sizes) else None


def normalize_agy(path):
    steps = {}
    terminal = None
    for line in path.read_text().splitlines():
        e = json.loads(line)
        if 'step_update' in e:
            s = e['step_update']
            key = (s.get('conversation_id'), s.get('step_index'))
            if s.get('tool_name') or key in steps:
                old = steps.setdefault(key, {})
                info = {**old.get('tool_info', {}), **s.get('tool_info', {})}
                output_observed = old.get('_output_observed', False) or info.get('output') is not None
                non_preflight_state = old.get('_non_preflight_state', False) or s.get('state') not in ('ACTIVE', 'ERROR')
                old.update(s); old['tool_info'] = info
                old['_output_observed'] = output_observed
                old['_non_preflight_state'] = non_preflight_state
        if e.get('event') in ('result', 'done', 'turn_complete'): terminal = e
    rows = []
    for s in steps.values():
        info = s.get('tool_info', {})
        output = info.get('output')
        row = {'name':s['tool_name'],'arguments':info.get('parameters'),
               'output':output,'status':s.get('state'),'image_bytes':None}
        if 'error' in info:
            row['client_error'] = info['error']
            # Only the observed error-only client shape proves that no file
            # data was exposed, including in earlier partial step updates.
            row['client_error_no_data'] = (info.get('name') == s['tool_name']
                and set(info) == {'name','parameters','error'} and not s['_output_observed'])
            row['client_preflight_only'] = not s['_non_preflight_state']
        if 'duration_seconds' in s:
            row['duration_seconds'] = s['duration_seconds']
        if s.get('tool_name') == 'call_mcp_tool':
            # Only this invocation's own step cache, never a path from tool text.
            conversation = str(uuid.UUID(s['conversation_id']))
            step = int(s['step_index'])
            require(0 <= step < 10000, 'Invalid agy step index')
            directory = Path.home()/'.gemini/antigravity-cli/brain'/conversation/'.system_generated/steps'/str(step)
            cached = directory/'output.txt'
            row['owned_output_path'] = str(cached)
            if output is None and cached.is_file(): output = cached.read_text()
            row['output'] = output
            try:
                payload, row['image_bytes'] = agy_output(output, directory)
                if payload != output:
                    row['payload_output'] = payload
                    row['owned_image_paths'] = [str(directory/filename) for filename in
                        re.findall(r'/(media_[0-9]+\.png)\]', output[len(payload):])]
            except (AssertionError, ValueError, TypeError) as error:
                row['normalization_error'] = str(error)
        rows.append(row)
    return rows, terminal


def checked_read_refreshes(summary):
    incomplete = summary.get('incomplete_reads', 0)
    recovered = summary.get('recovered_incomplete_reads', 0)
    require(type(incomplete) is int and incomplete >= 0,
            'Invalid summary.incomplete_reads: expected a non-negative integer')
    require(type(recovered) is int and 0 <= recovered <= incomplete,
            'Invalid recovered incomplete read counter')
    require(recovered == incomplete, 'Execution summary contains unrecovered incomplete reads')
    issue = summary.get('last_issue')
    if incomplete == 0:
        require(issue is None, 'Execution summary contains an issue')
        return 0
    # This count is produced by DTW from the actual ledger, not by JS return
    # values: same window, new complete observation, no intervening action/error.
    require(isinstance(issue, dict) and issue.get('operation') == 'observe'
            and issue.get('status') == 'completed' and issue.get('error') is None
            and issue.get('receipt') is None and issue.get('inventory') is None,
            'Recovered reads cannot hide another issue')
    discovery = issue.get('discovery')
    require(isinstance(discovery, dict) and discovery.get('complete') is False
            and discovery.get('has_more') is False
            and type(discovery.get('visited')) is int and discovery['visited'] >= 0
            and all(isinstance(discovery.get(key), str) and discovery[key]
                    for key in ('window_ref', 'observation_ref')),
            'Recovered read diagnostic is missing or invalid')
    return recovered


def check_execution(data, arguments, compact=True):
    require(data.get('app') == arguments['app'] and isinstance(data.get('execution_id'), str)
            and data['execution_id'], 'Execution identity is missing or changed')
    require(data.get('status') == 'completed', 'Execution is not completed')
    require(data.get('error') is None and data.get('shutdown_error') is None
            and not data.get('shutdown_errors'), 'Execution or cleanup error')
    require(not data.get('execution_unsettled') and not data.get('payload_expired'), 'Execution is unsettled or payload expired')
    script = data.get('script')
    require(isinstance(script, dict), 'Execution script receipt missing')
    require(script.get('state_committed') is True and script.get('failure') is None
            and script.get('shutdown_error') is None, 'Script failed, did not commit, or cleanup failed')
    if not compact:
        # Cleanup uses the supervisor's full ledger; any failed/unknown step fails.
        steps = script.get('steps')
        require(isinstance(steps, list), 'Cleanup ledger missing')
        for step in steps:
            require(step.get('status') == 'completed' and step.get('error') is None, 'Cleanup step failed')
            receipt = step.get('receipt') or {}
            require(receipt.get('interruption') is None, 'Cleanup input interrupted')
            for outcome in ('outcome', 'provider_outcome'):
                value = receipt.get(outcome) or {}
                require(value.get('error') is None and value.get('dispatch') != 'unknown'
                        and value.get('evidence', {}).get('kind') != 'mismatch', 'Cleanup input uncertain')
        return
    require('steps' not in script, 'Full ledger leaked into normal exec')
    summary = script.get('summary')
    require(isinstance(summary, dict), 'Compact execution summary missing')
    for key in ('steps', 'failed', 'pending'):
        require(type(summary.get(key)) is int and summary[key] >= 0, 'Invalid summary counter: '+key)
    require(summary['failed'] == 0 and summary['pending'] == 0,
            'Execution summary contains failed or pending steps')
    checked_read_refreshes(summary)
    if 'actions' in summary:
        actions = summary['actions']
        require(isinstance(actions, dict), 'Invalid action summary')
        require(type(actions.get('interrupted')) is int and actions['interrupted'] == 0, 'Interrupted actions')
        for key in ('dispatch', 'provider_dispatch', 'evidence', 'provider_evidence'):
            counts = actions.get(key)
            require(isinstance(counts, dict) and all(type(n) is int and n >= 0 for n in counts.values()),
                    'Invalid action counters: '+key)
            require(not any(counts.get(bad, 0) for bad in ('unknown', 'unrecorded', 'mismatch')), 'Unknown or mismatched action')


def metadata_discovery_miss(call, name):
    """Recognize only this plugin's observed missing root Skill alias lookup."""
    path = str(Path.home()/'.gemini/config/plugins'/name/'SKILL.md')
    return (call.get('name') == 'read_file' and call.get('arguments') == {'Path':path}
            and call.get('status') == 'ERROR' and call.get('output') is None
            and call.get('client_error_no_data') is True
            and call.get('client_error') == {'type':'TOOL_ERROR','message':'file does not exist'})


def denied_route_attempt(call):
    """Only the client's explicit pre-execution command-denial envelope qualifies."""
    if (call.get('name') != 'run_command' or call.get('status') != 'ERROR'
            or call.get('output') is not None or call.get('client_error_no_data') is not True
            or call.get('client_preflight_only') is not True):
        return False
    error = call.get('client_error')
    arguments = call.get('arguments')
    if (not isinstance(error, dict) or set(error) != {'type','message'}
            or error['type'] != 'TOOL_ERROR' or not isinstance(error['message'], str)
            or not isinstance(arguments, dict) or set(arguments) != {'CommandLine'}):
        return False
    # Generic runtime "permission denied" errors do not prove non-execution.
    # The client quotes the full rejected command even when its displayed input
    # is truncated. Bind that quoted request to this exact tool attempt.
    prefix = 'permission check failed for unsandboxed '
    message = error['message']
    if not message.startswith(prefix): return False
    try:
        command, end = json.JSONDecoder().raw_decode(message[len(prefix):])
    except (ValueError, TypeError):
        return False
    tail = message[len(prefix)+end:]
    denial_prefix = ': Permission denied for unsandboxed('
    denial_suffix = '). Matches user-configured deny rule.'
    if (not isinstance(command, str) or not command
            or not tail.startswith(denial_prefix) or not tail.endswith(denial_suffix)):
        return False
    rejected = tail[len(denial_prefix):-len(denial_suffix)]
    # This client's permission display drops blank lines, but must still name
    # the same request. Do not accept unrelated text inside a denial wrapper.
    if ([line for line in rejected.splitlines() if line.strip()]
            != [line for line in command.splitlines() if line.strip()]):
        return False
    displayed = arguments['CommandLine']
    return isinstance(displayed, str) and (displayed == command
        or (len(displayed) > 1 and displayed.endswith('…') and command.startswith(displayed[:-1])))


def metrics(calls, name, browser_app="com.google.Chrome", *, allowed_apps=None):
    if allowed_apps is None:
        allowed_apps = (browser_app, 'com.apple.TextEdit')
    selected = []
    for c in calls:
        if c['name'] == 'call_mcp_tool':
            a = c['arguments']
            if isinstance(a, dict) and a.get('ServerName') == name:
                selected.append({**c, 'method':a['ToolName'], 'arguments':a['Arguments']})
        elif isinstance(c['name'], str) and c['name'].startswith(f'mcp__{name}__'):
            selected.append({**c, 'method':c['name'].split('__')[-1]})
    denied = [{'agent_call':number, 'tool':c['name'], 'duration_seconds':c.get('duration_seconds')}
              for number, c in enumerate(calls, 1) if denied_route_attempt(c)]
    # A blocked attempt did not bypass the GUI route, but is not a clean run.
    text_bytes = 0; images_total = 0; issues = [dict(kind='denied_route_attempt', **row) for row in denied]
    exec_count = 0; images_complete = True
    script_calls = []; read_refreshes = []
    for number, c in enumerate(selected, 1):
        if c['method'] == 'exec':
            exec_count += 1
            script_calls.append({'call':number, 'execution_id':None, 'calls':None})
        try:
            require(not c.get('normalization_error'), c.get('normalization_error'))
            image_scope = c.get('acp_image_scope')
            require(image_scope is None or image_scope['namespace'] == name, 'Image namespace mismatch')
            data, is_error, envelope_images = decode_output(c.get('payload_output', c.get('output')), image_scope)
            text_bytes += json_bytes(data)
            if c['method'] == 'exec':
                script_calls[-1]['execution_id'] = data.get('execution_id')
                value = data.get('value')
                if isinstance(value, dict) and 'calls' in value:
                    count = value['calls']
                    require(type(count) is int and count >= 0, 'Invalid exec.value.calls counter')
                    script_calls[-1]['calls'] = count
            images = envelope_images if envelope_images is not None else c.get('image_bytes')
            if envelope_images is not None and c.get('image_bytes') not in (None, 0, envelope_images):
                images = None
            capture = isinstance(data.get('value'), dict) and data['value'].get('last_capture') is not None
            if c.get('image_unavailable') or (capture and not images):
                images_complete = False
            elif images is not None:
                images_total += images
            require(not is_error, 'MCP isError is true')
            require(data.get('error') is None and data.get('shutdown_error') is None
                    and not data.get('shutdown_errors'), 'Tool or cleanup error')
            if c['method'] == 'exec':
                arguments = c['arguments']
                require(isinstance(arguments, dict) and set(arguments) == {'app','source'}, 'exec must use exactly app/source')
                require(arguments['app'] in allowed_apps, 'Unexpected target app')
                check_execution(data, arguments)
                recovered = data['script']['summary'].get('recovered_incomplete_reads', 0)
                if recovered:
                    read_refreshes.append({'call':number, 'execution_id':data['execution_id'],
                                          'count':recovered, 'diagnostic':data['script']['summary']['last_issue']})
            elif c['method'] == 'apps':
                require(data.get('version') == 1 and isinstance(data.get('apps'), list), 'Application catalog missing')
            elif c['method'] == 'api':
                require(isinstance(data.get('methods'), dict), 'Method metadata missing')
            elif c['method'] != 'result':
                raise AssertionError('Unknown MCP method')
        except (AssertionError, ValueError, TypeError, KeyError, UnicodeError) as error:
            issues.append({'call':number,'method':c['method'],'invalid_output':str(error)})
            # Unparseable output cannot prove image absence either.
            images_complete = False
        if c.get('status') not in ('completed','DONE'): issues.append({'call':number,'tool_status':c.get('status')})
    return {'agent_tool_calls':len(calls),'mcp_calls':len(selected),'exec_calls':exec_count,
            'script_host_calls':sum(row['calls'] for row in script_calls)
                if script_calls and all(row['calls'] is not None for row in script_calls) else None,
            'script_calls_by_exec':script_calls,
            'script_call_measurement':'exec.value.calls; attempted app methods, not individual platform API calls',
            'mcp_json_bytes':text_bytes,'json_measurement':'compact_utf8_dtw_payload_once; image content excluded',
            'image_bytes':images_total if images_complete else None,
            'image_measurement':'complete' if images_complete else 'payload_not_fully_exposed',
            'issues':issues,'read_refreshes':read_refreshes,
            'denied_route_attempts':len(denied),'denied_route_attempt_details':denied,
            'metadata_discovery_misses':[{'agent_call':number,'path':c['arguments']['Path'],
                'client_error':c['client_error'],'duration_seconds':c.get('duration_seconds')}
                for number, c in enumerate(calls, 1) if metadata_discovery_miss(c, name)],
            'result_queries':sum(c['method']=='result' for c in selected)}


def skill_paths(name, root):
    return {str(path) for path in (
        Path.home()/'.gemini/config/plugins'/name/'skills'/name/'SKILL.md',
        root/'plugin/skills'/name/'SKILL.md',
        root/'.claude/skills'/name/'SKILL.md',
        root/'.agents/skills'/name/'SKILL.md')}


def dsh_skill_loaded(output, name, root):
    """Prove DSH exposed this staged Skill's complete instructions and base path."""
    path = root/'.agents/skills'/name/'SKILL.md'
    if not path.is_file(): return False
    if isinstance(output, list):
        if len(output) != 1: return False
        block = output[0]
        if not isinstance(block, dict): return False
        if block.get('type') == 'content': block = block.get('content', {})
        if not isinstance(block, dict) or block.get('type') != 'text': return False
        output = block.get('text')
    if not isinstance(output, str): return False
    pattern = (r'<skill_content name="'+re.escape(name)+r'">\s*<skill_resources>\n'
               r'Base directory for this skill: ([^\n]+)\n.*?</skill_resources>\s*'
               r'<skill_instructions>\n(.*)\n</skill_instructions>\s*</skill_content>')
    match = re.fullmatch(pattern, output.strip(), re.DOTALL)
    source = re.sub(r'\A---\r?\n.*?\r?\n---(?:\r?\n|\Z)', '', path.read_text(), count=1, flags=re.DOTALL)
    return (match is not None and match[1] == str(path.parent)
            and match[2].strip() == source.strip())


def check_skill_loaded(calls, name, root):
    for call in calls:
        if call.get('status') not in ('completed', 'DONE'): continue
        arguments = call.get('arguments') or {}
        output = call.get('output')
        if call.get('name') == 'skill' and arguments.get('name') == name:
            if dsh_skill_loaded(output, name, root): return
        if call.get('name') == 'Skill' and arguments.get('skill') == name:
            if output == 'Launching skill: '+name: return
        if call.get('name') in ('read_file', 'Read'):
            path = arguments.get('Path', arguments.get('file_path'))
            if path not in skill_paths(name, root) or not isinstance(output, str): continue
            # agy exposes the successful read's size, not its full contents.
            size = re.fullmatch(r'([1-9][0-9]*) lines, ([1-9][0-9]*) bytes', output)
            if size or re.search(r'^name: '+re.escape(name)+r'\s*$', output, re.MULTILINE): return
    raise AssertionError('This invocation\'s Skill was not successfully loaded')


def check_routes(calls, name, root):
    """Allow ordinary client discovery only for this invocation's exact paths."""
    metadata = Path.home()/'.gemini/antigravity-cli/mcp'/name
    reads = {str(metadata/filename) for filename in
             ('instructions.md', 'apps.json', 'api.json', 'exec.json', 'result.json')}
    reads.update(skill_paths(name, root))
    reads.update(str(Path(path).with_name('PATTERNS.md')) for path in skill_paths(name, root))
    # Only output caches belonging to this run's own DTW calls can be inspected.
    reads.update(c['owned_output_path'] for c in calls
                 if c.get('name') == 'call_mcp_tool'
                 and (c.get('arguments') or {}).get('ServerName') == name
                 and c.get('owned_output_path'))
    reads.update(path for c in calls
                 if c.get('name') == 'call_mcp_tool'
                 and (c.get('arguments') or {}).get('ServerName') == name
                 for path in c.get('owned_image_paths', []))
    for call in calls:
        tool = call['name']; arguments = call.get('arguments') or {}
        if tool == 'skill':
            require(arguments.get('name') == name, 'Unexpected Skill'); continue
        if tool == 'read':
            local = root/'.agents/skills'/name
            require(arguments.get('file_path') in {str(local/'SKILL.md'), str(local/'PATTERNS.md')},
                    'Agent read task data outside the GUI route'); continue
        if tool == 'Skill':
            require(arguments.get('skill') == name, 'Unexpected Skill'); continue
        if tool == 'ToolSearch': continue
        if tool == 'call_mcp_tool':
            require(arguments.get('ServerName') == name, 'Unexpected MCP server'); continue
        if str(tool).startswith(f'mcp__{name}__'): continue
        if tool in ('read_file', 'Read'):
            read = arguments.get('Path', arguments.get('file_path', ''))
            require(read in reads or metadata_discovery_miss(call, name),
                    'Agent read task data outside the GUI route'); continue
        if tool == 'run_command':
            if denied_route_attempt(call): continue
            try:
                argv = shlex.split(arguments.get('CommandLine', ''))
            except ValueError as error:
                raise AssertionError('Unparseable shell request cannot prove an allowed metadata listing') from error
            require(argv[:-1] in (['ls', '-la'], ['ls']) and '..' not in Path(argv[-1]).parts
                    and Path(argv[-1]) == metadata,
                    'Shell access is limited to this MCP metadata listing'); continue
        raise AssertionError('Unexpected Agent tool: '+str(tool))


def check_terminal(client, terminal, returncode, stderr=''):
    require(returncode == 0, 'Agent did not exit successfully')
    require(isinstance(terminal, dict), 'Agent completion terminal missing')
    require(not terminal.get('error'), 'Agent terminal contains an error')
    if client in ('claude', 'dsh'):
        require(terminal.get('stopReason') == 'end_turn', 'Agent did not complete an ordinary turn')
    else:
        # agy can emit SUCCESS and exit 0 after its print deadline while the
        # turn is still incomplete. This observed warning overrides the label.
        require(not any(line.strip().startswith('[agy] print timeout ')
                        or (line.strip().startswith('[agy]')
                            and 'turn in progress; returning partial output' in line)
                        for line in stderr.splitlines()),
                'agy returned partial output with an unfinished turn')
        result = terminal.get('result')
        require(terminal.get('event') == 'result' and isinstance(result, dict)
                and result.get('status') == 'SUCCESS' and not result.get('error'), 'agy did not report SUCCESS')


def process_cleaned(result):
    require(isinstance(result, dict) and type(result.get('returncode')) is int
            and result['returncode'] == 0 and result.get('forced') == [], 'Process cleanup failed or required force')
    if 'native_workers_reaped' in result:
        require(result['native_workers_reaped'] is True, 'Native workers were not reaped')


def stable_execution(record, app):
    check_execution(record, {'app':app}, compact=False)
    require(record.get('context_reset') is False and record.get('state_reset') is False,
            'Owned window identity cannot survive a context or state reset')


def checked_windows(record, app):
    stable_execution(record, app)
    inventory = successful(record)
    require(isinstance(inventory, dict), 'Window inventory missing')
    coverage = inventory.get('coverage') or {}
    require(coverage.get('status') in ('available', 'empty_within_scope')
            and coverage.get('complete_within_scope') is True,
            'Incomplete inventory cannot prove a window disappeared')
    windows = inventory.get('windows')
    require(isinstance(windows, list) and all(isinstance(w, dict)
            and isinstance(w.get('window_ref'), str) and w['window_ref'] for w in windows),
            'Invalid window identities')
    require(len({w['window_ref'] for w in windows}) == len(windows), 'Duplicate window identities')
    require(coverage['status'] != 'empty_within_scope' or not windows,
            'Empty inventory status contradicts its windows')
    return windows


def close_owned_window(client, app, window_ref, proof, timeout=5, epoch=None):
    """Dispatch at most once; only full, stable inventories prove disappearance."""
    proof.update(window_ref=window_ref, inventories=[], dispatch=None, closed=False)
    require(isinstance(window_ref, str) and window_ref, 'Owned setup window identity is missing')
    require(type(epoch) is int, 'Owned setup context identity is missing')
    proof['context_epoch'] = epoch

    def same_context():
        require(client.context_epochs.get(app, 0) == epoch,
                'Owned setup context changed before window disappearance was proven')

    def present():
        same_context()
        record, _ = client.execute(app, 'cleanup-inventory', 'return await app.windows();')
        proof['inventories'].append(record)
        same_context()
        return any(w['window_ref'] == window_ref for w in checked_windows(record, app))

    if present():
        source = 'await app.keypress({window_ref:'+json.dumps(window_ref)+',keys:["META","W"]});return true;'
        record, _ = client.execute(app, 'cleanup-close', source)
        proof['dispatch'] = record
        same_context()
        stable_execution(record, app)
        deadline = time.monotonic() + timeout
        while present():
            require(time.monotonic() < deadline, 'Owned window is still present after one close request')
            time.sleep(0.05)
    proof['closed'] = True
    return proof


def check_business_state(state, case):
    require(state.get('case') == case and state.get('ready') is True, 'Wrong or unready business fixture')
    expected = {}
    for i in range(1, 241):
        sku = f'DTW-{i:04}'
        expected[sku] = {'sku':sku, 'name':f'Field kit {i:03}',
                         'warehouse':['North','South','Central'][i % 3],
                         'quantity':QUANTITIES.get(sku, 10+i%25),
                         'reviewed':sku in QUANTITIES, 'note':NOTES.get(sku, '')}
    items = state.get('items')
    require(isinstance(items, list) and len(items) == len(expected), 'Business rows are missing or duplicated')
    seen = set()
    for row in items:
        sku = row.get('sku')
        require(sku in expected and sku not in seen, 'Unknown or duplicate business row')
        seen.add(sku)
        require(type(row.get('quantity')) is int and type(row.get('reviewed')) is bool
                and all(row.get(key) == value for key, value in expected[sku].items()),
                'Final business row differs: '+sku)
    saves = state.get('saves')
    require(isinstance(saves, list) and len(saves) == 2, 'Expected exactly two independent saves')
    for sku in QUANTITIES:
        rows = [row for row in saves if row.get('sku') == sku]
        require(len(rows) == 1 and type(rows[0].get('quantity')) is int
                and type(rows[0].get('reviewed')) is bool
                and all(rows[0].get(key) == value for key, value in expected[sku].items()),
                'Save journal differs: '+sku)


def business_snapshot(root, document):
    raw = document.read_bytes()
    return {'browser':json.loads((root/'web/state.json').read_text()),
            'document_text':raw.decode('utf-8'), 'document_sha256':hashlib.sha256(raw).hexdigest()}


def check_business_snapshot(snapshot, case):
    check_business_state(snapshot['browser'], case)
    require(snapshot['document_text'].rstrip('\n') == REPORT.rstrip('\n'), 'Saved report differs')


def check_final_business(evidence, root, document, case, browser_app):
    evidence['business_final_passed'] = False
    evidence['business_passed'] = False
    snapshot = business_snapshot(root, document)
    evidence['business_final'] = snapshot
    evidence['browser_final'] = snapshot['browser']
    closed = {row['resource']:row['ok'] for row in evidence.get('cleanup', [])}
    require(all(closed.get(resource) is True for resource in
                (browser_app, 'com.apple.TextEdit', 'mcp', 'server')),
            'Final business state is not proven quiescent')
    check_business_snapshot(snapshot, case)
    evidence['business_final_passed'] = True
    evidence['business_passed'] = evidence.get('business_initial_passed') is True
    return {'business_final_passed':True}


def cleanup_steps(evidence, steps):
    """Retain every result/failure and continue cleaning other owned resources."""
    before_cleanup_passed = evidence['passed']
    evidence['cleanup'] = []
    for resource, action, validate in steps:
        row = {'resource':resource}
        try:
            row['result'] = action()
            if validate: validate(row['result'])
            row['ok'] = True
        except BaseException as error:
            row.update(ok=False, error={'type':type(error).__name__, 'message':str(error)})
        evidence['cleanup'].append(row)
    evidence['task_passed'] = evidence.get('business_passed', before_cleanup_passed)
    evidence['cleanup_passed'] = all(row['ok'] for row in evidence['cleanup'])
    evidence['passed'] = before_cleanup_passed and evidence['cleanup_passed']
    if before_cleanup_passed and not evidence['passed']:
        evidence.setdefault('failure', {'type':'CleanupFailure','message':'One or more owned resources did not cleanly close'})


def usability_passed(metrics):
    return (metrics['mcp_calls'] > 0 and metrics['exec_calls'] <= 8
            and metrics['mcp_calls'] <= 12 and metrics['mcp_json_bytes'] <= 40 * 1024
            and not metrics['issues'] and metrics['result_queries'] == 0)


def native_task(client, title, report_name, evidence, browser_app):
    helper = """
async function find(role,text){
 const started=Date.now(),attempts=[];
 for(let attempt=0;attempt<4 && (attempt===0 || Date.now()-started<2000);attempt++){
  let p=await app.observe({window_ref:state.w,role,text,limit:5}),matches=[];
  const observation_ref=p.observation_ref;
  for(;;){matches.push(...p.elements.filter(e=>e.name===text));
   if(!p.next)break;
   p=await app.observe({cursor:p.next});
  }
  attempts.push(p.coverage);
  if(matches.length>1)throw Error('Ambiguous '+text);
  if(p.coverage.complete && matches.length===1)
   return {window_ref:state.w,observation_ref,element_ref:matches[0].element_ref};
 }
 throw Error('Missing or incomplete '+text+' '+JSON.stringify(attempts));
}
"""
    selected = 'const ws=(await app.windows()).windows.filter(w=>w.title==='+json.dumps(title)+');if(ws.length!==1)throw Error("owned window");state.w=ws[0].window_ref;'
    # Match the Skill's first-window inspection before task-specific edits.
    # A cold Chrome AX tree can initially contain browser chrome alone. Keep
    # that discovery and image, then use bounded fresh reads; never retry input.
    record, _ = client.execute(browser_app, 'initial-discovery', selected +
        'const page=await app.observe({window_ref:state.w,role:"text_field",limit:12});'
        'await app.capture({window_ref:state.w});return {coverage:page.coverage};')
    evidence['initial_discovery'] = successful(record)
    readiness = evidence['native_readiness'] = {'attempts':[]}
    started = time.monotonic()
    def ready():
        record, _ = client.execute(browser_app, 'native-readiness',
            'let p=await app.observe({window_ref:state.w,role:"text_field",text:"Search inventory",limit:5});'
            'const matches=[];for(;;){matches.push(...p.elements.filter(e=>e.name==="Search inventory"));'
            'if(!p.next)break;p=await app.observe({cursor:p.next});}'
            'return {count:matches.length,coverage:p.coverage};')
        readiness['attempts'].append(record)
        result = successful(record)
        require(result['count'] <= 1, 'Native readiness target is ambiguous')
        return result if result['coverage']['complete'] and result['count'] == 1 else None
    wait_until(ready, timeout=5)
    readiness['seconds'] = time.monotonic()-started
    for sku, quantity in QUANTITIES.items():
        source = selected + helper + f'''
await app.setValue({{...await find('text_field','Search inventory'),value:{json.dumps(sku)}}});
await app.press(await find('button','Edit {sku}'));
await app.setValue({{...await find('text_field','Target quantity'),value:'{quantity}'}});
await app.press(await find('checkbox','Reviewed'));
await app.setValue({{...await find('text_area','Handoff note'),value:{json.dumps(NOTES[sku])}}});
await app.press(await find('button','Save changes'));
return await app.observe({{window_ref:state.w,text:'Saved {sku}',limit:3}});
'''
        record, _ = client.execute(browser_app, sku, source)
        evidence[sku] = successful(record)
    source = 'const ws=(await app.windows()).windows.filter(w=>w.title.includes('+json.dumps(report_name)+'));if(ws.length!==1)throw Error("owned document");const window_ref=ws[0].window_ref;'
    source += 'await app.keypress({window_ref,keys:["META","A"]});await app.typeText({window_ref,text:'+json.dumps(REPORT)+'});await app.keypress({window_ref,keys:["META","S"]});return true;'
    record, _ = client.execute('com.apple.TextEdit', 'report', source); successful(record)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--installed-package', type=Path,
                        help='Extracted unpublished macOS candidate; execute its bin/dtw in place')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--client', choices=['native','claude','agy','dsh'], required=True)
    parser.add_argument('--acp-adapter', type=Path)
    parser.add_argument('--browser', choices=['chrome','safari'], default='chrome')
    args = parser.parse_args()
    browser_app, browser_name = {'chrome':('com.google.Chrome','Google Chrome'),'safari':('com.apple.Safari','Safari')}[args.browser]
    repo = Path(__file__).resolve().parents[2]
    root = args.output.absolute()
    if args.installed_package is not None:
        require(not root.resolve().is_relative_to(args.installed_package.resolve(strict=True)),
                'Output must be outside the installed package')
    root.mkdir(mode=0o700)
    case = 'case-'+uuid.uuid4().hex[:10]; name = 'dtw-workbench-'+case
    title = 'DTW Workbench '+case
    binary, artifact = prepare_runtime(repo, root, args.dtw, args.installed_package)
    package = Path(artifact['installed_package']['path']) if args.installed_package is not None else None
    document = root/('DTW-Report-'+case+'.txt'); document.write_text('Pending review\n')
    evidence = {'kind':'real-browser-and-TextEdit-business-task','browser':browser_name,'client':args.client,
                **artifact,'case':case,
                'passed':False,'business_passed':None,'usability_passed':None,
                'business_initial_passed':None,'business_final_passed':None,
                'owned_windows':{},'owned_window_epochs':{},'window_cleanup':{},
                'started_unix_ms':int(time.time()*1000)}
    server = client = agent = chrome = chrome_log = None; installed = registered = False
    try:
        server = subprocess.Popen(['python3',str(repo/'tests/fixtures/browser-workbench/server.py'),
                                   '--output',str(root/'web'),'--case',case],stdout=subprocess.PIPE,text=True)
        address = json.loads(server.stdout.readline())
        client = initialize(binary, root/'supervisor')
        if args.browser == 'chrome':
            profile = root/'chrome-profile'; profile.mkdir()
            chrome_log = (root/'chrome.log').open('w')
            chrome_args = ['/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
                           '--user-data-dir='+str(profile), '--no-first-run', '--no-default-browser-check',
                           '--disable-background-mode', '--new-window', address['url']]
            chrome = subprocess.Popen(chrome_args, stdout=chrome_log, stderr=subprocess.STDOUT)
            evidence['browser_process'] = {'pid':chrome.pid, 'profile':str(profile),
                                           'ownership':'exact child with fresh unique profile'}
        else:
            subprocess.run(['open','-a','Safari'],check=True,timeout=10)
            def existing_window():
                record,_=client.execute(browser_app,'setup-browser-ready',
                    'return (await app.windows()).windows.filter(w=>w.visible && w.title);')
                if record.get('error',{}): return None
                return successful(record)
            existing = wait_until(existing_window,timeout=12)
            # Setup creates a new owned browser window using ordinary GUI input;
            # no Apple Events, browser DOM or profile changes are involved.
            source='state.before=(await app.windows()).windows.map(w=>w.window_ref);await app.keypress({window_ref:'+json.dumps(existing[0]['window_ref'])+',keys:["META","N"]});return true;'
            record,_=client.execute(browser_app,'setup-new-window',source);successful(record)
            def new_window():
                record,_=client.execute(browser_app,'setup-window-inventory','return (await app.windows()).windows.filter(w=>!state.before.includes(w.window_ref)&&w.visible&&w.title);')
                matches=successful(record)
                require(len(matches)<=1,'New browser window is ambiguous')
                return matches[0] if matches else None
            owned = wait_until(new_window,timeout=8)
            source='const window_ref='+json.dumps(owned['window_ref'])+';await app.keypress({window_ref,keys:["META","L"]});await app.typeText({window_ref,text:'+json.dumps(address['url'])+'});await app.keypress({window_ref,keys:["RETURN"]});return true;'
            record,_=client.execute(browser_app,'setup-owned-page',source);successful(record)
        wait_until(lambda:json.loads((root/'web/state.json').read_text())['ready'],timeout=15)
        def page_window():
            record,_=client.execute(browser_app,'setup-page-title','return await app.windows();')
            found=[w for w in checked_windows(record,browser_app) if title in w['title']]
            require(len(found)<=1,'Owned page title is ambiguous')
            return found[0] if found else None
        owned=wait_until(page_window,timeout=5)
        title=owned['title'];evidence['owned_windows'][browser_app]=owned['window_ref']
        evidence['owned_window_epochs'][browser_app]=client.context_epochs.get(browser_app,0)
        evidence['browser_window_title']=title
        subprocess.run(['open','-a','TextEdit',str(document)],check=True,timeout=10)
        # Independent setup observes only WindowServer inventory, never warms AX.
        def opened():
            record,_=client.execute('com.apple.TextEdit','setup-document','return await app.windows();')
            found=[w for w in checked_windows(record,'com.apple.TextEdit') if document.stem in w['title']]
            require(len(found)<=1,'Owned document is ambiguous')
            return found[0] if found else None
        owned=wait_until(opened,timeout=10)
        evidence['owned_windows']['com.apple.TextEdit']=owned['window_ref']
        evidence['owned_window_epochs']['com.apple.TextEdit']=client.context_epochs.get('com.apple.TextEdit',0)
        prompt = f'''Use the {name} Skill and its DTW MCP server to complete this disposable inventory review. In {browser_name}, operate only the window titled "{title}". Find SKU DTW-0088, change Target quantity to 37, mark Reviewed, set Handoff note to "{NOTES['DTW-0088']}", and save. Then do the same for DTW-0240 with quantity 52 and note "{NOTES['DTW-0240']}". Confirm each save from the page. Each record should be saved exactly once; do not replay an uncertain write. Then switch to TextEdit and replace only the open document "{document.name}" with this text, and save it with the application's ordinary shortcut:\n{REPORT}\nUse standard DTW tools for every GUI operation. Only these two owned windows are authorized; preserve other windows and tabs. Do not use shell, browser/DOM, HTTP, filesystem edits or direct backend access. Loading the named Skill and MCP tool metadata through the Agent's normal discovery is allowed. Work efficiently with sequential scripts and filtered discovery; keep outputs limited to relevant facts. Verify the outcomes and report any uncertainty. Do not close the windows; the supervisor handles cleanup.'''
        (root/'prompt.txt').write_text(prompt)
        if args.client == 'native':
            start = time.monotonic(); native_task(client,title,document.stem,evidence,browser_app)
            evidence['agent_wall_seconds'] = round(time.monotonic()-start,3)
        else:
            stage, skill = stage_agent_plugin(repo, root, binary, name, package)
            env = os.environ.copy()
            if args.client == 'claude':
                require(args.acp_adapter is not None,'Official ACP adapter executable required')
                local=root/'.claude/skills'/name;local.mkdir(parents=True);shutil.copytree(skill.parent,local,dirs_exist_ok=True)
                settings=Path.home()/'.claude/settings.json'
                if settings.exists(): env.update({k:str(v) for k,v in json.loads(settings.read_text()).get('env',{}).items()})
                env['CLAUDE_CODE_EXECUTABLE']=shutil.which('claude')
                config=root/'mcp.json';write_json(config,{'mcpServers':[{'name':name,'command':str(binary),'args':['mcp'],'env':[]}]})
                allowed=['Skill',f"Read({local/'PATTERNS.md'})"]+[f'mcp__{name}__{m}' for m in ['apps','api','exec','result']]
                cmd=['acpx','--agent',str(args.acp_adapter),'--cwd',str(root),'--mcp-config',str(config),'--format','json','--timeout','600','--model','codex/gpt-6.1-terra','--deny-all','--allowed-tools',','.join(allowed),'--max-turns','18','--no-fs','--no-terminal','exec',prompt]
            elif args.client == 'dsh':
                cmd = prepare_dsh_agent(root, skill, args.acp_adapter, name, [str(binary), 'mcp'], 600) + [prompt]
            else:
                # agy currently needs its ordinary absolute-path MCP registration;
                # a plugin's CLAUDE_PLUGIN_ROOT placeholder is not portable to it.
                (stage/'.mcp.json').unlink(missing_ok=True)
                with (root/'setup.log').open('w') as log:
                    subprocess.run(['agy','plugin','install',str(stage)],check=True,stdout=log,stderr=log);installed=True
                    subprocess.run(['agy','mcp','add',name,str(binary),'mcp'],check=True,stdout=log,stderr=log);registered=True
                cmd=['agy','--print',prompt,'--dangerously-skip-permissions','--output-format','stream-json','--print-timeout','600s','--log-file',str(root/'agy.log')]
            start=time.monotonic()
            with (root/'agent.jsonl').open('w') as out,(root/'agent-stderr.log').open('w') as err:
                agent=subprocess.Popen(cmd,cwd=root,env=env,stdout=out,stderr=err)
                # Resource ceiling only; elapsed time is not a pass/fail SLA.
                agent.wait(timeout=620)
            evidence['agent_wall_seconds']=round(time.monotonic()-start,3);evidence['agent_returncode']=agent.returncode
            calls, terminal=(normalize_agy(root/'agent.jsonl') if args.client=='agy'
                             else normalize_acp(root/'agent.jsonl', args.client))
            write_json(root/'normalized-calls.json',calls)
            evidence['metrics']=metrics(calls,name,browser_app);evidence['terminal']=terminal
            evidence['usability_passed'] = usability_passed(evidence['metrics'])
            check_terminal(args.client, terminal, agent.returncode, (root/'agent-stderr.log').read_text())
            check_routes(calls, name, root)
            check_skill_loaded(calls, name, root)
        evidence['business_passed'] = False
        evidence['business_initial_passed'] = False
        def report_ready():
            text=document.read_text()
            return text if text.rstrip('\n')==REPORT.rstrip('\n') else None
        wait_until(report_ready,timeout=4)
        snapshot=business_snapshot(root,document);evidence['business_initial']=snapshot
        state=snapshot['browser'];evidence['browser_saves']=state['saves'];evidence['browser_event_count']=len(state['events'])
        evidence.update({key:snapshot[key] for key in ('document_sha256','document_text')})
        check_business_snapshot(snapshot,case)
        evidence['business_initial_passed'] = True
        evidence['business_passed'] = True
        if args.client!='native':
            require(evidence['usability_passed'], 'Usability gate not met')
        evidence['passed']=True
    except BaseException as error:
        evidence['failure']={'type':type(error).__name__,'message':str(error)}
        (root/'failure.txt').write_text(traceback.format_exc())
    finally:
        steps = []
        if agent is not None and agent.poll() is None:
            def close_agent():
                agent.terminate()
                result = stop_owned(agent)
                result['forced'].insert(0, 'terminate')
                return result
            steps.append(('agent', close_agent, process_cleaned))
        if client is not None:
            for app in (browser_app,'com.apple.TextEdit'):
                safe_document_close = (evidence.get('business_passed') is True
                    and not (evidence.get('metrics') or {}).get('issues'))
                if app=='com.apple.TextEdit' and not safe_document_close: continue
                proof={};evidence['window_cleanup'][app]=proof
                def close_window(app=app, proof=proof):
                    return close_owned_window(client,app,evidence['owned_windows'].get(app),proof,
                                              epoch=evidence['owned_window_epochs'].get(app))
                steps.append((app, close_window, None))
            steps.append(('mcp', client.close, process_cleaned))
        if server is not None:
            def close_server():
                # Even if signalling via the owned stop file fails, reap the
                # exact process and retain both the signalling and exit evidence.
                signal_error = None
                try: (root/'web/stop').touch()
                except OSError as error: signal_error = str(error)
                result = stop_owned(server)
                if signal_error: result['signal_error'] = signal_error
                return result
            def server_cleaned(result):
                process_cleaned(result)
                require(not result.get('signal_error'), 'Could not request server shutdown')
            steps.append(('server', close_server, server_cleaned))
        if chrome is not None:
            def close_chrome():
                if chrome.poll() is None: chrome.terminate()
                return stop_owned(chrome)
            def chrome_cleaned(result):
                require(result.get('returncode') in (0,-15) and result.get('forced') == [],
                        'Owned Chrome required forced cleanup')
            steps.append(('owned_chrome_process', close_chrome, chrome_cleaned))
        if (root/'web/state.json').exists():
            steps.append(('final_business', lambda:check_final_business(
                evidence,root,document,case,browser_app), None))
        def unregister(command):
            with (root/'cleanup.log').open('a') as log:
                result = subprocess.run(command, stdout=log, stderr=log, timeout=20)
            return {'returncode':result.returncode, 'forced':[]}
        if registered:
            steps.append(('agy_mcp', lambda:unregister(['agy','mcp','remove',name]), process_cleaned))
        if installed:
            steps.append(('agy_plugin', lambda:unregister(['agy','plugin','uninstall',name]), process_cleaned))
        if package is not None:
            steps.append(('installed_package_integrity', lambda:recheck_installed_package(artifact), None))
        cleanup_steps(evidence, steps)
        if chrome_log is not None: chrome_log.close()
        evidence['finished_unix_ms']=int(time.time()*1000);write_json(root/'evidence.json',evidence)
    print(json.dumps({k:evidence.get(k) for k in ['passed','business_passed','usability_passed','cleanup_passed','client','agent_wall_seconds','metrics','failure']},ensure_ascii=False))
    return 0 if evidence['passed'] else 1


if __name__=='__main__':raise SystemExit(main())
