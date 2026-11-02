#!/usr/bin/env python3
"""macOS lifecycle and LSUIElement acceptance over apps/exec, using owned apps."""
import argparse
import json
import os
from pathlib import Path
import plistlib
import shutil
import signal
import subprocess
import uuid

from standard_agent_probe import wait_until, write_json
from app_client import AppClient, METHODS, artifact_identity, require


def prepare(root, source, bundle_id, *, ui_element=False, compiled=None):
    bundle = root / "Lifecycle.app"
    executable = bundle / "Contents/MacOS/Lifecycle"
    executable.parent.mkdir(parents=True)
    metadata = {"CFBundleExecutable": executable.name, "CFBundleIdentifier": bundle_id,
                "CFBundleName": "DTW Lifecycle Fixture", "CFBundlePackageType": "APPL",
                "CFBundleVersion": "1", "LSUIElement": ui_element}
    with (bundle / "Contents/Info.plist").open("wb") as stream:
        plistlib.dump(metadata, stream)
    if compiled:
        shutil.copy2(compiled, executable)
    else:
        with (root / "build.log").open("w") as log:
            subprocess.run(["xcrun", "swiftc", "-module-cache-path", str(root / "swift-cache"),
                            str(source), "-o", str(executable)], stdout=log,
                           stderr=subprocess.STDOUT, check=True, timeout=90)
    return bundle, executable


def fixture_state(root):
    try:
        return json.loads((root / "state.json").read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        return None


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


def owned_cleanup(root, executable):
    """A last-resort signal requires this run's unique executable path to match."""
    state = fixture_state(root)
    if not state or not alive(state["pid"]):
        return {"forced": False}
    pid = state["pid"]
    command = subprocess.run(["ps", "-p", str(pid), "-o", "comm="],
                             text=True, capture_output=True, check=False).stdout.strip()
    require(command == str(executable), "cleanup refuses a PID whose executable changed")
    os.kill(pid, signal.SIGTERM)
    wait_until(lambda: not alive(pid), timeout=5)
    return {"forced": True, "pid": pid}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--installed-package', type=Path)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    output.chmod(0o700)
    source = Path(__file__).resolve().parents[1] / 'fixtures/app-lifecycle/main.swift'
    key = uuid.uuid4().hex
    regular = prepare(output / 'regular', source, 'dev.caelis.dtw.lifecycle.' + key)
    agent_id, helper_id = 'dev.caelis.dtw.agent.' + key, 'dev.caelis.dtw.helper.' + key
    agent = prepare(output / 'agent', source, agent_id, ui_element=True, compiled=regular[1])
    helper_root = output / 'Outer.app/Contents/Frameworks'
    helper = prepare(helper_root, source, helper_id, ui_element=True, compiled=regular[1])
    fixtures = [(output / 'regular', regular), (output / 'agent', agent), (helper_root, helper)]
    identity = artifact_identity(args.dtw, args.installed_package)
    report = {'binary_sha256': identity['binary_sha256'],
        'artifact': identity, 'cases': [], 'cleanup': [], 'passed': False,
        'scope': 'Owned regular/accessory applications; no real-account or update operation'}
    client, helper_child = None, None

    def record(name, **details):
        report['cases'].append({'name': name, 'passed': True, **details})
        write_json(output / 'report.json', report)

    def command(root, value):
        (root / 'command').write_text(value)
        wait_until(lambda: fixture_state(root) and fixture_state(root)['command'] == value)

    try:
        client = AppClient(args.dtw.resolve(), output / 'protocol')
        rejected, response = client.tool('exec', {'source': 'return 1;'})
        require(response.get('isError') and rejected['error']['code'] == 'invalid_arguments', rejected)
        initial, _ = client.execute(regular[0], "await app.look();return {methods:Object.keys(app),session:typeof session,state:typeof state};")
        require(initial['result'] == {'methods': METHODS, 'session': 'undefined', 'state': 'undefined'}, initial)
        require(initial['view'].get('reset') and fixture_state(output / 'regular'), initial)
        record('required_app_and_automatic_start')
        client.execute(regular[0], "await app.click({selector:{role:'button',text:'Increment'}});")
        wait_until(lambda: fixture_state(output / 'regular')['clicks'] == 1)
        client.capture(regular[0], 'regular-stack')
        record('regular_semantic_input_and_stacked_capture')

        command(output / 'regular', 'block_quit')
        client.execute(regular[0], 'await app.quit();')
        wait_until(lambda: fixture_state(output / 'regular')['quit_requests'] == 1)
        pending = client.capture(regular[0], 'quit-prompt')
        require(any(e.get('name') == 'Keep Open' for e in pending['view']['elements']), pending)
        uncertain, response = client.execute(regular[0], "await app.click({selector:{role:'button',text:'Keep Open'}});", allow_error=True)
        if response.get('isError'):
            require(uncertain.get('effects', {}).get('dispatch', {}).get('unknown') == 1, uncertain)
        wait_until(lambda: fixture_state(output / 'regular')['cancelled_quits'] == 1)
        client.execute(regular[0], 'await app.look();')
        require(alive(fixture_state(output / 'regular')['pid']), 'cancelled quit closed app')
        record('normal_quit_prompt_cancel_and_fresh_read')
        command(output / 'regular', 'allow_quit')
        old_pid = fixture_state(output / 'regular')['pid']
        client.execute(regular[0], 'await app.quit();')
        wait_until(lambda: not alive(old_pid))
        restarted, _ = client.execute(regular[0], 'await app.look();')
        require(restarted['view'].get('reset') and fixture_state(output / 'regular')['pid'] != old_pid, restarted)
        client.execute(regular[0], 'await app.quit();')
        wait_until(lambda: not alive(fixture_state(output / 'regular')['pid']))
        record('normal_exit_and_restart_resets_baseline')

        client.execute(agent[0], 'await app.look();')
        apps, response = client.tool('apps', {'query': agent_id, 'state': 'running'})
        require(not response.get('isError') and len(apps['apps']) == 1, apps)
        authorization = apps['apps'][0]['app']
        client.execute(authorization, "await app.look();await app.click({selector:{role:'button',text:'Increment'}});")
        wait_until(lambda: fixture_state(output / 'agent')['clicks'] == 1)
        client.capture(authorization, 'accessory-app')
        record('lsuielement_discover_look_input_readback', app=authorization)
        helper_child = subprocess.Popen([str(helper[1])], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        wait_until(lambda: fixture_state(helper_root))
        helper_apps, response = client.tool('apps', {'query': helper_id, 'state': 'running'})
        require(not response.get('isError') and not helper_apps['apps'], helper_apps)
        record('nested_helper_remains_excluded')
        pending = client.begin_execute(authorization,
            "await app.click({selector:{role:'button',text:'Increment'}});while(true){}")
        wait_until(lambda: fixture_state(output / 'agent')['clicks'] == 2)
        client.cancel(pending)
        def ordinary_read_after_cancel():
            observed, response = client.execute(authorization, 'await app.look({full:true});', allow_error=True)
            if observed.get('error', {}).get('code') == 'busy':
                return None
            require(not response.get('isError'), observed)
            return observed
        recovered = wait_until(ordinary_read_after_cancel, timeout=8)
        require(recovered['view'].get('reset') is True, recovered)
        require(fixture_state(output / 'agent')['clicks'] == 2, 'input was replayed during recovery')
        record('cancelled_execution_ordinary_look_recovers_without_result_query')
        client.execute(authorization, 'await app.quit();')
        wait_until(lambda: not alive(fixture_state(output / 'agent')['pid']))
        report['passed'] = True
    except BaseException as error:
        report['error'] = type(error).__name__ + ': ' + str(error)
        raise
    finally:
        cleanup_errors = []
        try:
            require(artifact_identity(args.dtw, args.installed_package) == identity, 'binary or installed package changed during acceptance')
        except BaseException as error:
            cleanup_errors.append(str(error))
        if client:
            try:
                report['mcp_cleanup'] = client.close()
                report['protocol_metrics'] = client.metrics
            except BaseException as error:
                cleanup_errors.append(str(error))
        if helper_child:
            if helper_child.poll() is None:
                helper_child.terminate()
            try:
                helper_child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                cleanup_errors.append('owned helper did not terminate')
        for root, (_, executable) in fixtures:
            try:
                result = owned_cleanup(root, executable)
                report['cleanup'].append(result)
                if result.get('forced') and root != helper_root:
                    cleanup_errors.append('owned application required forced cleanup: ' + str(root))
            except BaseException as error:
                cleanup_errors.append(str(error))
        if cleanup_errors:
            report['passed'] = False
            report['cleanup_errors'] = cleanup_errors
        write_json(output / 'report.json', report)
        if cleanup_errors:
            raise AssertionError(cleanup_errors)


if __name__ == '__main__':
    main()
