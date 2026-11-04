import unittest
import json
from pathlib import Path
import tempfile

from cursor_audit import MAX_SAMPLE_GAP_MS, POLICY_VERSION, attach_routes, borrow_intervals, independence_violation, summarize, trace


class CursorOracleTests(unittest.TestCase):
    def test_restored_endpoint_does_not_hide_transient_motion(self):
        rows = [{'unix_ms': index * 2, 'x': x, 'y': -100}
                for index, x in enumerate([-200, -160, -200])]
        result = trace(rows)
        self.assertEqual(result['before'], result['after'])
        self.assertEqual(result['status'], 'moved')
        self.assertEqual(result['max_displacement_points'], 40)
        self.assertEqual(result['transitions'], 2)

    def test_unavailable_oracle_never_proves_independence(self):
        self.assertEqual(trace([])['status'], 'oracle_unavailable')
        self.assertEqual(trace([{'unix_ms': 0, 'x': 0, 'y': 0}])['status'], 'oracle_unavailable')
        self.assertEqual(trace([{'unix_ms': 0, 'error': 'read_failed'}])['status'], 'oracle_unavailable')
        unavailable = trace([{'unix_ms': 0, 'x': 0, 'y': 0}, {'unix_ms': 25, 'error': 'read_failed'}])
        self.assertEqual(unavailable['max_sample_gap_ms'], 25)

    def test_stable_cursor(self):
        rows = [{'unix_ms': index * 2, 'x': -20, 'y': 100} for index in range(3)]
        self.assertEqual(trace(rows)['status'], 'stable')

    def audited(self, positions=(0, 0, 0, 0, 0, 0)):
        return {'start_unix_ms': 0, 'end_unix_ms': 10,
                'cursor': trace([{'unix_ms': i * 2, 'x': x, 'y': 0} for i, x in enumerate(positions)])}

    def borrowed(self, **overrides):
        return {'pid': 1, 'trace_id': 1, 'window': 5, 'route': 'borrowed',
                'operation': 'move', 'reason': 'native_hover_tracking',
                'start_unix_ms': 2, 'end_unix_ms': 8, **overrides}

    def test_motion_requires_a_bounded_allowed_interval(self):
        row = self.audited((0, 0, 20, 20, 0, 0))
        self.assertIsNotNone(independence_violation(row))
        row['borrow_intervals'] = [self.borrowed()]
        self.assertIsNone(independence_violation(row))
        self.assertEqual(row['cursor']['max_sample_gap_ms'], 2)

    def test_mixed_script_cannot_hide_later_addressed_motion(self):
        row = self.audited((0, 0, 20, 20, 20, 40))
        row['borrow_intervals'] = [self.borrowed()]
        row['routes'] = [{'operation': 'click', 'route': 'addressed'}]
        row['methods'] = ['move', 'click']
        self.assertIn('outside', independence_violation(row))

    def test_transition_must_be_fully_bracketed_inside_borrow(self):
        row = self.audited((0, 20, 20, 20, 20, 20))
        row['borrow_intervals'] = [self.borrowed()]
        self.assertIn('outside', independence_violation(row))

    def test_forbidden_reason_and_operation_never_waive_motion(self):
        for change in [{'reason': 'children_unavailable'}, {'operation': 'click'},
                       {'reason': 'native_keyboard_semantics', 'operation': 'keypress'},
                       {'reason': 'addressed_event_api_unavailable', 'operation': 'focus'}]:
            with self.subTest(change=change):
                row = self.audited()
                row['borrow_intervals'] = [self.borrowed(**change)]
                self.assertIsNotNone(independence_violation(row))
        row['borrow_intervals'] = [self.borrowed(reason='addressed_event_api_unavailable', operation='drag')]
        self.assertIsNone(independence_violation(row))

    def test_legacy_borrow_event_is_readable_but_insufficient(self):
        row = self.audited()
        row['routes'] = [dict(self.borrowed(), at_unix_ms=3)]
        self.assertIn('event-only', independence_violation(row))
        row['borrow_intervals'] = [self.borrowed()]
        self.assertIsNone(independence_violation(row))

    def test_stable_no_input_passes_but_reads_cannot_waive_motion(self):
        row = self.audited()
        row['methods'] = ['look']
        self.assertIsNone(independence_violation(row))
        row['cursor'] = self.audited((0, 0, 20, 20, 0, 0))['cursor']
        self.assertIsNotNone(independence_violation(row))

    def test_missing_pointer_route_evidence_fails_even_when_stable(self):
        row = self.audited()
        row.update(methods=['move', 'click'], dispatch={'unknown': 1},
                   routes=[{'operation': 'move', 'route': 'addressed'}])
        self.assertIn('missing', independence_violation(row))
        row['routes'].append({'operation': 'click', 'route': 'native'})
        self.assertIsNone(independence_violation(row))

    def test_semantic_click_accepts_only_native_press_ax_action_mapping(self):
        row = self.audited()
        row.update(methods=['click'], dispatch={'dispatched': 1},
                   routes=[{'operation': 'press', 'route': 'native', 'reason': 'ax_action'}])
        self.assertIsNone(independence_violation(row))
        for change in [{'route': 'addressed'}, {'reason': 'window_position'}, {'operation': 'set_value'}]:
            candidate = {**row, 'routes': [{**row['routes'][0], **change}]}
            self.assertIn('missing', independence_violation(candidate))
        for operation in ['move', 'drag', 'scroll']:
            candidate = {**row, 'methods': [operation]}
            self.assertIn('missing', independence_violation(candidate))
        row['methods'] = ['click', 'click']
        self.assertIn('missing', independence_violation(row))

    def test_unavailable_or_unbracketed_samples_fail_policy(self):
        row = self.audited()
        for samples in [[], [{'unix_ms': 1, 'x': 0, 'y': 0}, {'unix_ms': 9, 'x': 0, 'y': 0}],
                        [{'unix_ms': 0, 'x': float('nan'), 'y': 0}]]:
            row['cursor'] = trace(samples)
            self.assertIsNotNone(independence_violation(row))

    def test_maximum_sample_gap_policy_is_finite_positive_and_versioned(self):
        self.assertEqual(POLICY_VERSION, 'bounded-pointer-borrow-v3')
        self.assertEqual(MAX_SAMPLE_GAP_MS, 50)

    def test_sample_gap_boundary_and_stable_stalls(self):
        for gap, accepted in [(49.999, True), (50, True), (50.0001, False), (60000, False)]:
            with self.subTest(gap=gap):
                row = {'start_unix_ms': 0, 'end_unix_ms': gap,
                       'cursor': trace([{'unix_ms': 0, 'x': 0, 'y': 0},
                                        {'unix_ms': gap, 'x': 0, 'y': 0}])}
                self.assertEqual(row['cursor']['status'], 'stable')
                self.assertEqual(row['cursor']['max_sample_gap_ms'], gap)
                issue = independence_violation(row)
                if accepted:
                    self.assertIsNone(issue)
                else:
                    self.assertIn('sample gap', issue)

    def test_allowed_borrow_cannot_waive_an_excessive_sample_gap(self):
        row = {'start_unix_ms': 0, 'end_unix_ms': 60004,
               'cursor': trace([{'unix_ms': 0, 'x': 0, 'y': 0},
                                {'unix_ms': 2, 'x': 0, 'y': 0},
                                {'unix_ms': 60002, 'x': 20, 'y': 0},
                                {'unix_ms': 60004, 'x': 20, 'y': 0}]),
               'borrow_intervals': [self.borrowed(start_unix_ms=2, end_unix_ms=60002)]}
        self.assertEqual(row['cursor']['status'], 'moved')
        self.assertIn('sample gap', independence_violation(row))

    def test_missing_or_invalid_sample_gaps_fail_closed(self):
        for gap in [None, float('nan'), float('inf'), -1, '2', True]:
            with self.subTest(gap=gap):
                row = self.audited()
                row['cursor']['max_sample_gap_ms'] = gap
                self.assertIn('sample gap', independence_violation(row))
        row = self.audited()
        del row['cursor']['max_sample_gap_ms']
        self.assertIn('sample gap', independence_violation(row))

    def test_interval_pairing_rejects_incomplete_invalid_and_mismatched_records(self):
        base = {k: v for k, v in self.borrowed().items() if k not in ('start_unix_ms', 'end_unix_ms')}
        begin = dict(base, phase='begin', at_unix_ms=2)
        end = dict(base, phase='end', at_unix_ms=8)
        intervals, errors = borrow_intervals([begin, end])
        self.assertFalse(errors)
        self.assertEqual(intervals, [self.borrowed()])
        for records in [[begin], [end], [begin, end, end], [end, begin],
                        [begin, dict(end, at_unix_ms=1)], [begin, dict(end, operation='click')],
                        [dict(begin, trace_id=None)], [begin, dict(end, at_unix_ms='8')]]:
            with self.subTest(records=records):
                self.assertTrue(borrow_intervals(records)[1])
        row = self.audited()
        row['interval_errors'] = ['incomplete']
        self.assertIsNotNone(independence_violation(row))

    def test_cleanup_and_interval_bounds_must_remain_inside_request(self):
        row = self.audited()
        row['borrow_intervals'] = [self.borrowed(end_unix_ms=12)]
        self.assertIsNotNone(independence_violation(row))
        row['end_unix_ms'] = 12
        self.assertIn('bracket', independence_violation(row))

    def test_attach_routes_keeps_intervals_separate_and_includes_cleanup(self):
        base = {k: v for k, v in self.borrowed().items() if k not in ('start_unix_ms', 'end_unix_ms')}
        records = [('dtw_input ', dict(base, at_unix_ms=2)),
                   ('dtw_input_interval ', dict(base, at_unix_ms=2, phase='begin')),
                   ('dtw_input_interval ', dict(base, at_unix_ms=8, phase='end'))]
        row = dict(self.audited(), transcript='probe/mcp.jsonl')
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            (root / 'probe').mkdir()
            log = root / 'probe/dtw-stderr.log'
            log.write_text('\n'.join(prefix + json.dumps(record) for prefix, record in records))
            attach_routes(root, [row])
            self.assertEqual(len(row['routes']), 1)
            self.assertEqual(row['borrow_intervals'], [self.borrowed()])
            self.assertIsNone(independence_violation(row))
            log.write_text('dtw_input_interval {broken')
            attach_routes(root, [row])
            self.assertIsNotNone(independence_violation(row))

    def test_cancelled_action_keeps_missing_receipt_and_measures_cleanup(self):
        def frame(direction, at, message):
            return {'direction': direction, 'at_unix_ms': at, 'message': message}
        def execute(number, source):
            return {'id': number, 'method': 'tools/call', 'params': {'name': 'exec', 'arguments': {'source': source}}}
        frames = [frame('request', 0, execute(1, 'await app.drag({path:[]});')),
                  frame('request', 2, {'method': 'notifications/cancelled', 'params': {'requestId': 1}}),
                  frame('request', 4, execute(2, 'await app.capture();')),
                  frame('response', 8, {'id': 2, 'result': {'structuredContent': {'status': 'completed'}}})]
        samples = [{'unix_ms': index * 2, 'x': x, 'y': 0} for index, x in enumerate([0, 20, 40, 20, 0])]
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            (root / 'mcp.jsonl').write_text('\n'.join(json.dumps(f) for f in frames))
            rows = summarize(root, samples)
        cancelled = next(row for row in rows if row['request_id'] == 1)
        self.assertEqual(cancelled['completion'], 'no_reply_after_cancel')
        self.assertEqual(cancelled['cursor']['status'], 'moved')
        self.assertNotIn('tool_error', cancelled)


class EofRecoveryTests(unittest.TestCase):
    def fixture(self, root):
        original = root / 'before/mcp.jsonl'
        recovery = root / 'after/mcp.jsonl'
        for path in (original, recovery):
            path.parent.mkdir()
        request = {'direction': 'request', 'at_unix_ms': 0,
                   'message': {'id': 1, 'method': 'tools/call',
                               'params': {'name': 'exec', 'arguments': {'source': 'await app.drag({path:[]});'}}}}
        read = {'direction': 'request', 'at_unix_ms': 6,
                'message': {'id': 1, 'method': 'tools/call',
                            'params': {'name': 'exec', 'arguments': {'source': 'await app.look();'}}}}
        response = {'direction': 'response', 'at_unix_ms': 8,
                    'message': {'id': 1, 'result': {'structuredContent': {
                        'status': 'completed', 'execution_id': 'recovery-execution'}}}}
        boundary = {'mode': 'eof', 'transcript': str(original), 'request_id': 1,
                    'async_response': {'outcome': 'no-response', 'observed_until_unix_ms': 9},
                    'cleanup_read': {'transcript': str(recovery), 'request_id': 1,
                                     'start_unix_ms': 5, 'end_unix_ms': 9,
                                     'execution_id': 'recovery-execution', 'status': 'completed'}}
        return original, recovery, request, read, response, boundary

    def summarize_fixture(self, root, fixture, positions=(0, 0, 0, 0, 0, 0)):
        original, recovery, request, read, response, boundary = fixture
        original.write_text(json.dumps(request) + '\n')
        recovery.write_text('\n'.join(json.dumps(f) for f in (read, response)))
        (root / 'boundary-interruptions.jsonl').write_text(json.dumps(boundary) + '\n')
        samples = [{'unix_ms': i * 2, 'x': x, 'y': 0} for i, x in enumerate(positions)]
        return next(row for row in summarize(root, samples) if row['transcript'] == 'before/mcp.jsonl')

    def test_verified_cross_connection_read_bounds_oracle_without_action_receipt(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            row = self.summarize_fixture(root, self.fixture(root))
        self.assertEqual(row['completion'], 'no_reply_after_eof')
        self.assertEqual((row['start_unix_ms'], row['end_unix_ms']), (0, 8))
        self.assertEqual(row['methods'], ['drag'])
        self.assertIn('not an action receipt', row['oracle_ends_at'])
        self.assertNotIn('dispatch', row)
        self.assertNotIn('tool_error', row)
        self.assertIsNone(independence_violation(row))

    def test_eof_cleanup_motion_still_requires_fully_bounded_borrow(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            row = self.summarize_fixture(root, self.fixture(root), (0, 0, 20, 20, 0, 0))
        self.assertIn('outside', independence_violation(row))
        row['borrow_intervals'] = [{'route': 'borrowed', 'operation': 'drag',
                                    'reason': 'addressed_event_api_unavailable',
                                    'start_unix_ms': 2, 'end_unix_ms': 8}]
        self.assertIsNone(independence_violation(row))
        row['borrow_intervals'][0]['start_unix_ms'] = 3
        self.assertIn('outside', independence_violation(row))
        row['cursor']['max_sample_gap_ms'] = 51
        self.assertIn('sample gap', independence_violation(row))

    def test_unverified_sidecars_preserve_incomplete_request(self):
        changes = [
            ('wrong original', ('transcript',), '/outside/mcp.jsonl'),
            ('relative original', ('transcript',), 'before/mcp.jsonl'),
            ('wrong original id', ('request_id',), 2),
            ('boolean original id', ('request_id',), True),
            ('wrong mode', ('mode',), 'cancel'),
            ('claimed reply', ('async_response', 'outcome'), 'response'),
            ('outside recovery', ('cleanup_read', 'transcript'), '/outside/mcp.jsonl'),
            ('wrong recovery id', ('cleanup_read', 'request_id'), 2),
            ('late start', ('cleanup_read', 'start_unix_ms'), 7),
            ('early end', ('cleanup_read', 'end_unix_ms'), 7),
            ('nonfinite time', ('cleanup_read', 'start_unix_ms'), float('nan')),
            ('early observation', ('async_response', 'observed_until_unix_ms'), 7),
            ('wrong execution', ('cleanup_read', 'execution_id'), 'forged'),
            ('wrong status', ('cleanup_read', 'status'), 'running'),
            ('malformed cleanup', ('cleanup_read',), None),
        ]
        for label, keys, value in changes:
            with self.subTest(label=label), tempfile.TemporaryDirectory() as scratch:
                root = Path(scratch)
                fixture = self.fixture(root)
                target = fixture[-1]
                for key in keys[:-1]:
                    target = target[key]
                target[keys[-1]] = value
                row = self.summarize_fixture(root, fixture)
                self.assertEqual(row['cursor']['status'], 'incomplete_request')
                self.assertIsNotNone(independence_violation(row))
                self.assertNotIn('completion', row)

    def test_sidecar_cannot_forge_recovery_wire_evidence(self):
        changes = [
            ('input script', 3, ('message', 'params', 'arguments', 'source'), 'await app.look(); await app.click({});'),
            ('non exec', 3, ('message', 'params', 'name'), 'other'),
            ('non tools call', 3, ('message', 'method'), 'other'),
            ('wrong request', 3, ('message', 'id'), 2),
            ('wrong response', 4, ('message', 'id'), 2),
            ('wire before bounds', 3, ('at_unix_ms',), 4),
            ('wire after bounds', 4, ('at_unix_ms',), 10),
            ('response before request', 4, ('at_unix_ms',), 5),
            ('tool error', 4, ('message', 'result', 'isError'), True),
            ('protocol error', 4, ('message', 'error'), {'code': -1}),
            ('failed execution', 4, ('message', 'result', 'structuredContent', 'status'), 'failed'),
            ('missing execution', 4, ('message', 'result', 'structuredContent', 'execution_id'), None),
            ('execution failure', 4, ('message', 'result', 'structuredContent', 'failure'), {'code': 'failed'}),
        ]
        for label, index, keys, value in changes:
            with self.subTest(label=label), tempfile.TemporaryDirectory() as scratch:
                root = Path(scratch)
                fixture = self.fixture(root)
                target = fixture[index]
                for key in keys[:-1]:
                    target = target[key]
                target[keys[-1]] = value
                row = self.summarize_fixture(root, fixture)
                self.assertEqual(row['cursor']['status'], 'incomplete_request')
                self.assertIsNotNone(independence_violation(row))

    def test_real_outside_recovery_and_symlink_escape_are_rejected(self):
        for symlink in (False, True):
            with self.subTest(symlink=symlink), tempfile.TemporaryDirectory() as scratch:
                base = Path(scratch)
                root = base / 'audit'
                root.mkdir()
                fixture = self.fixture(root)
                self.summarize_fixture(root, fixture)
                outside = base / 'mcp.jsonl'
                fixture[1].replace(outside)
                if symlink:
                    fixture[1].symlink_to(outside)
                else:
                    fixture[-1]['cleanup_read']['transcript'] = str(outside)
                (root / 'boundary-interruptions.jsonl').write_text(json.dumps(fixture[-1]))
                row = next(row for row in summarize(root, []) if row['transcript'] == 'before/mcp.jsonl')
                self.assertEqual(row['cursor']['status'], 'incomplete_request')

    def test_missing_malformed_or_same_connection_sidecar_cannot_bound_eof(self):
        for sidecar in (None, '{broken', '{}', 'null', '[]'):
            with self.subTest(sidecar=sidecar), tempfile.TemporaryDirectory() as scratch:
                root = Path(scratch)
                self.summarize_fixture(root, self.fixture(root))
                path = root / 'boundary-interruptions.jsonl'
                if sidecar is None:
                    path.unlink()
                else:
                    path.write_text(sidecar)
                row = next(row for row in summarize(root, []) if row['transcript'] == 'before/mcp.jsonl')
                self.assertEqual(row['cursor']['status'], 'incomplete_request')
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            fixture = self.fixture(root)
            fixture[-1]['cleanup_read']['transcript'] = str(fixture[0])
            row = self.summarize_fixture(root, fixture)
            self.assertEqual(row['cursor']['status'], 'incomplete_request')


if __name__ == '__main__':
    unittest.main()
