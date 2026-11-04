"""Current apps/exec boundary acceptance; compose with computer_use_benchmark.CoreCases.

These cases never replay an interrupted input. Missing native test conditions are
reported explicitly, rather than inferred from older Session/Scene probes.
"""
import json
from pathlib import Path
import subprocess
import sys
import time

from app_client import AppClient, require
from standard_agent_probe import process_table, wait_until


def gesture_counts(rows):
    """Count the fixture's real left down, intermediate drag and paired up events."""
    return {key: sum(row.get('phase') in phases for row in rows) for key, phases in (
        ('down', ('click', 'double_click')), ('drag', ('drag',)), ('up', ('up',)))}


def owned_workers(table, parent):
    return {pid: command for pid, (ppid, command) in table.items()
            if ppid == parent and any(flag in command for flag in
                ('--dtw-native-worker', '--dtw-scene-worker'))}


class BoundaryCases:
    def _boundary_native(self):
        self.run("await app.click('Native controls');")
        wait_until(lambda: self.state()['mode'] == 'native')
        require(self.state()['pressed_mouse_buttons'] == 0, 'Pre-existing held mouse input')

    def _begin_held_drag(self, name):
        self._boundary_native()
        view = self.capture(name)
        start = self.anchor('gesture-desk', view)
        # 64 supplied points, 63 unit image-pixel segments: safely below the
        # provider's 256-sample limit even on scaled captures. Never return home.
        points = [{'x': start['x'] + index, 'y': start['y']} for index in range(64)]
        image = view['image']
        require(points[-1]['x'] < image['width'], view)
        left, top, width, height = image['bounds']
        self._drag_terminal = [left + points[-1]['x'] * width / image['width'],
                               top + points[-1]['y'] * height / image['height']]
        self._drag_full_samples = len(points) - 1
        offset = len(self.events('gesture'))
        request = self.client.begin_execute(self.app, 'await app.drag({path:' + json.dumps(points) + '});')
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            rows = self.events('gesture')[offset:]
            counts = gesture_counts(rows)
            require(counts['up'] == 0, {'reason': 'Drag completed before interruption; no cancellation coverage', 'events': rows})
            if counts['down'] == 1 and counts['drag'] > 0:
                return request, offset, counts
            time.sleep(.002)
        raise AssertionError('No actual held drag observed; interrupted input was not replayed')

    def _partial_drag(self, offset):
        rows = self.events('gesture')[offset:]
        counts = gesture_counts(rows)
        require(counts['down'] == counts['up'] == 1 and 0 < counts['drag'] < self._drag_full_samples,
                {'reason': 'No partial-path interruption: complete or unpaired drag', 'counts': counts})
        terminal = self._drag_terminal
        observed = [row for row in rows if row.get('phase') in ('drag', 'up')]
        for row in observed:
            require(isinstance(row.get('screen_x'), (int, float))
                    and isinstance(row.get('screen_y'), (int, float)),
                    {'reason': 'Independent gesture coordinates unavailable', 'event': row})
            # One screen-point tolerance covers native event coordinate rounding.
            require(abs(row['screen_x'] - terminal[0]) > 1 or abs(row['screen_y'] - terminal[1]) > 1,
                    {'reason': 'Terminal sentinel reached; natural completion cannot prove interruption', 'event': row})
        return {'counts': counts, 'terminal_screen': terminal, 'full_path_samples': self._drag_full_samples,
                'terminal_reached': False, 'gesture_events': observed}

    def _cleanup_read(self, transport, request, mode):
        deadline = time.monotonic() + 5
        while True:
            started = time.time() * 1000
            read_request = self.client.transport.serial + 1
            try:
                recovery = self.run('const v=await app.look(); return {elements:v.elements,semantics:v.semantics};')
                break
            except AssertionError as error:
                refused = error.args[0] if error.args else None
                if (not isinstance(refused, dict) or refused.get('error', {}).get('code') != 'busy'
                        or time.monotonic() >= deadline):
                    raise
                # Only a new read is polled; the interrupted input is never resent.
                time.sleep(.02)
        ended = time.time() * 1000
        require(recovery.get('status') == 'completed', recovery)
        transcript = Path(transport.output) / 'mcp.jsonl'
        frames = [json.loads(line) for line in transcript.read_text(encoding='utf-8').splitlines()]
        response = next((row for row in frames if row['direction'] == 'response'
                         and row['message'].get('id') == request), None)
        boundary = {'mode': mode, 'transcript': str(transcript), 'request_id': request,
                    'async_response': {'outcome': 'response', 'frame': response} if response else
                        {'outcome': 'no-response', 'observed_until_unix_ms': ended},
                    'cleanup_read': {'transcript': str(Path(self.client.transport.output) / 'mcp.jsonl'),
                        'request_id': read_request, 'start_unix_ms': started, 'end_unix_ms': ended,
                        'execution_id': recovery.get('execution_id'), 'status': recovery['status']}}
        if response:
            envelope = response['message'].get('result', {})
            data = envelope.get('structuredContent', {})
            effects = data.get('effects', {})
            error_code = data.get('failure', {}).get('code') or data.get('error', {}).get('code')
            uncertain = any(group.get('dispatch', {}).get('unknown') or group.get('interrupted')
                            for group in (effects, effects.get('focus_preparation', {})))
            is_error = bool(envelope.get('isError') or response['message'].get('error')
                            or error_code or uncertain or data.get('status') != 'completed')
            execution = data.get('execution_id')
        else:
            effects = {'dispatch': {'unknown': 1}, 'interrupted': 1}
            error_code, is_error, execution = 'async_outcome_unknown', True, None
        # A transport-specific metric identity for an unreceipted request is not
        # a fabricated provider execution receipt. Keep that distinction explicit.
        metric_id = execution or 'async-request:' + str(transcript) + ':' + str(request)
        metric = {'tool': 'exec', 'execution_id': metric_id, 'provider_execution_id': execution,
                  'async_request_id': request, 'async_transcript': str(transcript),
                  'is_error': is_error, 'error_code': error_code, 'effects': effects,
                  'seconds': 0, 'source_bytes': 0, 'json_bytes': len(json.dumps(boundary)),
                  'image_count': 0}
        if not any(row.get('execution_id') == metric_id for row in self.client.metrics):
            self.client.metrics.append(metric)
        boundary['async_metric'] = metric
        with (self.output / 'boundary-interruptions.jsonl').open('a', encoding='utf-8') as stream:
            stream.write(json.dumps(boundary) + '\n')
        return recovery, boundary

    def R01(self):
        request, offset, held = self._begin_held_drag('cancel-held-drag')
        require(gesture_counts(self.events('gesture')[offset:])['up'] == 0,
                'Drag already released before cancellation notification')
        self.client.cancel(request)
        wait_until(lambda: gesture_counts(self.events('gesture')[offset:])['up'] > 0)
        # Ordinary fresh read, with a new request ID; never await/replay the input.
        recovery, boundary = self._cleanup_read(self.client.transport, request, 'cancel')
        partial = self._partial_drag(offset)
        counts = partial['counts']
        require(counts['drag'] >= held['drag'], counts)
        require(self.state()['pressed_mouse_buttons'] == 0, self.state())
        require(recovery.get('status') == 'completed', recovery)
        return {'request_id': request, 'held_at_cancel': held, 'events': counts,
                'pressed_mouse_buttons': 0, 'recovery': recovery, 'input_replayed': False,
                'partial_path': partial, 'interruption_boundary': boundary}

    def R02(self):
        request, offset, held = self._begin_held_drag('eof-held-drag')
        old = self.client
        child = old.transport.child
        binary = Path(child.args[0])
        workers = owned_workers(process_table(), child.pid)
        require(workers, 'No exact owned native worker observed during active drag')
        require(gesture_counts(self.events('gesture')[offset:])['up'] == 0,
                'Drag already released before EOF; active-gesture cleanup unproven')
        # close() sends EOF first, then waits/reaps and retains forced cleanup.
        cleanup = old.close()
        require(cleanup.get('returncode') == 0 and not cleanup.get('forced')
                and cleanup.get('native_workers_reaped') is True, cleanup)
        def workers_reaped():
            current = process_table()
            return all(current.get(pid, (None, None))[1] != command
                       for pid, command in workers.items())
        wait_until(workers_reaped)
        require(child.poll() is not None, 'Exact MCP process survived EOF')
        wait_until(lambda: self.state()['pressed_mouse_buttons'] == 0)
        # Runner cleanup owns cases.client, including this replacement connection.
        self.client = AppClient(binary, self.output / 'mcp-after-eof')
        self.client.metrics[:0] = old.metrics
        recovery, boundary = self._cleanup_read(old.transport, request, 'eof')
        partial = self._partial_drag(offset)
        counts = partial['counts']
        require(self.fixture.child.poll() is None, 'EOF unexpectedly terminated the fixture')
        return {'request_id': request, 'held_at_eof': held, 'events': counts,
                'mcp_pid': child.pid, 'workers': workers, 'cleanup': cleanup,
                'fixture_alive': True, 'recovery': recovery, 'input_replayed': False,
                'partial_path': partial, 'interruption_boundary': boundary}

    def R03(self):
        self._boundary_native()
        view = self.capture('before-sibling-instance')
        point = self.anchor('gesture-desk', view)
        target = self.state()
        peer = type(self.fixture)(self.output / 'boundary-sibling', self.fixture.case + '-sibling')
        proof = {'same_bundle': str(self.fixture.bundle), 'original_pid': target['pid']}
        try:
            peer.prepare(build=False)
            config_path = peer.output / 'config.json'
            config = json.loads(config_path.read_text(encoding='utf-8'))
            bounds = target['windows'][0]['bounds']
            config.update(window_x=bounds[0], window_y=bounds[1], activate=False)
            config_path.write_text(json.dumps(config), encoding='utf-8')
            peer.log = (peer.output / 'native.log').open('w')
            peer.child = subprocess.Popen([str(self.fixture.executable), '--fixture-root', str(peer.output)],
                                          stdout=peer.log, stderr=subprocess.STDOUT)
            sibling = wait_until(lambda: s if (s := peer.state()) and s.get('ready') else None, timeout=15)
            require(sibling['pid'] != target['pid'], sibling)
            require(all(abs(a - b) <= 1 for a, b in zip(sibling['windows'][0]['bounds'], bounds)),
                    {'reason': 'Sibling overlap not established', 'original': bounds, 'sibling': sibling['windows']})
            require(sibling['anchors']['gesture-desk'] == target['anchors']['gesture-desk'], sibling)
            before = peer.state()['native']
            # No intervening DTW read: the old canvas still belongs to the original instance.
            data, response = self.client.execute(self.app, 'await app.click(' + json.dumps(point) + ');', allow_error=True)
            code = data.get('failure', {}).get('code') or data.get('error', {}).get('code')
            require(response.get('isError') is True and code in ('stale_scene', 'look_required'), data)
            keys = ('counter', 'note', 'saved_note', 'saves', 'audit_count', 'slider', 'reviewed', 'selection')
            require(all(self.native()[key] == target['native'][key] for key in keys), self.native())
            require(all(peer.state()['native'][key] == before[key] for key in keys), peer.state())
            for initial, final in ((before, peer.state()['native']), (target['native'], self.native())):
                require(all(final['gestures'].get(phase, 0) == initial['gestures'].get(phase, 0)
                            for phase in ('click', 'double_click', 'right_click', 'drag', 'up')), final)
            require(isinstance(data.get('execution_id'), str), data)
            proof.update(sibling_pid=sibling['pid'], observation=data, both_business_states_unchanged=True)
            if code == 'look_required' and not data.get('effects'):
                proof.update(unavailable='Public look_required rejection lacks dispatch/focus-preparation evidence; '
                             'unchanged independent business states do not prove the not_dispatched contract.',
                             sibling_rejection_verified=False)
                return proof
            require(code == 'stale_scene', data)
            dispatch = data.get('effects', {}).get('dispatch', {})
            require(dispatch.get('not_dispatched') == 1 and not dispatch.get('dispatched')
                    and not dispatch.get('unknown'), data)
            preparation = data.get('effects', {}).get('focus_preparation', {})
            require(not preparation.get('attempted') and not preparation.get('interrupted')
                    and not any(preparation.get('dispatch', {}).values()), data)
            proof.update(expected_tool_errors=[data['execution_id']],
                         sibling_rejected=True, both_business_states_unchanged=True,
                         unavailable='Same-bundle sibling refusal verified; foreign-occlusion subtarget remains '
                         'unverified. Supported directed input intentionally ignores foreign occlusion.')
            return proof
        except BaseException as error:
            proof['failure'] = {'type': type(error).__name__, 'message': str(error)}
            raise
        finally:
            proof['cleanup'] = peer.close()
            (self.output / 'boundary-sibling-report.json').write_text(json.dumps(proof, indent=2) + '\n', encoding='utf-8')
            if 'failure' not in proof:
                require(proof['cleanup']['passed'], proof['cleanup'])

    def R04(self):
        from computer_use_focus import run_background_contract
        if self.focus_observer is None:
            return {'blocked': 'Continuous independent focus observer is required for background interference'}
        self._boundary_native()
        proof = run_background_contract(self.fixture, Path(self.client.transport.child.args[0]),
            self.focus_observer, self.output / 'boundary-background',
            case=self.fixture.case + '-boundary', input_policy='auto')
        require(proof['passed'], proof)
        return {'contract': proof, 'cursor_audit': 'owned by outer acceptance runner',
                'scope': 'Supported directed click, keyboard and readonly operations; continuous focus and witness key destination'}

    def S01(self):
        if sys.platform != 'darwin':
            return {'blocked': 'Dock app-root acceptance requires macOS'}
        discovered, _ = self.client.tool('apps', {'query': 'Dock', 'state': 'running', 'limit': 100})
        docks = [row for row in discovered.get('apps', []) if row.get('app') == 'com.apple.dock']
        if len(docks) != 1:
            return {'unavailable': 'Current apps discovery did not expose one exact authorized Dock identity',
                    'discovery': discovered}
        data, response = self.client.execute(docks[0]['app'], 'await app.look();', allow_error=True)
        code = data.get('failure', {}).get('code') or data.get('error', {}).get('code')
        if response.get('isError') and code == 'no_window':
            require(isinstance(data.get('execution_id'), str), data)
            return {'unavailable': 'Authorized Dock app-root observation returns no_window', 'observation': data,
                    'expected_tool_errors': [data['execution_id']]}
        require(not response.get('isError'), data)
        return {'blocked': 'Dock observation alone does not prove an exact authorized app-root item action; '
                'current fixture has no Dock-item action oracle', 'observation': data}

    def S02(self):
        state = self.state()
        if state.get('mode') != 'accessory' or not state.get('accessory', {}).get('status_item'):
            return {'blocked': 'S02 requires runner-provided owned windowless accessory fixture'}
        require(state['windows'] == [] and state['accessory']['window_count'] == 0, state)
        before = state['accessory']['counter']
        data, response = self.client.execute(self.app, 'await app.look();', allow_error=True)
        require(self.state()['accessory']['counter'] == before, self.state())
        code = data.get('failure', {}).get('code') or data.get('error', {}).get('code')
        if response.get('isError') and code == 'no_window':
            require(isinstance(data.get('execution_id'), str), data)
            return {'unavailable': 'Owned windowless accessory app-root observation returns no_window',
                    'observation': data, 'pid': state['pid'], 'counter_unchanged': True,
                    'expected_tool_errors': [data['execution_id']]}
        require(not response.get('isError'), data)
        return {'blocked': 'Windowless accessory observation succeeded, but exact app-root menu action '
                'and ownership have not been verified', 'observation': data}

    def S03(self):
        return {'blocked': 'Windows-only chooser tooltip acceptance requires a native Windows host '
                'and real tooltip/shadow plus selected-file oracle', 'host_platform': sys.platform}

    def S04(self):
        if sys.platform != 'darwin':
            return {'blocked': 'Native display/desktop acceptance facility is not available on this host',
                    'host_platform': sys.platform}
        result = subprocess.run(['system_profiler', 'SPDisplaysDataType', '-json'],
                                capture_output=True, text=True, timeout=30, check=False)
        inventory = {'returncode': result.returncode, 'stderr': result.stderr}
        if result.returncode == 0:
            inventory['data'] = json.loads(result.stdout)
        path = self.output / 'boundary-displays.json'
        path.write_text(json.dumps(inventory, indent=2) + '\n', encoding='utf-8')
        return {'blocked': 'Actual display inventory does not establish mixed-DPI placement/input or '
                'unavailable-desktop refusal. The fixture has no acknowledged display-relocation or '
                'isolated unavailable-desktop facility; changing the user session is outside scope.',
                'actual_display_inventory': inventory, 'artifact': path.name}
