import unittest

from computer_use_acceptance import merge_evidence
from computer_use_benchmark import BenchmarkReport, load_targets
from cursor_audit import MAX_SAMPLE_GAP_MS, POLICY_VERSION, trace


class UnifiedAcceptanceTests(unittest.TestCase):
    def benchmark(self, *, clean=True, virtual=True, focus=True):
        report = BenchmarkReport(load_targets(), {})
        for row in report.targets['scenarios']:
            if row['profile'] == 'core' or row['id'] == 'P01' and virtual or row['id'] == 'P03' and focus:
                report.record(row['id'], 'passed', {})
        report.data['cleanup'] = {'passed': True}
        report.data['clean_run'] = clean
        return report.snapshot()

    def audit(self, points=(0, 0, 0)):
        samples = [{'unix_ms': index * 4, 'x': x, 'y': 10} for index, x in enumerate(points)]
        return {'policy_version': POLICY_VERSION, 'max_sample_gap_ms_policy': MAX_SAMPLE_GAP_MS, 'independence_violations': [], 'calls': [{
            'request_id': 1, 'start_unix_ms': 2, 'end_unix_ms': 6, 'methods': ['look'],
            'routes': [], 'borrow_intervals': [], 'interval_errors': [], 'cursor': trace(samples)}]}

    def merge(self, benchmark, audit):
        return merge_evidence(benchmark, audit, capture_prefix='cursor/probe', audit_path='cursor/report.json')

    def test_core_virtual_and_cursor_policy_are_required_together(self):
        result = self.merge(self.benchmark(), self.audit())
        self.assertTrue(result['acceptance_passed'])
        self.assertFalse(result['target_baseline_passed'])
        self.assertTrue(result['cursor_policy']['passed'])
        self.assertFalse(self.merge(self.benchmark(virtual=False), self.audit())['acceptance_passed'])
        self.assertFalse(self.merge(self.benchmark(focus=False), self.audit())['acceptance_passed'])
        self.assertFalse(self.merge(self.benchmark(clean=False), self.audit())['acceptance_passed'])

    def test_wrapper_recomputes_policy_instead_of_trusting_empty_violation_list(self):
        result = self.merge(self.benchmark(), self.audit((0, 30, 0)))
        self.assertFalse(result['cursor_policy']['passed'])
        self.assertFalse(result['acceptance_passed'])
        self.assertIn('outside an allowed borrow interval', result['cursor_policy']['violations'][0]['failure'])

    def test_missing_or_historical_policy_cannot_pass(self):
        audit = self.audit()
        audit.pop('policy_version')
        self.assertFalse(self.merge(self.benchmark(), audit)['acceptance_passed'])
        audit['policy_version'] = POLICY_VERSION
        audit.pop('independence_violations')
        self.assertFalse(self.merge(self.benchmark(), audit)['acceptance_passed'])

    def test_outer_review_retains_original_image_location(self):
        benchmark = self.benchmark()
        case = next(row for row in benchmark['cases'] if row['id'] == 'C01')
        case.update(status='pending_review', detail={'capture': 'mcp/native-chat.png', 'capture_sha256': 'original'})
        result = self.merge(benchmark, self.audit())
        updated = next(row for row in result['cases'] if row['id'] == 'C01')
        self.assertEqual(updated['detail']['capture'], 'cursor/probe/mcp/native-chat.png')
        self.assertFalse(result['acceptance_passed'])


if __name__ == '__main__':
    unittest.main()
