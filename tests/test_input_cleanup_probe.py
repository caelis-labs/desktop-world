"""Timing evidence tests; never read or post real desktop input."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent / "acceptance"))
from input_cleanup_probe import mid_pair_hit


class InputCleanupEvidenceTests(unittest.TestCase):
    def evidence(self):
        return ({"monotonic_ns": 10},
                {"sample": {"down": True, "before_monotonic_ns": 20, "after_monotonic_ns": 21},
                 "written_monotonic_ns": 30,
                 "sample_after_write": {"down": True, "before_monotonic_ns": 31, "after_monotonic_ns": 32}},
                {"monotonic_ns": 40})

    def test_request_sent_and_direct_sample_still_down(self):
        self.assertTrue(mid_pair_hit(*self.evidence()))

    def test_late_poll_cannot_hide_up_before_request_finished(self):
        down, trigger, up = self.evidence()
        # A poll notices up at 40, but the direct sample already sees it at 31.
        trigger["sample_after_write"]["down"] = False
        self.assertFalse(mid_pair_hit(down, trigger, up))

    def test_missed_down_before_send_cannot_count_as_mid_pair(self):
        down, trigger, up = self.evidence()
        trigger["sample"]["down"] = False
        self.assertFalse(mid_pair_hit(down, trigger, up))

    def test_observer_up_before_post_send_sample_is_not_proof(self):
        down, trigger, up = self.evidence()
        up["monotonic_ns"] = 31
        self.assertFalse(mid_pair_hit(down, trigger, up))


if __name__ == "__main__":
    unittest.main()
