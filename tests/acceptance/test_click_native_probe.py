"""Negative controls: dispatch success and a wrong/empty event log must fail."""
import unittest

from click_native_probe import validate_case


class NativeEventAssertions(unittest.TestCase):
    def test_failed_or_unknown_receipt_never_passes(self):
        for status in ("failed", "unknown", "cancelled", "running"):
            with self.subTest(status=status), self.assertRaises(AssertionError):
                validate_case("bound_right", {"status": status}, [])

    def test_completed_without_events_fails_for_every_valid_action(self):
        for case in ("adapted_ctrl", "native_scalar", "bound_right", "plain_right", "double", "native_right_ctrl"):
            with self.subTest(case=case), self.assertRaises(AssertionError):
                validate_case(case, {"status": "completed"}, [])

    def test_wrong_modifiers_button_or_double_count_fail(self):
        for case, events in [
            ("adapted_ctrl", [("leftDown", 1, False), ("leftUp", 1, False)]),
            ("bound_right", [("leftDown", 1, False), ("leftUp", 1, False)]),
            ("double", [("leftDown", 1, False), ("leftUp", 1, False)] * 2),
        ]:
            with self.subTest(case=case), self.assertRaises(AssertionError):
                validate_case(case, {"status": "completed"},
                              [dict(kind=k, count=n, ctrl=c) for k, n, c in events])

    def test_rejected_input_with_an_event_fails(self):
        with self.assertRaises(AssertionError):
            validate_case("adapted_scalar_rejected", {"status": "failed", "text": "DTW click"},
                          [{"kind": "leftDown", "count": 1, "ctrl": False}])


if __name__ == "__main__":
    unittest.main()
