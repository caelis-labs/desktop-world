#!/usr/bin/env python3
"""Opt-in real WPS / Obsidian / Chrome tasks through ordinary Agent + DTW.

Owns only disposable documents, profiles and registrations. Task files are
prepared/read by the supervisor, never exposed as an alternative Agent route.
Every attempt keeps its transcript, including unsuccessful attempts. GUI
reopening/search evidence requires transcript review in addition to disk checks.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import time
import traceback
import uuid
import xml.etree.ElementTree as ET
import zipfile
from xml.sax.saxutils import escape

from standard_agent_probe import require, successful, wait_until, write_json, stop_owned
from browser_workbench_probe import (
    NOTES, QUANTITIES, check_business_state, check_routes, check_skill_loaded,
    check_terminal, cleanup_steps, file_sha256, initialize,
    metrics, normalize_acp, normalize_agy, prepare_runtime, process_cleaned, checked_windows, stable_execution,
    stage_agent_plugin,
)

APPS = {'wps': ('com.kingsoft.wpsoffice.mac', '/Applications/wpsoffice.app'),
        'obsidian': ('md.obsidian', '/Applications/Obsidian.app'),
        'chrome': ('com.google.Chrome', '/Applications/Google Chrome.app')}
W = '{http://schemas.openxmlformats.org/wordprocessingml/2006/main}'


def owned_window_titles(child):
    """Read only the exact child PID's CG metadata; do not warm AX or activate it."""
    require(child.poll() is None, 'Owned application exited during setup')
    cg = ctypes.CDLL('/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics')
    cf = ctypes.CDLL('/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation')
    cg.CGWindowListCopyWindowInfo.argtypes = [ctypes.c_uint32, ctypes.c_uint32]
    cg.CGWindowListCopyWindowInfo.restype = ctypes.c_void_p
    cf.CFPropertyListCreateData.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_long,
                                         ctypes.c_ulong, ctypes.c_void_p]
    cf.CFPropertyListCreateData.restype = ctypes.c_void_p
    cf.CFDataGetLength.argtypes = [ctypes.c_void_p]; cf.CFDataGetLength.restype = ctypes.c_long
    cf.CFDataGetBytePtr.argtypes = [ctypes.c_void_p]; cf.CFDataGetBytePtr.restype = ctypes.c_void_p
    cf.CFRelease.argtypes = [ctypes.c_void_p]
    windows = cg.CGWindowListCopyWindowInfo(16, 0)
    require(windows, 'Native window inventory unavailable for setup ownership proof')
    data = None
    try:
        data = cf.CFPropertyListCreateData(None, windows, 100, 0, None)
        require(data, 'Native window inventory serialization failed')
        size = cf.CFDataGetLength(data)
        require(0 < size < 16 * 1024 * 1024, 'Native setup inventory size is invalid')
        rows = plistlib.loads(ctypes.string_at(cf.CFDataGetBytePtr(data), size))
        require(child.poll() is None, 'Owned application exited during ownership proof')
        return [row.get('kCGWindowName') for row in rows
                if row.get('kCGWindowOwnerPID') == child.pid
                and row.get('kCGWindowLayer') == 0]
    finally:
        if data: cf.CFRelease(data)
        cf.CFRelease(windows)


def leave_gui_review_pending(evidence):
    """An on-disk result cannot prove the route taken through the GUI."""
    evidence['automatic_checks_passed'] = evidence['passed']
    evidence['passed'] = False
    evidence['acceptance_status'] = ('pending_gui_review' if evidence['automatic_checks_passed']
                                     else 'automatic_checks_failed')


def close_visible_owned_document(client, app_id, path):
    """Close one visible owned document, not an entire tabbed WPS window.

    A changed title proves only that the document is no longer displayed. Never
    repeat Close to make the containing window disappear: it may hold user tabs.
    """
    stem = Path(path).stem
    epoch = client.context_epochs.get(app_id, 0)
    proof = {'document': str(path), 'dispatch': None, 'inventories': []}

    def matching():
        record, _ = client.execute(app_id, 'cleanup-document-inventory', 'return await app.windows();')
        proof['inventories'].append(record)
        require(client.context_epochs.get(app_id, 0) == epoch, 'Owned document context changed')
        rows = [w for w in checked_windows(record, app_id) if stem in w['title']]
        require(len(rows) <= 1, 'Owned document is not uniquely visible during cleanup')
        return rows

    rows = matching()
    if not rows:
        proof['status'] = 'not_visible_no_action'
        return proof
    source = 'await app.keypress({window_ref:'+json.dumps(rows[0]['window_ref'])+',keys:["META","W"]});return true;'
    record, _ = client.execute(app_id, 'cleanup-document-close', source)
    proof['dispatch'] = record
    require(client.context_epochs.get(app_id, 0) == epoch, 'Owned document context changed')
    stable_execution(record, app_id)
    deadline = time.monotonic() + 5
    while matching():
        require(time.monotonic() < deadline, 'Owned document remains visible after one close request')
        time.sleep(0.05)
    proof['status'] = 'close_dispatched_document_no_longer_visible'
    return proof


def seed_docx(path, text):
    """A disposable starting file, not the document the Agent must create."""
    types = ('<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
             '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
             '<Default Extension="xml" ContentType="application/xml"/>'
             '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>')
    relationships = ('<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
                     '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>')
    document = ('<w:document xmlns:w="'+W[1:-1]+'"><w:body><w:p><w:r><w:t>'
                +escape(text)+'</w:t></w:r></w:p><w:sectPr/></w:body></w:document>')
    with zipfile.ZipFile(path, 'x', zipfile.ZIP_DEFLATED) as archive:
        for name, content in (('[Content_Types].xml', types), ('_rels/.rels', relationships),
                              ('word/document.xml', document)):
            archive.writestr(name, '<?xml version="1.0" encoding="UTF-8"?>'+content)


def docx_paragraphs(path):
    with zipfile.ZipFile(path) as archive:
        info = archive.getinfo('word/document.xml')
        require(info.file_size <= 16 * 1024 * 1024, 'Unexpected DOCX document size')
        document = ET.fromstring(archive.read(info))
    return [''.join(node.text or '' for node in paragraph.iter(W+'t'))
            for paragraph in document.iter(W+'p')]


def task_spec(kind, root, case):
    if kind == 'wps':
        seed = root/('DTW-Seed-'+case+'.docx')
        seed_docx(seed, 'Keep this starting document unchanged. '+case)
        destination = root/'saved documents'; destination.mkdir()
        final = destination/('Review-'+case+'.docx')
        lines = ['项目核对 '+case, '负责人：Alex', '库存目标：37', '已复核：内容正确']
        return {'seed': str(seed), 'seed_sha256': file_sha256(seed),
                'output': str(final), 'paragraphs': lines,
                'prompt': f'''In WPS Office, use the disposable starting document "{seed.name}" only to create a NEW blank word-processing document. Preserve the starting document. Enter these four separate paragraphs:\n{chr(10).join(lines[:-1])}\n需要修改：旧内容\nThen replace the last paragraph with "{lines[-1]}" through the GUI. Use the application's native Save As dialog to save the NEW DOCX at "{final}" (the directory already exists). Close that saved document, reopen it using WPS's normal Open dialog, and read back its title and all four paragraphs. Leave it open for supervision.'''}
    if kind == 'obsidian':
        location = root/'vault location'; location.mkdir()
        name = 'DTW-'+case
        summary = '# 项目核对\n'+case+'\n负责人：Alex\n后续事项见 [[Tasks]]\n'
        tasks = '# Tasks\n- [x] 核对库存 37\n- [ ] 安排补货\n'
        return {'location': str(location), 'vault': str(location/name),
                'notes': {'Projects/Summary.md': summary, 'Projects/Tasks.md': tasks},
                'prompt': f'''In the fresh isolated Obsidian welcome window, create a NEW local vault named "{name}" at "{location}". Select that location through Obsidian's Browse button and the native directory chooser. Do not open an existing personal vault. In the new vault create a Projects folder and two notes. Projects/Summary.md must contain:\n{summary}\nProjects/Tasks.md must contain:\n{tasks}\nUse Obsidian search to find the unique marker "{case}", open Summary from the results, and follow its internal Tasks link. Verify the saved note content through the UI and leave the vault open.'''}
    attachment = root/('Review-evidence-'+case+'.txt')
    attachment.write_text('Verified review evidence\n'+case+'\n库存已核对 🐦\n')
    return {'attachment': str(attachment), 'attachment_sha256': file_sha256(attachment),
            'prompt': f'''In the isolated Chrome window "DTW Workbench {case}", review SKU DTW-0088 and DTW-0240. Set their Target quantity to 37 and 52 respectively, mark Reviewed and set Handoff note to {json.dumps(NOTES, ensure_ascii=False)} for the corresponding SKU. Save each record exactly once. Reopen each edit form and verify all persisted values, then cancel without saving again. The table spans multiple pages; use its actual search and controls. Finally use "Attach review evidence" and the native file chooser to select "{attachment}". Verify the received filename in the page. Preserve all other records; leave the test window open.'''}


def independent_readback(kind, spec, root, case):
    if kind == 'wps':
        require(file_sha256(Path(spec['seed'])) == spec['seed_sha256'], 'Starting document changed')
        path = Path(spec['output']); require(path.is_file(), 'New DOCX was not saved')
        paragraphs = [p for p in docx_paragraphs(path) if p.strip()]
        require(paragraphs == spec['paragraphs'], 'Saved DOCX paragraphs differ')
        return {'document_sha256': file_sha256(path), 'paragraphs': paragraphs, 'seed_unchanged': True}
    if kind == 'obsidian':
        vault = Path(spec['vault'])
        require((vault/'.obsidian').is_dir(), 'Obsidian did not create the requested vault')
        notes = {}
        for relative, expected in spec['notes'].items():
            path = vault/relative
            require(path.is_file(), 'Missing note: '+relative)
            text = path.read_text()
            require(text.rstrip('\n') == expected.rstrip('\n'), 'Saved note differs: '+relative)
            notes[relative] = {'sha256': file_sha256(path), 'text': text}
        return {'vault_created': True, 'notes': notes}
    state = json.loads((root/'web/state.json').read_text())
    check_business_state(state, case)
    uploads = state.get('uploads', [])
    require(len(uploads) == 1, 'Expected exactly one file selection/upload')
    require(uploads[0]['name'] == Path(spec['attachment']).name
            and uploads[0]['sha256'] == spec['attachment_sha256'], 'Uploaded attachment differs')
    return {'records_and_save_journal_match': True, 'saves': state['saves'], 'uploads': uploads,
            'events': state['events']}


def recovery_metrics(calls, name):
    """Count reported failures and their following executions, without inferring replay."""
    from browser_workbench_probe import decode_output
    rows = []; previous_failed = False
    for index, call in enumerate(calls, 1):
        arguments = call.get('arguments') or {}
        if call.get('name') == 'call_mcp_tool' and arguments.get('ServerName') == name:
            method = arguments.get('ToolName'); arguments = arguments.get('Arguments') or {}
        elif str(call.get('name', '')).startswith('mcp__'+name+'__'):
            method = call['name'].split('__')[-1]
        else:
            continue
        if method != 'exec': continue
        row = {'agent_call': index, 'app': arguments.get('app'),
               'follows_failed_exec': previous_failed, 'execution_id': None}
        try:
            data, is_error, _ = decode_output(call.get('payload_output', call.get('output')),
                                              call.get('acp_image_scope'))
            summary = (data.get('script') or {}).get('summary') or {}
            row.update(execution_id=data.get('execution_id'), status=data.get('status'),
                       error=data.get('error'), failed_steps=summary.get('failed'),
                       recovered_incomplete_reads=summary.get('recovered_incomplete_reads', 0))
            row['failed'] = bool(is_error or data.get('error') or data.get('status') != 'completed'
                                 or summary.get('failed') or summary.get('pending')
                                 or call.get('status') not in ('completed', 'DONE'))
        except (ValueError, AssertionError, TypeError, KeyError) as error:
            row.update(failed=True, normalization_error=str(error))
        previous_failed = row['failed']; rows.append(row)
    return {'executions': rows, 'attempted_exec': len(rows),
            'failed_exec': sum(row['failed'] for row in rows),
            'successful_exec': sum(not row['failed'] for row in rows),
            'followup_exec_after_failure': sum(row['follows_failed_exec'] for row in rows),
            'retry_measurement': 'Following a failure is a review candidate, not proof of replay; inspect original source and receipt.'}


def recorded_mcp_command(root, binary):
    """Preserve native stderr even when the external Agent consumes its MCP pipe."""
    launcher = root/'mcp-stdio.py'
    launcher.write_text(
        "import os,sys\n"
        "fd=os.open(sys.argv[1],os.O_WRONLY|os.O_CREAT|os.O_APPEND,0o600)\n"
        "os.dup2(fd,2)\n"
        "os.close(fd)\n"
        "os.environ['DTW_AX_READINESS_TRACE']='1'\n"
        "os.execv(sys.argv[2],sys.argv[2:])\n")
    return [sys.executable, str(launcher), str(root/'native-stderr.log'), str(binary), 'mcp']


def run_agent(repo, root, binary, client, adapter, name, prompt, evidence):
    stage, skill = stage_agent_plugin(repo, root, binary, name)
    mcp = recorded_mcp_command(root, binary)
    evidence['mcp_stdio'] = {'command': mcp, 'launcher_sha256': file_sha256(root/'mcp-stdio.py'),
                            'capture': 'stderr only; stdin and stdout inherited unchanged'}
    env = os.environ.copy(); installed = registered = False; process = None; started = None
    try:
        if client == 'claude':
            local = root/'.claude/skills'/name; local.mkdir(parents=True)
            shutil.copytree(skill.parent, local, dirs_exist_ok=True)
            settings = Path.home()/'.claude/settings.json'
            if settings.exists():
                env.update({k: str(v) for k, v in json.loads(settings.read_text()).get('env', {}).items()})
            env['CLAUDE_CODE_EXECUTABLE'] = shutil.which('claude')
            config = root/'mcp.json'
            write_json(config, {'mcpServers': [{'name': name, 'command': mcp[0], 'args': mcp[1:], 'env': []}]})
            command = ['acpx'] + (['--agent', str(adapter)] if adapter else [])
            allowed = ['Skill', f"Read({local/'PATTERNS.md'})"]
            allowed += [f'mcp__{name}__{m}' for m in ('apps','api','exec','result')]
            command += ['--cwd', str(root), '--mcp-config', str(config), '--format', 'json',
                        '--timeout', '900', '--model', 'codex/gpt-6.1-terra', '--deny-all',
                        '--allowed-tools', ','.join(allowed),
                        '--no-fs', '--no-terminal']
            if not adapter: command.append('claude')
            command += ['exec', '--file', str(root/'prompt.txt')]
        else:
            (stage/'.mcp.json').unlink(missing_ok=True)
            with (root/'setup.log').open('w') as log:
                subprocess.run(['agy','plugin','install',str(stage)],check=True,stdout=log,stderr=log); installed=True
                subprocess.run(['agy','mcp','add',name,*mcp],check=True,stdout=log,stderr=log); registered=True
            command = ['agy','--print',prompt,'--dangerously-skip-permissions',
                       '--output-format','stream-json','--print-timeout','900s',
                       '--log-file',str(root/'agy.log')]
        started = time.monotonic()
        with (root/'agent.jsonl').open('w') as out, (root/'agent-stderr.log').open('w') as err:
            process = subprocess.Popen(command, cwd=root, env=env, stdout=out, stderr=err)
            try:
                process.wait(timeout=930)
            except subprocess.TimeoutExpired:
                evidence['agent_deadline_exceeded'] = True
    finally:
        if process is not None and process.poll() is None:
            process.terminate(); evidence['agent_timeout_cleanup'] = stop_owned(process)
        if started is not None:
            evidence['agent_wall_seconds'] = round(time.monotonic()-started, 3)
        if process is not None:
            evidence['agent_returncode'] = process.returncode
        with (root/'registration-cleanup.log').open('w') as log:
            failures = []
            for label, enabled, command in (
                ('mcp', registered, ['agy','mcp','remove',name]),
                ('plugin', installed, ['agy','plugin','uninstall',name]),
            ):
                if enabled:
                    result = subprocess.run(command, stdout=log, stderr=log, timeout=30)
                    if result.returncode: failures.append(label)
            evidence['registration_cleanup'] = {'failed': failures}
    calls, terminal = (normalize_acp if client == 'claude' else normalize_agy)(root/'agent.jsonl')
    write_json(root/'normalized-calls.json', calls)
    evidence['terminal'] = terminal
    evidence['metrics'] = metrics(calls, name, allowed_apps=(APPS[evidence['task']][0],))
    evidence['recovery'] = recovery_metrics(calls, name)
    require(not evidence.get('agent_deadline_exceeded'), 'Agent resource deadline elapsed; partial transcript retained')
    check_routes(calls, name, root)
    check_skill_loaded(calls, name, root)
    check_terminal(client, terminal, process.returncode, (root/'agent-stderr.log').read_text())
    require(not evidence['registration_cleanup']['failed'], 'Temporary Agent registration cleanup failed')


def launch_task(kind, root, spec, case, resources, evidence):
    app_id, bundle = APPS[kind]
    with (Path(bundle)/'Contents/Info.plist').open('rb') as stream:
        info = plistlib.load(stream)
    require(info['CFBundleIdentifier'] == app_id, 'Installed application identity differs')
    evidence['app_version'] = info.get('CFBundleShortVersionString')
    if kind == 'wps':
        subprocess.run(['open','-a',bundle,spec['seed']],check=True,timeout=15)
        evidence['process_ownership'] = 'Only the disposable seed and resulting document windows; never terminate WPS globally'
        return Path(spec['seed']).stem
    arguments = [str(Path(bundle)/'Contents/MacOS'/info['CFBundleExecutable'])]
    profile = root/(kind+'-profile'); profile.mkdir()
    arguments.append('--user-data-dir='+str(profile))
    if kind == 'chrome':
        server = subprocess.Popen(['python3',str(Path(__file__).parents[1]/'fixtures/browser-workbench/server.py'),
                                   '--output',str(root/'web'),'--case',case,'--file-task'],
                                  stdout=subprocess.PIPE,text=True)
        resources['server'] = server
        address = json.loads(server.stdout.readline())
        arguments += ['--no-first-run','--no-default-browser-check','--disable-background-mode','--new-window',address['url']]
    log = (root/(kind+'.log')).open('w'); resources['app_log'] = log
    child = subprocess.Popen(arguments,stdout=log,stderr=subprocess.STDOUT)
    resources['app'] = child
    evidence['process_ownership'] = {'pid':child.pid,'profile':str(profile),'kind':'exact child with unique profile'}
    return 'DTW Workbench '+case if kind == 'chrome' else 'Obsidian'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--task', choices=APPS, required=True)
    parser.add_argument('--client', choices=('claude','agy'), required=True)
    parser.add_argument('--acp-adapter', type=Path)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    root = args.output.absolute(); root.mkdir(mode=0o700)
    case = 'case-'+uuid.uuid4().hex[:10]; name = 'dtw-real-'+case
    binary, artifact = prepare_runtime(repo, root, args.dtw)
    artifact['skill_files_sha256'] = {
        str(path.relative_to(repo/'skills/desktop-world')): file_sha256(path)
        for path in sorted((repo/'skills/desktop-world').rglob('*')) if path.is_file()
    }
    app_id = APPS[args.task][0]
    evidence = {'kind':'real-application-Agent-task','task':args.task,'case':case,'client':args.client,
                **artifact,'passed':False,'business_passed':False,'clean_run':False,
                'gui_subtasks':'Inspect transcript for reopen/search/link coverage; disk readback alone is insufficient',
                'started_unix_ms':int(time.time()*1000)}
    resources = {}; supervisor = None; spec = None
    try:
        spec = task_spec(args.task, root, case); write_json(root/'task.json', spec)
        supervisor = initialize(binary, root/'supervisor')
        title = launch_task(args.task, root, spec, case, resources, evidence)
        def ready():
            record, _ = supervisor.execute(app_id,'setup-window','return await app.windows();')
            if record.get('error'): return None
            candidates = [w for w in successful(record)['windows'] if title in w['title']]
            if args.task == 'obsidian':
                coverage = successful(record).get('coverage') or {}
                require(coverage.get('complete_within_scope') is True,
                        'Incomplete app inventory cannot establish a unique welcome window')
                candidates = [w for w in candidates if w['title'] == 'Obsidian']
                native_titles = owned_window_titles(resources['app'])
                if candidates:
                    require(native_titles.count('Obsidian') == 1,
                            'Welcome window is not proven to belong to the owned child')
                    evidence['starting_window_owner_proof'] = {
                        'pid': resources['app'].pid, 'title': 'Obsidian',
                        'method': 'unique app-wide DTW title and matching exact live-child CG window'}
            require(len(candidates) <= 1, 'Disposable starting window is not uniquely identifiable')
            return candidates[0] if candidates else None
        evidence['starting_window'] = wait_until(ready, timeout=20)
        prompt = f'''Use the {name} Skill and its DTW MCP server. {spec['prompt']}
Perform GUI work only through this DTW server and the selected app. Only the disposable resources named above and their new dialogs/windows are authorized. Preserve personal documents, vaults and browser tabs. No shell, filesystem editing, DOM/CDP, HTTP or another desktop tool may perform the task. Ordinary Skill and MCP metadata discovery is allowed. Batch known actions into scripts; use fresh observation when the target changes. Do not replay an uncertain action. Report which subgoals completed, any blocker, unnecessary discovery/retry steps, and specific usability difficulties. Leave the requested final UI open; the supervisor owns cleanup.'''
        (root/'prompt.txt').write_text(prompt)
        print(json.dumps({'output':str(root),'task':args.task,'client':args.client,'case':case}),flush=True)
        run_agent(repo, root, binary, args.client, args.acp_adapter, name, prompt, evidence)
        evidence['readback'] = independent_readback(args.task, spec, root, case)
        evidence['business_passed'] = True
        evidence['clean_run'] = not evidence['metrics']['issues']
        evidence['passed'] = True
    except BaseException as error:
        evidence['failure'] = {'type':type(error).__name__,'message':str(error)}
        (root/'failure.txt').write_text(traceback.format_exc())
        # Preserve independent partial effects even when the model or route failed.
        if spec is not None:
            try: evidence['partial_readback'] = independent_readback(args.task, spec, root, case)
            except Exception as read_error: evidence['readback_failure'] = str(read_error)
    finally:
        steps = []
        if args.task == 'wps' and supervisor is not None and evidence['business_passed']:
            def close_documents():
                return [close_visible_owned_document(supervisor, app_id, path)
                        for path in (spec['output'], spec['seed'])]
            steps.append(('owned-WPS-documents', close_documents, None))
        if supervisor is not None: steps.append(('supervisor',supervisor.close,process_cleaned))
        if 'app' in resources:
            def close_app():
                child = resources['app']
                if child.poll() is None: child.terminate()
                result = stop_owned(child)
                require(not result['forced'] and result['returncode'] in (0, -15),
                        'Owned app did not stop normally after requested termination')
                return result
            steps.append(('owned-app-process',close_app,None))
        if 'server' in resources:
            def close_server():
                (root/'web/stop').write_text('stop'); return stop_owned(resources['server'])
            steps.append(('owned-server',close_server,process_cleaned))
        cleanup_steps(evidence,steps)
        if evidence['business_passed']:
            try:
                evidence['readback_after_cleanup'] = independent_readback(args.task, spec, root, case)
            except Exception as error:
                evidence['passed'] = False
                evidence['readback_after_cleanup_failure'] = str(error)
        if 'app_log' in resources: resources['app_log'].close()
        leave_gui_review_pending(evidence)
        write_json(root/'evidence.json',evidence)
        print(json.dumps({'evidence':str(root/'evidence.json'),'acceptance_status':evidence['acceptance_status'],
                          'automatic_checks_passed':evidence['automatic_checks_passed'],'clean_run':evidence['clean_run'],
                          'business_passed':evidence['business_passed'],'failure':evidence.get('failure')},ensure_ascii=False),flush=True)
    return 0 if evidence['automatic_checks_passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
