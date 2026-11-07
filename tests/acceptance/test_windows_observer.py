"""Native sample evidence controls; no desktop input or Windows APIs."""
import copy
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location('windows_observe_run',
    Path(__file__).with_name('windows') / 'observe_run.py')
observer = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(observer)


class NativeSamplingTests(unittest.TestCase):
    def setUp(self):
        self.rows = [dict(unix_ms=value, foreground_pid=7, cursor_readable=True, cursor={'x': 10, 'y': 20})
                     for value in (0, 10, 20, 30)]
        self.interval = dict(name='owned-background', start_ms=5, end_ms=25,
                             foreground_pid=7, cursor={'x': 10, 'y': 20})

    def test_bracketed_continuous_foreground_and_cursor_pass(self):
        self.assertTrue(observer.check_intervals(self.rows, [self.interval])[0]['passed'])

    def test_matching_endpoints_cannot_hide_a_foreign_middle_sample(self):
        self.rows[1]['foreground_pid'] = 99
        self.assertFalse(observer.check_intervals(self.rows, [self.interval])[0]['passed'])

    def test_cursor_excursion_missing_proof_or_bracket_fails(self):
        for mutate in (lambda rows: rows[1]['cursor'].update(x=11),
                       lambda rows: rows[1].update(cursor_readable=False),
                       lambda rows: rows.pop(0)):
            rows = copy.deepcopy(self.rows)
            mutate(rows)
            self.assertFalse(observer.check_intervals(rows, [self.interval])[0]['passed'])

    def test_actual_large_gap_is_retained_without_changing_observed_effects(self):
        self.rows[2]['unix_ms'] = 71
        self.rows[3]['unix_ms'] = 80
        interval = dict(self.interval, end_ms=75)
        result = observer.check_intervals(self.rows, [interval])[0]
        self.assertTrue(result['passed'])
        self.assertEqual(result['max_gap_ms'], 61)
        self.assertEqual(result['sampling_status'], 'observed')
        self.assertTrue(result['sample_gaps_are_diagnostic'])

    def test_missing_boundaries_are_unknown_not_preserved(self):
        result = observer.check_intervals([], [self.interval])[0]
        self.assertFalse(result['passed'])
        self.assertEqual(result['sampling_status'], 'unknown')
        self.assertNotIn('cursor_preserved', result)

    def test_invalid_clocks_are_not_diagnosed_as_quiet(self):
        for frequency in (0, -1):
            rows = [dict(row, timestamp=index, frequency=frequency) for index, row in enumerate(self.rows)]
            result = observer.check_intervals(rows, [self.interval])[0]
            self.assertFalse(result['passed'])
            self.assertEqual(result['error'], 'invalid_sample_clock')
        rows = copy.deepcopy(self.rows)
        rows[2]['unix_ms'] = 5
        self.assertIsNone(observer.sample_gaps(rows))

    def test_monotonic_gap_retained_and_foreign_sample_still_fails(self):
        rows = [dict(row, timestamp=index * 200, frequency=1000) for index, row in enumerate(self.rows)]
        rows[1]['foreground_pid'] = 99
        result = observer.check_intervals(rows, [self.interval])[0]
        self.assertEqual(result['max_gap_ms'], 200)
        self.assertFalse(result['passed'])

    def test_missing_call_times_do_not_create_sampling_success(self):
        self.assertFalse(observer.check_intervals(self.rows, [{'name': 'missing'}])[0]['passed'])

    def test_boundary_only_interval_does_not_claim_foreground_or_cursor_preservation(self):
        self.rows[1]['foreground_pid'] = 99
        self.rows[1]['cursor']['x'] += 50
        result = observer.check_intervals(self.rows, [dict(name='boundary-only', start_ms=5, end_ms=25)])[0]
        self.assertTrue(result['passed'])
        self.assertFalse(result['foreground_asserted'])
        self.assertFalse(result['cursor_asserted'])
        self.assertNotIn('foreground_preserved', result)
        self.assertNotIn('cursor_preserved', result)


if __name__ == '__main__':
    unittest.main()
