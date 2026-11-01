#!/usr/bin/env python3
"""Pure action-scoped observation checks; no processes, GUI, or input start."""
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import background_input_probe as probe


class FocusEstablishedTraceTests(unittest.TestCase):
    def test_native_trace_accepts_additional_fields(self):
        for extra in ['', 'activation_window=42 ', 'activation_window=42 revision=3 ']:
            trace = f'dtw_focus_transition pid=7 window=42 {extra}phase=focus_established extra=value'
            self.assertTrue(probe.focus_established_trace(trace, 7, 42))

    def test_wrong_target_or_incomplete_phase_never_opens_barrier(self):
        for trace in [
            'dtw_focus_transition pid=70 window=42 phase=focus_established',
            'dtw_focus_transition pid=7 window=420 phase=focus_established',
            'dtw_focus_transition pid=7 window=42 phase=focus_established_later',
            'dtw_focus_transition pid=7 window=42 phase=raise_requested',
            'dtw_focus_transition pid=7 window=42 other_phase=focus_established',
            'other_trace pid=7 window=42 phase=focus_established',
            'dtw_focus_transition pid=7 window=42\nphase=focus_established',
            'dtw_focus_transition pid=7 window=42 phase=raise_requested phase=focus_established',
        ]:
            with self.subTest(trace=trace):
                self.assertFalse(probe.focus_established_trace(trace, 7, 42))


class ExactForegroundRetentionTests(unittest.TestCase):
    def test_same_pid_wrong_window_is_not_retained(self):
        with self.assertRaisesRegex(AssertionError, 'exact window'):
            probe.require_exact_foreground({'pid':7,'frontmost_pid':7,'key_window':22},7,11)

    def test_wrong_process_with_matching_window_cannot_confirm_retention(self):
        for state in [{'pid':7,'frontmost_pid':8,'key_window':11},
                      {'pid':8,'frontmost_pid':7,'key_window':11}]:
            with self.assertRaisesRegex(AssertionError, 'exact window'):
                probe.require_exact_foreground(state,7,11)

    def test_exact_selected_window_confirms_retention(self):
        probe.require_exact_foreground({'pid':7,'frontmost_pid':7,'key_window':11},7,11)


class DesktopEvidenceTests(unittest.TestCase):
    def verify(self, samples, activations=(), target_pids=(9,), end=10.1, initial_pid=4):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for name, rows in [('desktop.jsonl', samples), ('focus.jsonl', activations)]:
                (root/name).write_text(''.join(json.dumps(row)+'\n' for row in rows))
            before = {'published_uptime': 10, 'frontmost_pid': initial_pid, 'pointer_screen': [300, 200]}
            after = {**before, 'published_uptime': end}
            proof = {}
            probe.observe_desktop(SimpleNamespace(root=root), before, after, proof, forbidden_pids=target_pids)
            return proof

    def samples(self):
        return [{'uptime': 10+i*0.02, 'frontmost_pid': 4, 'pointer_screen': [300, 200]}
                for i in range(1, 6)]

    def test_matching_fresh_samples_retain_observation_boundary(self):
        result = self.verify(self.samples(), [{'uptime': 10.01, 'frontmost_pid': 4}])
        self.assertEqual(len(result['samples']), 5)
        self.assertIn('shorter excursions', result['pointer_evidence'])
        self.assertEqual(result['background_coverage']['status'], 'established')

    def test_unrelated_foreground_pointer_and_held_input_do_not_fail_keyboard_coverage(self):
        samples = self.samples()
        samples[1].update(frontmost_pid=7, pointer_screen=[100, 50], keys_down_global=[55],
                          modifier_flags_global=0x100000, buttons_down_global=[0])
        result = self.verify(samples, [{'uptime': 10.03, 'frontmost_pid': 7}])
        self.assertEqual(result['background_coverage']['status'], 'established')
        self.assertEqual(len(result['pointer_deltas']), 2)
        self.assertEqual(len(result['foreground_changes']), 2)
        self.assertIn('cannot be attributed', result['pointer_evidence'])

    def test_target_foreground_sample_is_unproven_background_not_attributed_failure(self):
        samples = self.samples(); samples[1]['frontmost_pid'] = 9
        result = self.verify(samples)
        self.assertEqual(result['background_coverage']['status'], 'not_established')
        self.assertEqual(result['target_foreground_observations'][0]['source'], 'sample')

    def test_transient_target_activation_is_reported_even_after_it_returned(self):
        result = self.verify(self.samples(), [{'uptime': 10.01, 'frontmost_pid': 9},
                                             {'uptime': 10.015, 'frontmost_pid': 4}])
        self.assertEqual(result['background_coverage']['status'], 'not_established')
        self.assertEqual(result['target_foreground_observations'][0]['source'], 'activation')

    def test_already_foreground_target_cannot_establish_background_coverage(self):
        result = self.verify(self.samples(), initial_pid=9)
        self.assertEqual(result['background_coverage']['status'], 'not_established')
        self.assertEqual(result['target_foreground_observations'][0]['source'], 'baseline')

    def test_missing_final_or_stale_samples_cannot_claim_success(self):
        for rows in ([], self.samples()[:-1], self.samples()[-1:]):
            with self.subTest(rows=rows), self.assertRaisesRegex(AssertionError, 'fresh desktop samples'):
                self.verify(rows)

    def test_publication_gap_cannot_claim_continuous_coverage(self):
        rows = self.samples()[:1]+[{'uptime': 10.5, 'frontmost_pid': 4, 'pointer_screen': [300, 200]}]
        result = self.verify(rows, end=10.5)
        self.assertEqual(result['sampling_coverage']['reason'], 'publication_gap')
        self.assertEqual(result['background_coverage']['status'], 'not_established')
        self.assertGreater(result['largest_sample_gap_seconds'], 0.2)

    def test_browser_prefix_cannot_stand_in_for_backspace_completion(self):
        from background_browser_probe import completed_keyboard_sample
        events = [
            {'type':'probe_keydown','probe_seq':1,'control':'note','trusted':True,'key':'a','meta':True},
            {'type':'probe_input','probe_seq':2,'control':'note','trusted':True},
            {'type':'probe_snapshot','probe_seq':3,'note':'done','dialog_open':True}]
        self.assertIsNone(completed_keyboard_sample({'events':events}, 0, 'done'))
        events.extend([
            {'type':'probe_keydown','probe_seq':4,'control':'note','trusted':True,'key':'Backspace'},
            {'type':'probe_keyup','probe_seq':5,'control':'note','trusted':True,'key':'Backspace'}])
        self.assertIsNone(completed_keyboard_sample({'events':events}, 0, 'done'))
        events.append({'type':'probe_snapshot','probe_seq':6,'note':'done','dialog_open':True})
        self.assertIsNotNone(completed_keyboard_sample({'events':events}, 0, 'done'))
        events[-2]['probe_seq'] = 3
        self.assertIsNone(completed_keyboard_sample({'events':events}, 0, 'done'))
        events[-2]['probe_seq'] = 5
        events[-2]['trusted'] = False
        self.assertIsNone(completed_keyboard_sample({'events':events}, 0, 'done'))

    def test_partial_append_preserves_completed_rows(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root/'events.jsonl').write_bytes(b'{"sequence":1}\n{"sequence":')
            self.assertEqual(probe.journal(SimpleNamespace(root=root), 'events.jsonl'), [{'sequence': 1}])

    def test_fresh_end_does_not_require_user_input_released(self):
        states = iter([{'published_uptime': stamp, 'keys_down_global': [55],
                        'modifier_flags_global': 0x100000, 'buttons_down_global': [0]}
                       for stamp in (10.0, 10.05, 10.11)])
        fixture = SimpleNamespace(state=lambda: next(states))
        with patch.object(probe.time, 'sleep'):
            self.assertEqual(probe.fresh_end(fixture)['published_uptime'], 10.11)


class OwnedInputEvidenceTests(unittest.TestCase):
    def test_own_pairs_ignore_global_user_modifiers_and_buttons(self):
        state = {'published_uptime': 10, 'keys_down_global': [55],
                 'modifier_flags_global': 0x100000, 'buttons_down_global': [0]}
        rows = [{'sequence': 11, 'kind': 'key_down', 'key_code': 0, 'flags': 0},
                {'sequence': 12, 'kind': 'key_up', 'key_code': 0, 'flags': 0}]
        with patch.object(probe, 'fresh_end', return_value=state), patch.object(probe, 'journal', return_value=rows):
            proof = probe.owned_input_pairs(object(), {'events_count': 10}, {})
        self.assertEqual(proof['unpaired'], {})
        self.assertEqual(proof['after']['keys_down_global'], [55])

    def test_earlier_extra_up_cannot_cover_later_unreleased_down(self):
        result = probe.observed_pairs([{'kind': 'key_up', 'key_code': 0},
                                       {'kind': 'key_down', 'key_code': 0}])
        self.assertEqual(result['unpaired'], {'key:0': 1})

    def test_menu_extra_key_up_is_allowed_but_owned_modifier_needs_release(self):
        down = {'kind': 'flags_changed', 'key_code': 55, 'flags': 0x100000}
        menu_up = {'kind': 'key_up', 'key_code': 13, 'flags': 0x100000}
        release = {'kind': 'flags_changed', 'key_code': 55, 'flags': 0}
        self.assertTrue(probe.observed_pairs([down, menu_up])['unpaired'])
        self.assertEqual(probe.observed_pairs([down, menu_up, release])['unpaired'], {})

    def test_missing_owned_mouse_up_is_detected(self):
        self.assertEqual(probe.observed_pairs([{'kind': 'left_down'}])['unpaired'], {'left': 1})

    def test_unrelated_peer_text_is_allowed_but_unique_marker_leak_fails(self):
        fixture = SimpleNamespace(state=lambda: {'pid': 7, 'text': 'User typed unrelated text', 'saved': 'another task'})
        probe.no_marker_leak('OWNED-5fa9', (fixture,), {})
        fixture.state = lambda: {'pid': 7, 'text': 'User text OWNED-5fa9'}
        with self.assertRaisesRegex(AssertionError, 'probe_marker_reached_wrong_app'):
            probe.no_marker_leak('OWNED-5fa9', (fixture,), {})


class FocusOwnershipEvidenceTests(unittest.TestCase):
    def verify(self, pids, final_pid, sampling='complete', final_window=91):
        before = {'frontmost_pid': 4, 'key_window': 41}
        target = SimpleNamespace(child=SimpleNamespace(pid=9))
        rows = [{'uptime': i, 'frontmost_pid': pid} for i, pid in enumerate(pids)]
        desktop = {'samples': rows, 'activations': [], 'sampling_coverage': {'status':sampling}}
        final = {'pid':9,'frontmost_pid':final_pid,'key_window':final_window,'windows':[{'id':91}]}
        with patch.object(probe, 'fresh_end', return_value=final), \
             patch.object(probe, 'observe_desktop', return_value=desktop):
            return probe.focus_return_coverage(object(), before, target, {})

    def test_target_retained_is_required_after_controlled_focus(self):
        self.assertEqual(self.verify([4, 9, 9], 9)['status'], 'established')
        with self.assertRaisesRegex(AssertionError, 'exact window'):
            self.verify([4, 9, 4], 4)

    def test_same_app_wrong_window_is_not_retention(self):
        with self.assertRaisesRegex(AssertionError, 'exact window'):
            self.verify([4, 9], 9, final_window=92)

    def test_sampling_gap_does_not_claim_retention(self):
        self.assertEqual(self.verify([9], 9, 'not_established')['status'], 'not_established')

    def test_external_takeover_does_not_require_stealing_focus_back(self):
        result = self.verify([9, 7], 7)
        self.assertEqual(result['status'], 'not_established')
        self.assertEqual(result['external_foreground_observations'][0]['frontmost_pid'], 7)

    def test_missing_actual_selection_is_not_silently_accepted(self):
        with self.assertRaisesRegex(AssertionError, 'never independently reached'):
            self.verify([4], 4)


class FocusRetentionReceiptTests(unittest.TestCase):
    def proof(self, native_return, borrowed=True):
        delivery = {'route':'foreground','foreground_borrowed':borrowed,
                    'focus_return':'pending' if borrowed else None}
        proof = {'record':{'script':{'focus_return':native_return,'steps':[
            {'operation':'focus','receipt':{'provider_outcome':{'delivery':delivery}}}]}}}
        probe.focus_receipt_evidence(proof, 1)
        return proof

    def test_retained_receipt_preserves_original_pending_step(self):
        proof = self.proof('retained')
        self.assertTrue(proof['native_retention_evidence']['target_retained_reported'])
        self.assertEqual(proof['record']['script']['steps'][0]['receipt']['provider_outcome']
                         ['delivery']['focus_return'], 'pending')

    def test_already_selected_window_does_not_require_borrow(self):
        self.proof('retained', borrowed=False)

    def test_external_change_is_not_claimed_as_target_retention(self):
        proof = self.proof('changed_externally')
        self.assertFalse(proof['native_retention_evidence']['target_retained_reported'])

    def test_old_restore_policy_and_missing_finalizer_are_rejected(self):
        for returned in [None,'pending','success','restored','application_restored']:
            with self.subTest(returned=returned), self.assertRaisesRegex(AssertionError, 'terminal retention evidence'):
                self.proof(returned)


class SameProcessSelectedWindowTests(unittest.TestCase):
    def verify(self, final_window, timeout=False):
        before = {'pid':9,'frontmost_pid':9,'key_window':91,
                  'windows':[{'id':91},{'id':92}], 'published_uptime':1}
        after = dict(before, key_window=final_window, published_uptime=3)
        target = SimpleNamespace(child=SimpleNamespace(pid=9), app_id='test.app')
        record = {'status':'completed','error':True,'script':{'failure':{'kind':'timeout'},
                  'steps':[{'operation':'windows'},{'operation':'focus'}]}}
        mcp = SimpleNamespace(execute=lambda *args:(record, {}))
        with patch.object(probe, 'reset'), patch.object(probe, 'front', return_value=before), \
             patch.object(probe, 'execute'), patch.object(probe, 'fresh_end', return_value=after), \
             patch.object(probe, 'journal', return_value=[{'uptime':2,'frontmost_pid':9,'key_window':92}]):
            run = probe.timed_out_focus_return if timeout else probe.same_pid_focus_return
            run(mcp, target, {})

    def test_selected_sibling_stays_foreground_after_success_and_timeout(self):
        for timeout in (False, True):
            with self.subTest(timeout=timeout): self.verify(92, timeout)

    def test_restoring_original_sibling_is_a_regression(self):
        for timeout in (False, True):
            with self.subTest(timeout=timeout), self.assertRaisesRegex(AssertionError, 'exact window'):
                self.verify(91, timeout)


class CloseReceiptTests(unittest.TestCase):
    def record(self, message):
        error = {'code':'background_input_failed', 'message':message,
                 'details':{'provider_code':'background_input_failed'}}
        outcome = {'dispatch':'unknown','error':error}
        return {'app':'owned','app_ref':'same','status':'completed','context_reset':False,
                'state_reset':False,'error':{'code':'script_exception','message':'keypress failed: '+message},
                'script':{'state_committed':False,'shutdown_error':None,
                          'failure':{'kind':'exception','operation':'keypress','code':'background_input_failed','call':1},
                          'steps':[{'number':1,'operation':'keypress','status':'completed','error':error,
                                    'receipt':{'interruption':None,'outcome':outcome,'provider_outcome':outcome}}]}}

    def test_closed_retained_ax_object_keeps_unknown(self):
        value = self.record('ax_unavailable: AX attribute AXModal failed: -25202')
        self.assertEqual(probe.close_record_outcome(value,'owned','same'), 'target_changed_unknown')
        self.assertEqual(value['script']['steps'][0]['receipt']['outcome']['dispatch'], 'unknown')

    def test_busy_or_other_ax_failures_are_not_window_close_proof(self):
        for message in ['ax_unavailable: AX attribute AXModal failed: -25204',
                        'ax_unavailable: AX attribute AXChildren failed: -25202']:
            with self.assertRaisesRegex(AssertionError, 'other than the exact'):
                probe.close_record_outcome(self.record(message),'owned','same')


if __name__ == '__main__':
    unittest.main()
