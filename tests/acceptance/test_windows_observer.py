"""Strict native sample evaluation controls; no desktop input or Windows APIs."""
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

    def test_actual_large_gap_fails_despite_unchanged_endpoints(self):
        self.rows[2]['unix_ms'] = 71
        self.rows[3]['unix_ms'] = 80
        interval = dict(self.interval, end_ms=75)
        self.assertFalse(observer.check_intervals(self.rows, [interval])[0]['passed'])


if __name__ == '__main__':
    unittest.main()
