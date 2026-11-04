"""Owned-fixture cleanup controls; no desktop processes are launched."""
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import macos_scene_sequence_probe as probe


class FixtureCleanupTests(unittest.TestCase):
    def run_cleanup(self, rows, waits):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            executable = root / 'Scene.app/Contents/MacOS/Scene'
            frames = [subprocess.CompletedProcess([], 0, stdout=(f'123 {executable}\n' if row else ''))
                      for row in rows]
            record = {'forced': []}
            with patch.object(probe.subprocess, 'run', side_effect=frames), \
                    patch.object(probe, 'wait_until', side_effect=waits), \
                    patch.object(probe.os, 'kill') as kill:
                failures = probe.cleanup_fixture(root, executable, lambda: None, record)
                self.assertEqual((root / 'command').read_text(), 'quit')
                self.assertTrue(record['quit_requested'])
                return failures, record, kill.call_args_list

    def test_missing_state_still_requests_quit_and_verifies_normal_exit(self):
        failures, record, kills = self.run_cleanup([True, True], [True])
        self.assertEqual(failures, [])
        self.assertEqual(record['status'], 'exited_after_quit')
        self.assertEqual(record['pids'], [123])
        self.assertEqual(kills, [])

    def test_missing_state_and_no_process_preserves_a_delayed_quit_command(self):
        failures, record, kills = self.run_cleanup([False, False], [AssertionError('not observed')])
        self.assertEqual(failures, [])
        self.assertEqual(record['status'], 'not_running_after_quit')
        self.assertEqual(kills, [])

    def test_forced_cleanup_is_reported_as_failure(self):
        failures, record, kills = self.run_cleanup([True, True, True], [AssertionError('still running'), True])
        self.assertTrue(failures)
        self.assertEqual(record['status'], 'exited_after_forced_cleanup')
        self.assertEqual(record['forced'], [{'pid': 123, 'signal': 'SIGTERM'}])
        self.assertEqual(len(kills), 1)
        self.assertEqual(kills[0].args, (123, 15))


if __name__ == '__main__':
    unittest.main()
