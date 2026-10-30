#!/usr/bin/env python3
"""Pure controls for anchored-scroll evidence; no native input or app launch."""
import copy
import unittest

from anchored_scroll_probe import (execute, map_pixel, require_browser_effect, require_native_effect,
                                   require_session, require_stale, semantic_source)


def native_before():
    return {'events_count': 0, 'windows': [{'id': 7}], 'offsets': {'left': 0, 'right': 0, 'nested': 0}}


def native_after():
    return {'events_count': 3, 'windows': [{'id': 7}], 'offsets': {'left': 400, 'right': 0, 'nested': 0},
            'events': [{'sequence': i+1, 'window_id': 7, 'region': 'left', 'delta_y': -delta,
                        'screen_point': [120, 90]} for i, delta in enumerate((10, 10, 5))]}


def rejected_record():
    return {'status': 'completed', 'error': {'code': 'stale_observation'}, 'script': {'steps': [
        {'operation': 'scroll', 'receipt': {'outcome': {'dispatch': 'not_dispatched',
         'error': {'code': 'stale_observation'}}, 'provider_outcome': None}}]}}


class AnchoredScrollControls(unittest.TestCase):
    def test_locked_precondition_is_explicit_and_unknown_is_not_unlocked(self):
        with self.assertRaisesRegex(AssertionError, 'gui_precondition_locked'):
            require_session({'screen_locked': True})
        self.assertIsNone(require_session({'screen_locked': None})['screen_locked'])
        self.assertFalse(require_session({'screen_locked': False})['screen_locked'])
        self.assertNotIn('screen_locked', require_session({}))
        with self.assertRaises(AssertionError): require_session({'screen_locked': 1})

    def test_pixel_mapping_uses_capture_dimensions(self):
        self.assertEqual(map_pixel([100, 80, 800, 500], {'width': 1600, 'height': 1000}, [300, 180]), {'x': 400, 'y': 200})

    def test_pixel_mapping_rejects_nonfinite_and_outside(self):
        for point in ([1000, 100], [99, 100], [200, float('nan')]):
            with self.assertRaises(AssertionError): map_pixel([100, 80, 800, 500], {'width': 1600, 'height': 1000}, point)

    def test_full_three_tick_amount_and_unchanged_neighbors(self):
        self.assertEqual(len(require_native_effect(native_before(), native_after(), 'left', 25, [120, 90])), 3)

    def test_truncated_and_replayed_wheels_do_not_pass(self):
        for delta in (-10, -30):
            after = native_after(); after['events'][2]['delta_y'] = delta
            with self.assertRaisesRegex(AssertionError, 'truncated_or_replayed'):
                require_native_effect(native_before(), after, 'left', 25, [120, 90])

    def test_received_wheel_without_full_business_offset_does_not_pass(self):
        after = native_after(); after['offsets']['left'] = 160
        with self.assertRaisesRegex(AssertionError, 'full_amount'):
            require_native_effect(native_before(), after, 'left', 25, [120, 90])

    def test_wrong_region_or_window_does_not_pass(self):
        for key, value in (('region', 'right'), ('window_id', 8)):
            after = native_after(); after['events'][0][key] = value
            with self.assertRaisesRegex(AssertionError, 'other_region'):
                require_native_effect(native_before(), after, 'left', 25, [120, 90])

    def test_neighbor_motion_does_not_pass(self):
        after = native_after(); after['offsets']['nested'] = 1
        with self.assertRaisesRegex(AssertionError, 'neighbor'):
            require_native_effect(native_before(), after, 'left', 25, [120, 90])

    def test_semantic_point_must_be_inside_original_label_rect(self):
        require_native_effect(native_before(), native_after(), 'left', 25, [500, 500], [100, 80, 220, 28])
        with self.assertRaisesRegex(AssertionError, 'point_does_not_match'):
            require_native_effect(native_before(), native_after(), 'left', 25, [120, 90], [500, 500, 20, 20])

    def test_default_center_cannot_satisfy_offcenter_pixel_anchor(self):
        with self.assertRaisesRegex(AssertionError, 'point_does_not_match'):
            require_native_effect(native_before(), native_after(), 'left', 25, [500, 300])

    def test_stale_requires_not_dispatched_and_zero_effects(self):
        before = native_before(); require_stale(rejected_record(), before, copy.deepcopy(before))
        for field in ('dispatch', 'code'):
            record = rejected_record(); outcome = record['script']['steps'][0]['receipt']['outcome']
            if field == 'dispatch': outcome['dispatch'] = 'unknown'
            else: outcome['error']['code'] = 'foreground_required'
            with self.assertRaises(AssertionError): require_stale(record, before, copy.deepcopy(before))
        after = copy.deepcopy(before); after['events_count'] += 1
        with self.assertRaises(AssertionError): require_stale(rejected_record(), before, after)

    def test_stale_rejects_nonterminal_or_missing_terminal_state(self):
        before = native_before()
        for status in ('running', 'failed', None):
            record = rejected_record()
            if status is None: record.pop('status')
            else: record['status'] = status
            with self.assertRaisesRegex(AssertionError, 'terminal execution'):
                require_stale(record, before, copy.deepcopy(before))

    def test_stale_rejects_terminal_success_or_missing_error(self):
        before = native_before()
        for error in (None, False, {}, 'missing'):
            record = rejected_record()
            if error == 'missing': record.pop('error')
            else: record['error'] = error
            with self.assertRaisesRegex(AssertionError, 'terminal execution'):
                require_stale(record, before, copy.deepcopy(before))

    def test_semantic_query_is_bounded_nonpressable_static_text(self):
        source = semantic_source('Owned window', 'nested')
        self.assertIn("role:'static_text'", source)
        self.assertIn('pages.length>=12', source)
        self.assertIn('Nested anchor', source)
        self.assertNotIn('can_press', source)
        self.assertNotIn('app.press', source)

    def test_original_receipt_is_kept_before_failure_is_raised(self):
        record = {'app': 'owned', 'execution_id': 'original', 'status': 'completed', 'error': {'code': 'unknown'}}
        class Client:
            exec_frames = {('owned', 'case'): {'id': 42}}
            calls = 0
            def execute(self, app, case, source):
                self.calls += 1
                return record, {}
        client, proof = Client(), {}
        with self.assertRaises(AssertionError): execute(client, 'owned', 'case', 'scroll once', proof)
        self.assertEqual(proof['execution_id'], 'original'); self.assertEqual(proof['rpc_id'], 42)
        self.assertEqual(proof['record'], record); self.assertEqual(client.calls, 1)
        self.assertEqual(proof['dispatch_attempts'], 1)

    def test_transport_failure_keeps_already_known_original_identity(self):
        class Client:
            exec_frames = {('owned', 'case'): {'id': 77}}
            executions = {('owned', 'case'): 'original-running'}
            def execute(self, app, case, source):
                raise EOFError('response transport failed')
        proof = {}
        with self.assertRaises(EOFError): execute(Client(), 'owned', 'case', 'scroll once', proof)
        self.assertEqual(proof['execution_id'], 'original-running')
        self.assertEqual(proof['rpc_id'], 77)
        self.assertEqual(proof['dispatch_attempts'], 1)

    def browser_sample(self):
        before = {'sequence': 4, 'offsets': {'left': 0, 'right': 0, 'nested': 0}}
        after = {'sequence': 7, 'offsets': {'left': 320, 'right': 0, 'nested': 0}}
        events = [{'type': 'wheel', 'sequence': 5, 'trusted': True, 'region': 'left',
                   'delta_mode': 0, 'delta_y': 320, 'client_point': [80, 100]}]
        return before, after, events

    def test_browser_ordinary_scroll_matches_full_trusted_delta(self):
        before, after, events = self.browser_sample()
        self.assertEqual(require_browser_effect(before, after, events, 'left', [80, 100]), events)

    def test_browser_untrusted_wrong_region_and_unknown_units_fail(self):
        for key, value in (('trusted', False), ('region', 'nested'), ('delta_mode', 1)):
            before, after, events = self.browser_sample(); events[0][key] = value
            with self.assertRaises(AssertionError): require_browser_effect(before, after, events, 'left', [80, 100])

    def test_browser_truncated_offset_or_neighbor_motion_fails(self):
        for name, delta in (('left', -10), ('nested', 5)):
            before, after, events = self.browser_sample(); after['offsets'][name] += delta
            with self.assertRaises(AssertionError): require_browser_effect(before, after, events, 'left', [80, 100])

    def test_browser_different_point_and_wrong_semantic_label_fail(self):
        before, after, events = self.browser_sample()
        with self.assertRaises(AssertionError): require_browser_effect(before, after, events, 'left', [500, 300])
        with self.assertRaises(AssertionError): require_browser_effect(before, after, events, 'left', [80, 100], [400, 300, 100, 20])


if __name__ == '__main__':
    unittest.main()
