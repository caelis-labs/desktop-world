import copy
import unittest

from direct_input_probe import verify_save


class DirectInputOracleTests(unittest.TestCase):
    def setUp(self):
        self.before = {'items': [
            {'sku': 'DTW-0137', 'quantity': 22, 'note': ''},
            {'sku': 'DTW-0138', 'quantity': 23, 'note': 'neighbor'},
        ], 'saves': []}
        self.after = copy.deepcopy(self.before)
        self.after['items'][0].update(quantity=37, note='中文 😀')
        self.after['saves'].append(copy.deepcopy(self.after['items'][0]))

    def test_verified_note_requires_the_actual_saved_record(self):
        self.assertEqual(verify_save(self.before, self.after, 'DTW-0137', 37, '中文 😀')['note'], '中文 😀')
        self.after['items'][0]['note'] = ''
        with self.assertRaises(AssertionError):
            verify_save(self.before, self.after, 'DTW-0137', 37, '中文 😀')

    def test_neighbor_mutation_cannot_pass_even_when_target_is_correct(self):
        self.after['items'][1]['note'] = 'misdirected input'
        with self.assertRaises(AssertionError):
            verify_save(self.before, self.after, 'DTW-0137', 37, '中文 😀')

    def test_repeated_submission_cannot_pass_as_one_effect(self):
        self.after['saves'].append(copy.deepcopy(self.after['saves'][0]))
        with self.assertRaises(AssertionError):
            verify_save(self.before, self.after, 'DTW-0137', 37, '中文 😀')


if __name__ == '__main__':
    unittest.main()
