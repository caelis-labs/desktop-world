import copy
import unittest

from computer_use_cursor import follow_proof


class CursorFollowProofTests(unittest.TestCase):
    def setUp(self):
        self.identity = {'pid': 7, 'birth': 11, 'windows': [13]}
        self.window = {'id': 13, 'on_screen': True,
                       'bounds': {'x': -114, 'y': 186, 'width': 88, 'height': 64}}
        self.snapshot = {'identity_matches': True, 'windows': [self.window],
                         'after_capture': {'identity_matches': True,
                                           'windows': [copy.deepcopy(self.window)]},
                         'capture': {'status': 'captured', 'exact_owned_window': 13,
                                     'colored_pixels': 70, 'opaque_pixels': 100}}

    def proof(self):
        return follow_proof(self.snapshot, self.identity, (-100, 200))

    def test_exact_window_pixels_and_hotspot_prove_negative_display_point(self):
        proof = self.proof()
        self.assertTrue(proof['passed'])
        self.assertEqual(proof['hotspot'], [-100, 200])
        self.assertEqual(proof['window_bounds'], self.window['bounds'])

    def test_visible_rendered_marker_at_previous_position_does_not_prove_follow(self):
        proof = follow_proof(self.snapshot, self.identity, (400, 200))
        self.assertTrue(proof['rendering_verified'])
        self.assertFalse(proof['position_verified'])
        self.assertFalse(proof['passed'])

    def test_visibility_without_pixels_is_not_render_proof(self):
        self.snapshot['capture']['colored_pixels'] = 0
        proof = self.proof()
        self.assertTrue(proof['position_verified'])
        self.assertFalse(proof['rendering_verified'])
        self.assertFalse(proof['passed'])

    def test_capture_of_another_window_is_rejected(self):
        self.snapshot['capture']['exact_owned_window'] = 99
        self.assertFalse(self.proof()['passed'])

    def test_movement_or_identity_change_during_capture_is_rejected(self):
        self.snapshot['after_capture']['windows'][0]['bounds']['x'] += 1
        self.assertFalse(self.proof()['passed'])
        self.snapshot['after_capture']['windows'][0]['bounds']['x'] -= 1
        self.snapshot['after_capture']['identity_matches'] = False
        self.assertFalse(self.proof()['passed'])

    def test_hidden_window_and_wrong_window_owner_proof_are_rejected(self):
        self.snapshot['windows'][0]['on_screen'] = False
        self.assertFalse(self.proof()['passed'])
        self.snapshot['windows'][0]['on_screen'] = True
        self.snapshot['identity_matches'] = False
        self.assertFalse(self.proof()['passed'])

    def test_hotspot_tolerance_is_bounded_in_screen_points(self):
        self.assertTrue(follow_proof(self.snapshot, self.identity, (-99, 201))['passed'])
        self.assertFalse(follow_proof(self.snapshot, self.identity, (-97, 200))['passed'])
        with self.assertRaises(ValueError):
            follow_proof(self.snapshot, self.identity, (-100, 200), float('nan'))


if __name__ == '__main__':
    unittest.main()
