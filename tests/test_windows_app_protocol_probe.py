"""State-publication controls for the current Windows entry; no GUI input."""
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    'windows_app_protocol_probe', Path(__file__).parent / 'acceptance/windows/app_protocol_probe.py')
probe = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(probe)


class StatePublicationTests(unittest.TestCase):
    def test_transient_missing_state_is_waited_and_each_predicate_uses_one_snapshot(self):
        pending, ready = {'ready': False}, {'ready': True}
        observed = []
        with patch.object(probe, 'state', side_effect=[None, pending, None, ready]) as state, \
                patch.object(probe.time, 'sleep'):
            result = probe.wait_state(Path('owned'), lambda snapshot: observed.append(snapshot) or snapshot['ready'])
        self.assertIs(result, ready)
        self.assertEqual(observed, [pending, ready])
        self.assertEqual(state.call_count, 4)

    def test_missing_state_still_obeys_the_readiness_deadline(self):
        with patch.object(probe, 'state', return_value=None) as state, \
                patch.object(probe.time, 'monotonic', side_effect=[0, 0, 1]), \
                patch.object(probe.time, 'sleep'), self.assertRaises(AssertionError):
            probe.wait_state(Path('owned'), lambda snapshot: self.fail('missing state reached predicate'), timeout=.1)
        self.assertEqual(state.call_count, 1)

    @unittest.skipUnless(os.name == 'nt', 'Windows publication sharing contract')
    def test_publisher_can_replace_state_while_the_observer_handle_is_open(self):
        import ctypes
        from ctypes import wintypes as w
        kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        kernel.ReplaceFileW.argtypes = [w.LPCWSTR, w.LPCWSTR, w.LPCWSTR, w.DWORD, w.LPVOID, w.LPVOID]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            current, replacement = root / 'state.json', root / 'state.next'
            current.write_text(json.dumps({'sequence': 1}), encoding='utf-8-sig')
            replacement.write_text(json.dumps({'sequence': 2}), encoding='utf-8-sig')
            fdopen = os.fdopen
            replaced = []

            def opened(fd, *args, **kwargs):
                stream = fdopen(fd, *args, **kwargs)
                try:
                    # Match the fixture's .NET File.Replace, not MoveFileEx.
                    if not kernel.ReplaceFileW(str(current), str(replacement), None, 0, None, None):
                        raise ctypes.WinError(ctypes.get_last_error())
                    replaced.append(True)
                    return stream
                except BaseException:
                    stream.close()
                    raise

            with patch.object(os, 'fdopen', side_effect=opened):
                snapshot = probe.state(root)
            self.assertEqual(replaced, [True])
            self.assertEqual(snapshot, {'sequence': 1})
            self.assertEqual(probe.state(root), {'sequence': 2})


if __name__ == '__main__':
    unittest.main()
