#!/usr/bin/env python3
"""Owned AppKit right-click sequences over current apps/exec; no Telegram access."""
import argparse
import json
import os
from pathlib import Path
import plistlib
import subprocess
import uuid

from app_client import AppClient, artifact_identity, require
from standard_agent_probe import wait_until, write_json


def cleanup_fixture(out, executable, read_state, record):
    # This owned command must also survive delayed startup/state publication.
    (out / 'command').write_text('quit')
    record['quit_requested'] = True
    observed = set()
    failures = []

    def owned_pids():
        rows = subprocess.run(['ps', '-axww', '-o', 'pid=,comm='], check=True,
            capture_output=True, text=True).stdout.splitlines()
        pids = {int(fields[0]) for row in rows
            if len(fields := row.strip().split(None, 1)) == 2
            and fields[1] == str(executable)}
        observed.update(pids)
        record['pids'] = sorted(observed)
        return pids

    if read_state() is None and not owned_pids():
        try:
            wait_until(lambda: read_state() or owned_pids(), timeout=2)
        except AssertionError:
            pass  # No state or exact owned process was observed in this interval.
    if owned_pids():
        try:
            wait_until(lambda: not owned_pids())
            record['status'] = 'exited_after_quit'
        except AssertionError:
            failures.append('Fixture did not exit through its quit command')
            for pid in owned_pids():
                record['forced'].append({'pid': pid, 'signal': 'SIGTERM'})
                try:
                    os.kill(pid, 15)
                except ProcessLookupError:
                    pass
            wait_until(lambda: not owned_pids())
            record['status'] = 'exited_after_forced_cleanup'
    else:
        record['status'] = 'not_running_after_quit'
    return failures


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    out.chmod(0o700)
    bundle = out / 'Scene.app'
    executable = bundle / 'Contents/MacOS/Scene'
    executable.parent.mkdir(parents=True)
    identifier = 'dev.caelis.dtw.sequence.' + uuid.uuid4().hex
    with (bundle / 'Contents/Info.plist').open('wb') as handle:
        plistlib.dump({'CFBundleExecutable': 'Scene', 'CFBundleIdentifier': identifier,
            'CFBundleName': 'DTW Sequence', 'CFBundlePackageType': 'APPL',
            'NSPrincipalClass': 'NSApplication'}, handle)
    source = Path(__file__).resolve().parents[1] / 'fixtures/app-scene/main.swift'
    subprocess.run(['xcrun', 'swiftc', '-module-cache-path', str(out / 'swift-cache'),
        str(source), '-o', str(executable)], check=True, capture_output=True, timeout=90)
    report = {**artifact_identity(args.dtw), 'cases': [], 'limits': [
        'Owned AppKit fixture only; no Telegram or app-family coverage.',
        'CGEvent pointer and NSWorkspace frontmost samples are observations; '
        'before/after samples do not exclude transient movement or identify its source.']}
    client = None
    lazy_client = None

    def state():
        try:
            return json.loads((out / 'state.json').read_text())
        except (FileNotFoundError, json.JSONDecodeError):
            return None

    try:
        client = AppClient(args.dtw.resolve(), out / 'mcp')
        client.capture(bundle, 'initial')
        baseline = wait_until(state)
        apps, response = client.tool('apps', {'query': identifier, 'state': 'running'})
        require(not response.get('isError') and apps['apps'], apps)
        report['discovery'] = apps
        pid = baseline['pid']
        message = baseline['buttons']['Sequence message']
        base_geometry = next(row['bounds'] for row in baseline['windows']
            if row['title'] == 'DTW Scene main')
        before = state()
        lazy_client = AppClient(args.dtw.resolve(), out / 'lazy-mcp')
        cold_source = "await app.click({selector:{role:'button',text:'peer'}});"
        data, response = lazy_client.execute(bundle, cold_source)
        after = wait_until(lambda: current if (current := state())
            and current['counts']['peer'] == before['counts']['peer'] + 1 else None)
        require(after['pid'] == pid and after['sequence'] == before['sequence'], after)
        require(after['counts']['main'] == before['counts']['main'], after)
        report['lazy_cleanup'] = lazy_client.close()
        lazy_client = None
        require(not report['lazy_cleanup'].get('forced') and report['lazy_cleanup'].get('returncode') == 0,
            report['lazy_cleanup'])
        report['cases'].append({'mode':'cold_known_selector','passed':True,'source':cold_source,
            'before_environment':before['environment'],'after_environment':after['environment'],
            'before_counts':before['counts'],'after_counts':after['counts'],'execution':data,
            'mcp_image_count':sum(item['type'] == 'image' for item in response['content'])})
        for mode in ['selector', 'pixel']:
            before = state()
            require(before['pid'] == pid, 'Owned fixture instance changed')
            require(not any(row['title'] == 'DTW Sequence popup' for row in before['windows']), before)
            # The same script captures, opens a new native surface and acts on
            # it. No intermediate look/capture authorizes the new popup.
            prefix = ('const v=await app.capture({full:true}); '
                'const b=v.image.bounds; const s=' + json.dumps(message) + '; '
                'const p={x:(s[0]-b[0])*v.image.width/b[2],'
                'y:(s[1]-b[1])*v.image.height/b[3]}; '
                "await app.click({...p,button:'right'}); ")
            action = ("await app.click({selector:{text:'Sequence action'}}); "
                if mode == 'selector' else 'await app.click(p); ')
            script = prefix + action + 'await app.click(p);'
            data, response = client.execute(bundle, script)
            expected = {key: before['sequence'][key] + 1
                for key in ['right', 'opened', 'action', 'base']}
            after = wait_until(lambda: current if (current := state())
                and current['sequence'] == expected else None)
            require(after['pid'] == pid, after)
            require(not any(row['title'] == 'DTW Sequence popup' for row in after['windows']), after)
            require(next(row['bounds'] for row in after['windows']
                if row['title'] == 'DTW Scene main') == base_geometry, after)
            require(after['counts'] == before['counts'], 'Unrelated fixture button received input')
            require(after['pressed_mouse_buttons'] == 0, after)
            report['cases'].append({'mode': mode, 'passed': True, 'source': script,
                'before_effects': before['sequence'], 'after_effects': after['sequence'],
                'before_environment': before['environment'], 'after_environment': after['environment'],
                'pointer_events': after['pointer_events'][len(before['pointer_events']):],
                'fixture_windows_after': after['windows'], 'execution': data,
                'mcp_image_count': sum(item['type'] == 'image' for item in response['content'])})
            write_json(out / 'report.json', report)
        before = state()
        data, _ = client.execute(bundle, "await app.look({scope:'menu',text:'File',full:true});")
        require(isinstance(data.get('view', {}).get('elements'), list)
            and data['view']['elements'], data)
        after = wait_until(lambda: current if (current := state())
            and current['environment']['uptime'] > before['environment']['uptime'] else None)
        require(after['sequence'] == before['sequence'] and after['counts'] == before['counts'], after)
        require(after['environment']['frontmost_pid'] == before['environment']['frontmost_pid'], after)
        require(after['key'] == before['key'] and after['active'] == before['active'], after)
        report['cases'].append({'mode': 'readonly_menu', 'passed': True,
            'before_environment': before['environment'], 'after_environment': after['environment'],
            'elements': data['view']['elements'], 'effects_unchanged': True})
        report['passed'] = True
    except BaseException as error:
        report.update(passed=False, failure=str(error))
        raise
    finally:
        cleanup_failures = []
        current = state()
        report['final_fixture_state'] = current
        report['fixture_cleanup'] = {'quit_requested': False, 'pids': [], 'forced': []}
        try:
            cleanup_failures.extend(cleanup_fixture(out, executable, state, report['fixture_cleanup']))
        except BaseException as error:
            report['fixture_cleanup']['status'] = 'cleanup_failed'
            cleanup_failures.append('Fixture cleanup: ' + str(error))
        try:
            if lazy_client:
                report['lazy_cleanup'] = lazy_client.close()
                if report['lazy_cleanup'].get('forced') or report['lazy_cleanup'].get('returncode', 0) != 0:
                    cleanup_failures.append('Cold-context MCP required forced cleanup or exited nonzero')
        except BaseException as error:
            cleanup_failures.append('Cold-context MCP cleanup: ' + str(error))
        try:
            if client:
                report['cleanup'] = client.close()
                if report['cleanup'].get('forced') or report['cleanup'].get('returncode', 0) != 0:
                    cleanup_failures.append('MCP transport required forced cleanup or exited nonzero')
        except BaseException as error:
            cleanup_failures.append('MCP cleanup: ' + str(error))
        if cleanup_failures:
            report['passed'] = False
            report['cleanup_failures'] = cleanup_failures
            report.setdefault('failure', 'Cleanup failed')
        write_json(out / 'report.json', report)
    print(json.dumps({'passed': report['passed'], 'report': str(out / 'report.json')}))
    if not report['passed']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
