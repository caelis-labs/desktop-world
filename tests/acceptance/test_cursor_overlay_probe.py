#!/usr/bin/env python3
"""Pure acceptance-oracle controls: no child processes, windows or input."""
import copy
import json
import tempfile
from pathlib import Path
from types import SimpleNamespace
import unittest

import cursor_overlay_probe as probe


class IdentityControls(unittest.TestCase):
    def reply(self):
        return {'nonce': 'expected', 'identity': {'pid': 17, 'birth': 200, 'windows': [23]}}

    def test_exact_identity(self):
        self.assertEqual(probe.validate_identity(self.reply(), 'expected', 17), self.reply()['identity'])

    def test_wrong_nonce_and_pid(self):
        for nonce, pid in [('other', 17), ('expected', 18)]:
            with self.assertRaises(AssertionError): probe.validate_identity(self.reply(), nonce, pid)

    def test_invalid_birth_and_window(self):
        for key, values in [('pid', [True, -1]), ('birth', [True, 0, -1]), ('windows', [[], [True], [0], [23, 24]])]:
            for value in values:
                reply = self.reply(); reply['identity'][key] = value
                with self.assertRaises(AssertionError): probe.validate_identity(reply, 'expected', 17)

    def test_session_comes_from_authenticated_reply_not_instance_directory(self):
        self.assertEqual(probe.logical_session({'session': 'a'*32}), 'a'*32)
        for value in [None, '', 'a'*31, 'g'*32, 'a'*31+'/', True]:
            with self.assertRaises(AssertionError): probe.logical_session({'session': value})

    def test_only_explicit_dead_helper_replacement_can_change_identity(self):
        old = {'session': 'a'*32, 'pid': 17, 'birth': 200, 'windows': [23], 'instance': 'b'*32}
        new = {'session': 'a'*32, 'pid': 18, 'birth': 201, 'windows': [24], 'instance': 'c'*32}
        probe.require_helper_continuity(old, old)
        with self.assertRaises(AssertionError): probe.require_helper_continuity(old, new)
        probe.require_helper_continuity(old, new, old)
        for key in ['session', 'pid', 'windows', 'instance']:
            bad = dict(new); bad[key] = 'd'*32 if key == 'session' else old[key]
            with self.assertRaises(AssertionError): probe.require_helper_continuity(old, bad, old)

    def test_signal_requires_exact_live_owned_helper(self):
        binary = Path('/private/tmp/owned-dtw'); command = str(binary)+' --dtw-overlay-worker'
        identity = {'pid': 17, 'command': command}
        probe.require_owned_signal_target(identity, {'identity_matches': True}, {17: (7, command)}, 7, binary)
        for snapshot, table in [({'identity_matches': False}, {17: (7, command)}),
                                ({'identity_matches': True}, {17: (8, command)}),
                                ({'identity_matches': True}, {17: (7, command+'-wrong')})]:
            with self.assertRaises(AssertionError): probe.require_owned_signal_target(identity, snapshot, table, 7, binary)

    def test_exact_child_and_executable_only(self):
        binary = Path('/private/tmp/owned-dtw')
        table = {1: (7, str(binary)+' --dtw-overlay-worker'),
                 2: (8, str(binary)+' --dtw-overlay-worker'),
                 3: (7, str(binary)+' --dtw-overlay-worker-extra'),
                 4: (7, '/other/dtw --dtw-overlay-worker'),
                 5: (7, str(binary)+' --dtw-native-worker')}
        self.assertEqual(probe.owned_helpers(table, 7, binary), {1: table[1][1]})


class RenderingControls(unittest.TestCase):
    def evidence(self):
        state = {'identity_matches': True, 'windows': [{'id': 23, 'on_screen': True}]}
        return {**state, 'after_capture': copy.deepcopy(state),
                'capture': {'status': 'captured', 'exact_owned_window': 23, 'colored_pixels': 40, 'opaque_pixels': 200}}

    def test_independent_exact_window_pixels(self):
        self.assertTrue(probe.rendering_proof(self.evidence()))

    def test_trace_or_inventory_alone_is_not_rendering(self):
        value = self.evidence(); del value['capture']
        self.assertTrue(probe.visible(value)); self.assertFalse(probe.rendering_proof(value))

    def test_permission_gap_or_blank_image_is_not_success(self):
        for key, value in [('status', 'unavailable'), ('colored_pixels', 0), ('opaque_pixels', 0)]:
            evidence = self.evidence(); evidence['capture'][key] = value
            self.assertFalse(probe.rendering_proof(evidence))

    def test_wrong_window_pixels_rejected(self):
        evidence = self.evidence(); evidence['capture']['exact_owned_window'] = 24
        self.assertFalse(probe.rendering_proof(evidence))

    def test_hidden_or_changed_identity_invalidates_capture(self):
        for field in ('before', 'after'):
            for change in ('birth', 'hidden', 'wrong-window'):
                evidence = self.evidence(); state = evidence if field == 'before' else evidence['after_capture']
                if change == 'birth': state['identity_matches'] = False
                elif change == 'hidden': state['windows'][0]['on_screen'] = False
                else: state['windows'][0]['id'] = 24
                self.assertFalse(probe.rendering_proof(evidence))


class ProbeControls(unittest.TestCase):
    def test_source_extension_requires_one_exact_hook(self):
        hook = 'switch next.trimmingCharacters(in:.whitespacesAndNewlines) {'
        state = 'try? JSONSerialization.data(withJSONObject:value,options:[.sortedKeys]).write(to:output.appendingPathComponent("state.json"),options:.atomic)'
        result = probe.derived_fixture(hook+state, '// owned controls')
        self.assertEqual(result.count('cursorOverlayProbeCommand('), 1)
        self.assertTrue(result.endswith('// owned controls'))
        for base in ['', hook, state, hook+hook+state, hook+state+state]:
            with self.assertRaises(AssertionError): probe.derived_fixture(base, '')

    def test_current_fixture_supports_owned_extension(self):
        root = Path(__file__).resolve().parents[1]/'fixtures'
        result = probe.derived_fixture((root/'native-input/main.swift').read_text(), (root/'cursor-overlay-controls.swift').read_text())
        self.assertIn('func cursorOverlayProbeCommand', result)
        self.assertIn('case "overlay_close": window.close()', result)

    def test_delivery_preserves_provider_evidence(self):
        record = {'script': {'steps': [{'operation': 'click', 'receipt': {
            'outcome': {'delivery': {'route': 'foreground'}},
            'provider_outcome': {'delivery': {'route': 'background', 'foreground_borrowed': False}}}}]}}
        self.assertEqual(probe.require_route(record, 'click', 'background')['route'], 'background')
        record['script']['steps'][0]['receipt']['provider_outcome']['delivery']['foreground_borrowed'] = True
        with self.assertRaises(AssertionError): probe.require_route(record, 'click', 'background')

    def test_strict_focus_error_and_route_reason_are_distinct(self):
        outcome = {'dispatch': 'not_dispatched', 'error': {'code': 'foreground_required'},
                   'delivery': {'route': None, 'reason': 'explicit_focus', 'foreground_borrowed': False}}
        probe.require_strict_focus_rejection(outcome)
        for section, key, value in [('error', 'code', 'explicit_focus'),
                                    ('delivery', 'reason', 'foreground_required'),
                                    ('delivery', 'route', 'foreground'),
                                    ('delivery', 'foreground_borrowed', True)]:
            bad = copy.deepcopy(outcome); bad[section][key] = value
            with self.assertRaises(AssertionError): probe.require_strict_focus_rejection(bad)

    def test_cancel_uses_original_terminal_ledger_without_waiting_for_rpc_response(self):
        record = {'execution_id': 'original', 'status': 'completed', 'error': {'kind': 'cancelled'},
                  'script': {'failure': {'kind': 'cancelled'}, 'state_committed': False,
                             'steps': [{'operation': 'typeText', 'receipt': {'outcome': {'dispatch': 'unknown'},
                                                                          'provider_outcome': {'dispatch': 'unknown'}}}]}}
        calls = []
        def receipt(app, case):
            calls.append((app, case)); return copy.deepcopy(record)
        client = SimpleNamespace(receipt=receipt, pending={})  # No receive method by design.
        result = probe.cancelled_execution_evidence(client, 'owned.app', 'original-case', 'original', 7)
        self.assertEqual(calls, [('owned.app', 'original-case')]*2)
        self.assertEqual(result['exec_response']['availability'], 'unavailable')
        self.assertEqual(result['record'], result['retained_record'])
        with self.assertRaises(AssertionError):
            probe.cancelled_execution_evidence(client, 'owned.app', 'original-case', 'replacement', 7)

    def test_transport_exit_retains_original_rpc_and_execution_fields(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            client = SimpleNamespace(output=output, begin=lambda method, arguments: 17, decode=lambda value: value)
            proof = {'execution_id': 'original'}
            self.assertEqual(probe.begin_inflight(client, 'owned.app', 'return 1;', proof), 17)
            self.assertEqual(proof['rpc_id'], 17)
            self.assertEqual(proof['dispatch_attempts'], 1)
            # No final tools/call reply remains explicitly unavailable.
            (output/'mcp.jsonl').write_text('')
            probe.retained_exec_response(client, proof)
            self.assertEqual(proof['final_receipt']['execution_id'], 'original')
            self.assertEqual(proof['final_receipt']['availability'], 'unavailable')
            # A recorded reply is selected by the original RPC ID and still has
            # to carry that same execution ID; unrelated responses are ignored.
            rows = [{'direction': 'response', 'message': {'id': 99, 'result': {'execution_id': 'other'}}},
                    {'direction': 'response', 'message': {'id': 17, 'result': {'execution_id': 'original'}}}]
            (output/'mcp.jsonl').write_text(''.join(json.dumps(row)+'\n' for row in rows))
            probe.retained_exec_response(client, proof)
            self.assertEqual(proof['final_receipt']['availability'], 'recorded_transport_response_only')
            rows[1]['message']['result']['execution_id'] = 'replacement'
            (output/'mcp.jsonl').write_text(''.join(json.dumps(row)+'\n' for row in rows))
            with self.assertRaises(AssertionError): probe.retained_exec_response(client, proof)

    def test_sample_gap_does_not_claim_no_spawn_coverage(self):
        for times, expected in [([1, 1.02, 1.04], 'established'), ([1], 'unavailable'), ([1, 1.5], 'unavailable')]:
            watcher = probe.ChildWatch.__new__(probe.ChildWatch)
            watcher.stop = SimpleNamespace(set=lambda: None)
            watcher.thread = SimpleNamespace(join=lambda timeout: None, is_alive=lambda: False)
            watcher.error = None
            watcher.samples = [{'at': t, 'helpers': []} for t in times]
            self.assertEqual(watcher.finish()['coverage'], expected)


class CaptureIsolationControls(unittest.TestCase):
    def captures(self):
        capture = {'fixture': {'pid': 7, 'windows': [{'id': 23}], 'window_bounds': [100, 418, 540, 452],
                               'overlay_probe_pad_bounds': [450, 590, 165, 230]},
                   'png': {'width': 1080, 'height': 904}}
        snapshots = [{'identity_matches': True, 'windows': [{'id': number, 'on_screen': True,
                        'bounds': {'x': 518, 'y': 691, 'width': 88, 'height': 64}}]} for number in [31, 32]]
        return capture, copy.deepcopy(capture), snapshots

    def test_exact_union_roi_inside_static_pad(self):
        self.assertEqual(probe.marker_roi(*self.captures()), [832, 542, 184, 136])

    def test_changed_target_geometry_and_missing_marker_rejected(self):
        for change in ('pid', 'window', 'bounds', 'pad', 'dimensions', 'hidden', 'outside-pad'):
            before, after, markers = self.captures()
            if change == 'pid': after['fixture']['pid'] = 8
            elif change == 'window': after['fixture']['windows'][0]['id'] = 24
            elif change == 'bounds': after['fixture']['window_bounds'][0] += 1
            elif change == 'pad': after['fixture']['overlay_probe_pad_bounds'][0] += 1
            elif change == 'dimensions': after['png']['width'] += 1
            elif change == 'hidden': markers[0]['windows'][0]['on_screen'] = False
            else: markers[0]['windows'][0]['bounds']['width'] = 250
            with self.assertRaises(AssertionError): probe.marker_roi(before, after, markers)

    def test_roi_requires_equal_actual_rgba_with_nonempty_content(self):
        result = {'pixels': 200, 'baseline_opaque_pixels': 200, 'different_pixels': 0,
                  'different_channels': 0, 'maximum_channel_delta': 0,
                  'before_rgba_sha256': 'a'*64, 'after_rgba_sha256': 'a'*64}
        probe.require_equal_roi(result)
        for key, value in [('pixels', 0), ('baseline_opaque_pixels', 0), ('different_pixels', 1),
                           ('different_channels', 1), ('maximum_channel_delta', 1), ('after_rgba_sha256', 'b'*64)]:
            changed = dict(result); changed[key] = value
            with self.assertRaises(AssertionError): probe.require_equal_roi(changed)


if __name__ == '__main__': unittest.main()
