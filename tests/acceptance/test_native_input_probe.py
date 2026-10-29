#!/usr/bin/env python3
"""Pure release-sampling tests; no native process, input, or GUI is started."""
import copy
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import native_input_probe as probe


def sample(uptime, keys=None, buttons=None, flags=536870912):
    return {'published_uptime':uptime, 'keys_down_global':keys or [],
            'buttons_down_global':buttons or [], 'modifier_flags_global':flags,
            'events_count':259, 'text':'CANCEL-partial'}


class Clock:
    def __init__(self): self.now = 0
    def monotonic(self): return self.now
    def sleep(self, seconds): self.now += seconds


class Samples:
    def __init__(self, values):
        self.values = values
        self.reads = 0

    def state(self):
        index = min(self.reads, len(self.values)-1)
        self.reads += 1
        return copy.deepcopy(self.values[index])


class ReleaseTests(unittest.TestCase):
    def run_release(self, values, proof):
        clock = Clock()
        with patch.object(probe.time, 'monotonic', clock.monotonic), patch.object(probe.time, 'sleep', clock.sleep):
            return probe.released(Samples(values), proof, timeout=0.3)

    def test_repeated_old_snapshot_cannot_prove_a_stable_release(self):
        proof = {}
        with self.assertRaisesRegex(AssertionError, 'fresh, continuous 100 ms'):
            self.run_release([sample(10)], proof)
        self.assertFalse(proof['released'])
        self.assertEqual(proof['fresh_samples'], 0)
        self.assertIsNone(proof['first_stable'])
        self.assertEqual(proof['baseline'], proof['last_sample'])

    def test_success_retains_distinct_post_baseline_first_and_last_samples(self):
        proof = {}
        final = self.run_release([sample(10+i*0.02) for i in range(9)], proof)
        self.assertTrue(proof['released'])
        self.assertGreater(proof['first_stable']['published_uptime'], proof['baseline']['published_uptime'])
        self.assertGreaterEqual(proof['stable_seconds'], 0.1)
        self.assertEqual(proof['last_stable'], final)
        self.assertGreaterEqual(proof['stable_samples'], 6)
        # Non-coalesced is a flag, not a held modifier.
        self.assertEqual(final['modifier_flags_global'], 536870912)

    def test_late_keydown_interrupts_false_initial_release_and_is_retained(self):
        proof = {}
        held = sample(10.08, keys=[0]); held['events_count'] = 264
        with self.assertRaises(AssertionError):
            self.run_release([sample(10), sample(10.02), sample(10.04), held], proof)
        self.assertFalse(proof['released'])
        self.assertEqual(proof['last_sample'], held)
        self.assertEqual(proof['interrupted_intervals'][0]['reason'], 'held_input')
        self.assertEqual(proof['interrupted_intervals'][0]['first_sample'], sample(10.02))
        self.assertEqual(proof['interrupted_intervals'][0]['last_sample'], sample(10.04))
        self.assertIsNone(proof['first_stable'])
        self.assertIsNone(proof['last_stable'])

    def test_release_interval_restarts_after_held_input_or_missing_publications(self):
        proof = {}
        values = [sample(10), sample(10.02), sample(10.04, keys=[0])]
        values += [sample(10.06+i*0.02) for i in range(9)]
        self.run_release(values, proof)
        self.assertTrue(proof['released'])
        self.assertEqual(proof['first_stable']['published_uptime'], 10.06)
        gap = {}
        with self.assertRaises(AssertionError):
            self.run_release([sample(10), sample(10.02), sample(10.04), sample(10.2)], gap)
        self.assertEqual(gap['interrupted_intervals'][0]['reason'], 'publish_gap')
        self.assertEqual(gap['stable_samples'], 1)

    def test_buttons_and_actual_modifiers_must_also_be_released(self):
        held = [sample(10.02, buttons=[0]), sample(10.02, buttons=[1])]
        held += [sample(10.02, flags=536870912|flag) for flag in (0x20000,0x40000,0x80000,0x100000)]
        for state in held:
            with self.subTest(state=state), self.assertRaises(AssertionError):
                self.run_release([sample(10), state], {})
        self.assertTrue(probe.input_is_released(sample(10, flags=536870912|0x10000)))

    def test_missing_invalid_or_reversed_publication_cannot_confirm_release(self):
        for value in (None, True, float('nan'), float('inf'), 0, -1):
            with self.subTest(value=value), self.assertRaises(AssertionError):
                self.run_release([sample(value)], {})
        with self.assertRaisesRegex(AssertionError, 'moved backwards'):
            self.run_release([sample(10), sample(9.99)], {})

    def test_event_journal_is_read_after_release_sampling_not_before(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            journal = root/'events.jsonl'; journal.write_text('')
            class JournalFixture(Samples):
                def state(self):
                    result = super().state()
                    if self.reads == 3:
                        journal.write_text(json.dumps({'kind':'key_down','key_code':0})+'\n')
                    return result
            fixture = JournalFixture([sample(10+i*0.02) for i in range(9)]); fixture.root = root
            proof = {}; clock = Clock()
            with patch.object(probe.time, 'monotonic', clock.monotonic), patch.object(probe.time, 'sleep', clock.sleep):
                with self.assertRaises(AssertionError): probe.event_pairs(fixture, proof)
            self.assertTrue(proof['release']['released'])
            self.assertEqual(proof['keydown'], {'0':1})
            self.assertEqual(proof['keyup'], {})
            self.assertEqual(proof['events_count'], 1)


class FakePipe:
    def __init__(self): self.closed = False
    def close(self): self.closed = True


class OwnedChild:
    def __init__(self):
        self.pid = 900
        self.stdin = FakePipe()
        self.returncode = None
        self.terminations = 0
    def poll(self): return self.returncode
    def terminate(self):
        self.terminations += 1
        self.returncode = -probe.signal.SIGTERM


class FocusInterruptionTests(unittest.TestCase):
    def record(self):
        return {'error':{'code':'script_exception'}, 'script':{
            'failure':{'code':'focus_changed'}, 'state_committed':False,
            'steps':[{'operation':'typeText', 'error':{'code':'focus_changed'},
                      'receipt':{'outcome':{'dispatch':'unknown'},
                                 'provider_outcome':{'dispatch':'unknown'}}}]}}

    def test_original_focus_error_and_unknown_dispatch_are_required(self):
        record = self.record(); probe.assert_focus_interruption(record)
        bad = copy.deepcopy(record); bad['error'] = None
        with self.assertRaises(AssertionError): probe.assert_focus_interruption(bad)
        for key in ('outcome','provider_outcome'):
            bad = copy.deepcopy(record); bad['script']['steps'][0]['receipt'][key]['dispatch'] = 'dispatched'
            with self.assertRaises(AssertionError): probe.assert_focus_interruption(bad)

    def test_save_suffix_or_state_commit_invalidates_interruption(self):
        bad = self.record(); bad['script']['steps'].append({'operation':'keypress'})
        with self.assertRaises(AssertionError): probe.assert_focus_interruption(bad)
        bad = self.record(); bad['script']['state_committed'] = True
        with self.assertRaises(AssertionError): probe.assert_focus_interruption(bad)


class ExitMcp:
    decode = staticmethod(probe.Mcp.decode)
    def __init__(self, root):
        self.child = OwnedChild()
        self.output = root
        self.begin_calls = []
        self.close_calls = 0
        (root/'mcp.jsonl').write_text('')
        (root/'dtw-stderr.log').write_text('')
    def begin(self, method, params):
        self.begin_calls.append((method, params))
        return 1
    def receipt(self, app, case):
        return {'app':app, 'execution_id':'execution-original', 'status':'running'}
    def close(self):
        self.close_calls += 1
        self.child.stdin.close()
        if self.child.returncode is None: self.child.returncode = 0
        return {'pid':self.child.pid, 'returncode':self.child.returncode, 'forced':[],
                'native_workers_reaped':True}


class ExitFixture:
    app_id = 'dev.dtw.owned'
    def __init__(self, text): self.text = text
    def command(self, command):
        if command != 'reset': raise AssertionError('Unexpected fixture command')
        return {'text':''}
    def state(self): return {'text':self.text}


class TransportExitTests(unittest.TestCase):
    def setUp(self):
        environment = patch.dict(probe.os.environ, {'DTW_AX_READINESS_TRACE':'1'})
        environment.start(); self.addCleanup(environment.stop)

    def test_each_exit_mode_dispatches_once_and_retains_unavailable_ledger(self):
        for case in ('eof', 'parent-exit'):
            with self.subTest(case=case), tempfile.TemporaryDirectory() as directory:
                mcp = ExitMcp(Path(directory)); evidence = {}
                payload = case.upper()+'-中文😀-'+'q'*1000
                target = ExitFixture(payload[:18]); peer = ExitFixture('untouched')
                def release(fixture, proof):
                    proof.update(released=True, stable_seconds=0.12)
                    return {'text':payload[:30]}
                tables = [{901:(900, 'owned --dtw-native-worker'),
                           902:(888, 'foreign --dtw-native-worker')}, {}]
                with patch.object(probe, 'process_table', side_effect=tables), patch.object(probe, 'released', side_effect=release):
                    probe.transport_exit(mcp, target, peer, evidence, case)
                proof = evidence[case]
                self.assertEqual(len(mcp.begin_calls), 1)
                self.assertEqual(proof['dispatch_attempts'], 1)
                self.assertFalse(proof['replayed'])
                self.assertEqual(proof['execution_id'], 'execution-original')
                self.assertEqual(proof['running']['status'], 'running')
                self.assertEqual(proof['final_receipt']['availability'], 'unavailable')
                self.assertEqual(proof['final_receipt']['ledger'], 'unavailable')
                self.assertNotIn('record', proof['final_receipt'])
                self.assertEqual([w['pid'] for w in proof['native_workers']], [901])
                self.assertIsNone(proof['native_workers'][0]['returncode'])
                self.assertEqual(proof['native_workers'][0]['exit_status'], 'unavailable')
                self.assertTrue(proof['mcp_close']['recorded_worker_exit']['exited'])
                diagnostics = proof['mcp_close']['native_worker_stderr']
                self.assertTrue(diagnostics['checked_after_recorded_workers_exited'])
                self.assertEqual(diagnostics['cleanup_failure_marker_lines'], [])
                self.assertEqual(mcp.child.terminations, int(case == 'parent-exit'))
                self.assertEqual(proof['peer_after'], proof['peer_before'])
                # Normal finally may recheck workers, but does not close/join a
                # second time or dispatch input again.
                with patch.object(probe, 'process_table', return_value={}): probe.close_mcp(mcp)
                self.assertEqual(mcp.close_calls, 1)
                self.assertEqual(len(mcp.begin_calls), 1)

    def test_reparented_worker_is_not_mistaken_for_an_exited_worker(self):
        with tempfile.TemporaryDirectory() as directory:
            mcp = ExitMcp(Path(directory))
            mcp.probe_native_workers = {901:'owned --dtw-native-worker'}
            mcp.probe_require_stderr_diagnostics = True
            def once(predicate, timeout):
                if predicate(): return True
                raise AssertionError('Recorded worker remains alive')
            with patch.object(probe, 'process_table', return_value={901:(1, 'owned --dtw-native-worker')}), \
                    patch.object(probe, 'wait_until', side_effect=once):
                with self.assertRaises(AssertionError): probe.close_mcp(mcp)
            self.assertFalse(mcp.probe_close_result['recorded_worker_exit']['exited'])
            self.assertEqual(mcp.probe_close_result['recorded_worker_exit']['remaining'], [901])
            self.assertNotIn('native_worker_stderr', mcp.probe_close_result)

    def test_missing_trace_is_rejected_before_any_input_or_fixture_command(self):
        with tempfile.TemporaryDirectory() as directory:
            mcp = ExitMcp(Path(directory)); evidence = {}
            target = ExitFixture('unused'); peer = ExitFixture('untouched')
            with patch.dict(probe.os.environ, {'DTW_AX_READINESS_TRACE':'0'}), \
                    patch.object(target, 'command') as command:
                with self.assertRaisesRegex(AssertionError, 'before input dispatch'):
                    probe.transport_exit(mcp, target, peer, evidence, 'parent-exit')
                command.assert_not_called()
            self.assertFalse(evidence['parent-exit']['native_worker_stderr_trace']['enabled'])
            self.assertEqual(mcp.begin_calls, [])
            self.assertEqual(mcp.child.terminations, 0)

    def test_exact_cleanup_marker_fails_after_worker_disappearance_even_if_released(self):
        with tempfile.TemporaryDirectory() as directory:
            mcp = ExitMcp(Path(directory)); evidence = {}
            payload = 'PARENT-EXIT-中文😀-'+'q'*1000
            target = ExitFixture(payload[:18]); peer = ExitFixture('untouched')
            original_close = mcp.close
            def close_with_marker():
                result = original_close()
                (mcp.output/'dtw-stderr.log').write_bytes(b'other trace\n'+probe.NATIVE_CLEANUP_FAILURE+b'\n')
                return result
            with patch.object(probe, 'process_table', side_effect=[{901:(900, 'owned --dtw-native-worker')}, {}]), \
                    patch.object(mcp, 'close', side_effect=close_with_marker), \
                    patch.object(probe, 'released', return_value={'text':payload[:30]}):
                with self.assertRaisesRegex(AssertionError, 'incomplete input cleanup'):
                    probe.transport_exit(mcp, target, peer, evidence, 'parent-exit')
            proof = evidence['parent-exit']
            self.assertEqual(proof['errors'][0]['phase'], 'close')
            self.assertEqual(proof['mcp_close']['native_worker_stderr']['cleanup_failure_marker_lines'], [2])
            self.assertEqual(proof['after']['text'], payload[:30])
            self.assertEqual(len(mcp.begin_calls), 1)

    def test_stderr_must_exist_and_similar_trace_text_is_not_the_exact_failure_marker(self):
        with tempfile.TemporaryDirectory() as directory:
            mcp = ExitMcp(Path(directory))
            mcp.probe_native_workers = {901:'owned --dtw-native-worker'}
            mcp.probe_require_stderr_diagnostics = True
            path = mcp.output/'dtw-stderr.log'
            path.write_bytes(probe.NATIVE_CLEANUP_FAILURE+b' (example text)\n')
            with patch.object(probe, 'process_table', return_value={}):
                result = probe.close_mcp(mcp)
                self.assertEqual(result['native_worker_stderr']['cleanup_failure_marker_lines'], [])
                path.unlink()
                with self.assertRaises(FileNotFoundError): probe.close_mcp(mcp)

    def test_cancel_rejects_partial_text_that_is_not_the_original_payload_prefix(self):
        with tempfile.TemporaryDirectory() as directory:
            mcp = ExitMcp(Path(directory)); evidence = {}
            target = ExitFixture('CANCEL-'+'q'*15); peer = ExitFixture('untouched')
            running = {'app':target.app_id,'execution_id':'execution-original','status':'running'}
            terminal = {'app':target.app_id,'execution_id':'execution-original','status':'completed',
                        'error':{'code':'cancelled'}, 'script':{'failure':{'kind':'cancelled'}, 'steps':[
                            {'operation':'typeText','receipt':{'outcome':{'dispatch':'unknown'},'provider_outcome':{}}}]}}
            with patch.object(mcp, 'receipt', side_effect=[running, terminal]), \
                    patch.object(mcp, 'notify', create=True) as notify, \
                    patch.object(probe, 'released', return_value={'text':'foreign text'}):
                with self.assertRaisesRegex(AssertionError, 'changed, replayed'):
                    probe.cancel(mcp, target, peer, evidence)
            self.assertEqual(evidence['cancellation']['record'], terminal)
            self.assertEqual(evidence['cancellation']['after']['text'], 'foreign text')
            self.assertEqual(len(mcp.begin_calls), 1)
            notify.assert_called_once()

    def test_completed_input_or_changed_peer_fails_without_replay(self):
        for changed in ('complete', 'peer'):
            with self.subTest(changed=changed), tempfile.TemporaryDirectory() as directory:
                mcp = ExitMcp(Path(directory)); evidence = {}
                payload = 'EOF-中文😀-'+'q'*1000
                target = ExitFixture(payload[:18]); peer = ExitFixture('untouched')
                after = {'text':payload if changed == 'complete' else payload[:30]}
                peer_after = {'text':'foreign input' if changed == 'peer' else 'untouched'}
                with patch.object(probe, 'process_table', side_effect=[{901:(900, 'owned --dtw-native-worker')}, {}]), \
                        patch.object(probe, 'released', return_value=after), \
                        patch.object(peer, 'state', side_effect=[{'text':'untouched'}, peer_after]):
                    with self.assertRaises(AssertionError):
                        probe.transport_exit(mcp, target, peer, evidence, 'eof')
                self.assertEqual(evidence['eof']['errors'][0]['phase'], 'independent_release')
                self.assertEqual(evidence['eof']['after'], after)
                self.assertEqual(evidence['eof']['peer_after'], peer_after)
                self.assertEqual(len(mcp.begin_calls), 1)

    def test_missed_inflight_interval_does_not_terminate_or_retry(self):
        with tempfile.TemporaryDirectory() as directory:
            mcp = ExitMcp(Path(directory)); evidence = {}
            target = ExitFixture('PARENT-EXIT-中文😀-qqqqq'); peer = ExitFixture('untouched')
            completed = {'app':target.app_id,'execution_id':'execution-original','status':'completed'}
            with patch.object(mcp, 'receipt', return_value=completed), patch.object(probe, 'process_table') as processes:
                with self.assertRaisesRegex(AssertionError, 'no retry'):
                    probe.transport_exit(mcp, target, peer, evidence, 'parent-exit')
                processes.assert_not_called()
            self.assertEqual(evidence['parent-exit']['running'], completed)
            self.assertEqual(mcp.child.terminations, 0)
            self.assertEqual(len(mcp.begin_calls), 1)

    def test_expected_signal_only_applies_to_exact_parent_and_never_hides_cleanup_failure(self):
        mcp = probe.Mcp.__new__(probe.Mcp)
        mcp.child = OwnedChild()
        mcp.probe_expected_exit_child = mcp.child
        mcp.probe_expected_exit = {'pid':900, 'returncode':-probe.signal.SIGTERM, 'sent':True}
        result = {'pid':900, 'returncode':-probe.signal.SIGTERM, 'forced':[], 'native_workers_reaped':True}
        probe.check_cleanup(mcp, result)
        for changed in ({'forced':['kill']}, {'pid':999}, {'returncode':0}, {'native_workers_reaped':False}):
            with self.assertRaises(AssertionError): probe.check_cleanup(mcp, {**result, **changed})
        with self.assertRaises(AssertionError): probe.check_cleanup(SimpleNamespace(), result)
        mcp.probe_expected_exit_child = OwnedChild()
        with self.assertRaises(AssertionError): probe.check_cleanup(mcp, result)

    def test_cleanup_failure_is_retained_even_when_independent_release_passes(self):
        with tempfile.TemporaryDirectory() as directory:
            mcp = ExitMcp(Path(directory)); evidence = {}
            payload = 'PARENT-EXIT-中文😀-'+'q'*1000
            target = ExitFixture(payload[:18]); peer = ExitFixture('untouched')
            def failed_close():
                mcp.probe_close_result = {'pid':900, 'returncode':-9, 'forced':['kill'], 'native_workers_reaped':True}
                return mcp.probe_close_result
            with patch.object(probe, 'process_table', side_effect=[{901:(900, 'owned --dtw-native-worker')}, {}]), \
                    patch.object(mcp, 'close', side_effect=failed_close), \
                    patch.object(probe, 'released', return_value={'text':payload[:30]}):
                with self.assertRaises(AssertionError):
                    probe.transport_exit(mcp, target, peer, evidence, 'parent-exit')
            proof = evidence['parent-exit']
            self.assertEqual(proof['errors'][0]['phase'], 'close')
            self.assertEqual(proof['mcp_close']['forced'], ['kill'])
            self.assertEqual(proof['after']['text'], payload[:30])
            self.assertEqual(len(mcp.begin_calls), 1)

    def test_retained_response_never_becomes_a_fabricated_full_ledger(self):
        with tempfile.TemporaryDirectory() as directory:
            mcp = ExitMcp(Path(directory))
            record = {'execution_id':'execution-original', 'status':'completed',
                      'error':{'code':'cancelled'}, 'script':{'state_committed':False}}
            response = {'id':1, 'result':{'structuredContent':record,
                        'content':[{'type':'text','text':json.dumps(record)}]}}
            (mcp.output/'mcp.jsonl').write_text(json.dumps({'direction':'response','message':response})+'\n')
            proof = {'execution_id':'execution-original', 'rpc_id':1}
            probe.retained_exec_response(mcp, proof)
            self.assertEqual(proof['final_receipt']['ledger'], 'unavailable')
            self.assertEqual(proof['final_receipt']['record'], record)
            self.assertEqual(proof['final_receipt']['response'], response)
            record['shutdown_error'] = {'code':'cleanup_failed'}
            response['result']['content'][0]['text'] = json.dumps(record)
            (mcp.output/'mcp.jsonl').write_text(json.dumps({'direction':'response','message':response})+'\n')
            with self.assertRaises(AssertionError): probe.retained_exec_response(mcp, proof)
            self.assertEqual(proof['final_receipt']['record']['shutdown_error'], record['shutdown_error'])


if __name__ == '__main__': unittest.main()
