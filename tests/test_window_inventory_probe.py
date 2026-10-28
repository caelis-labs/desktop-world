"""Negative controls for inventory acceptance; no native access."""
import sys
from pathlib import Path
import unittest
sys.path.insert(0, str(Path(__file__).parent / "acceptance"))
from window_inventory_probe import validate_reply


class InventoryAssertions(unittest.TestCase):
    def test_empty_success_cannot_prove_attribution_failure(self):
        reply = {"status": "completed", "error": None, "value": {"windows": [],
            "coverage": {"status": "empty_within_scope", "complete_within_scope": True, "supported_count": 0}}}
        with self.assertRaises(AssertionError):
            validate_reply(reply, "windows", 0, "window_inventory_unproven")
        validate_reply(reply, "windows", 0, None)
        with self.assertRaises(AssertionError):
            validate_reply(reply, "windows", 2, None)

    def test_failed_script_must_keep_step_error_and_not_commit_state(self):
        error = {"code": "window_inventory_unproven", "message": "text is not a contract",
                 "details": {"coverage": {"status": "unproven", "complete_within_scope": False}, "diagnostics": {"counters": {"ax_window_count": 0}}}}
        reply = {"error": error, "value": {"calls": 1}, "script": {
            "state_committed": False, "steps": [{"operation": "windows", "error": error}]}}
        validate_reply(reply, "script", 0, "window_inventory_unproven")
        reply["script"]["state_committed"] = True
        with self.assertRaises(AssertionError):
            validate_reply(reply, "script", 0, "window_inventory_unproven")
        reply["script"]["state_committed"] = False
        reply["value"]["result"] = {"windows": []}
        with self.assertRaises(AssertionError):
            validate_reply(reply, "script", 0, "window_inventory_unproven")


if __name__ == "__main__":
    unittest.main()
