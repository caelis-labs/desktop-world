#!/usr/bin/env python3
"""Pure raised-surface evidence checks; no process launch or GUI input."""
import copy
import unittest

import application_surface_probe as probe


class SurfaceEvidenceTests(unittest.TestCase):
    def test_image_mapping_uses_exact_frame_and_rejects_outside_points(self):
        surface = {'bounds': [-200, 100, 400, 200], 'pad_screen_point': [0, 150]}
        shot = {'image': {'width': 800, 'height': 400}}
        self.assertEqual(probe.image_point(surface, shot), {'x': 400, 'y': 100})
        surface['pad_screen_point'] = [200, 150]
        with self.assertRaises(AssertionError): probe.image_point(surface, shot)
        surface['pad_screen_point'] = [float('nan'), 150]
        with self.assertRaises(AssertionError): probe.image_point(surface, shot)

    def samples(self):
        before = {'application_surface': {'id': 9, 'pad': {'left_down': 0, 'left_up': 0, 'right_down': 0, 'right_up': 0}},
                  'pad': {'left_down': 0, 'left_up': 0}, 'pointer_screen': [20, 40]}
        after = copy.deepcopy(before)
        after['application_surface']['pad'].update(left_down=1, left_up=1)
        after.update(pointer_screen=[900, 800], keys_down_global=[55], frontmost_pid=6789)
        events = [{'kind': kind, 'window_number': 9} for kind in ['surface_left_down', 'surface_left_up']]
        return before, after, events

    def test_click_accepts_unrelated_desktop_changes_but_requires_exact_received_pair(self):
        before, after, events = self.samples()
        self.assertEqual(probe.verify_click(before, after, [{'kind': 'key_down', 'window_number': 8}]+events), events)
        for broken in [events[:1], events+events, [dict(events[0], window_number=8), events[1]]]:
            with self.assertRaises(AssertionError): probe.verify_click(before, after, broken)

    def test_click_rejects_effect_on_ordinary_window(self):
        before, after, events = self.samples()
        after['pad']['left_down'] = 1
        with self.assertRaises(AssertionError): probe.verify_click(before, after, events)

    def refusal(self):
        return {'status': 'completed', 'error': {'code': 'script_exception'},
                'context_reset': False, 'state_reset': False,
                'script': {'state_committed': False, 'failure': {'code': 'target_changed'},
                           'steps': [{'operation': 'click', 'error': {'code': 'target_changed'},
                                      'receipt': {'outcome': {'dispatch': 'not_dispatched'}, 'provider_outcome': None}}]}}

    def test_stale_refusal_rejects_unknown_effect_or_other_failure(self):
        record = self.refusal()
        probe.verify_stale_refusal(record)
        record['script']['steps'][0]['receipt']['outcome']['dispatch'] = 'unknown'
        with self.assertRaises(AssertionError): probe.verify_stale_refusal(record)
        record = self.refusal(); record['script']['failure']['code'] = 'timed_out'
        with self.assertRaises(AssertionError): probe.verify_stale_refusal(record)
        record = self.refusal(); record['state_reset'] = True
        with self.assertRaises(AssertionError): probe.verify_stale_refusal(record)


if __name__ == '__main__':
    unittest.main()
