#!/usr/bin/env python3
"""Pure transcript/cleanup tests. No Agent, MCP, native process, or GUI is started."""
import base64
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import browser_workbench_probe as probe


APP = 'com.google.Chrome'
NAME = 'owned-test'


def completed():
    return {'version': 1, 'app': APP, 'execution_id': 'execution-test',
            'status': 'completed', 'error': None, 'payload_expired': False,
            'value': {'result': '核对 🐦', 'last_capture': None},
            'script': {'state_committed': True, 'failure': None, 'shutdown_error': None,
                       'summary': {'steps': 1, 'failed': 0, 'pending': 0}}}


def call(output, images=0):
    return {'name': f'mcp__{NAME}__exec', 'arguments': {'app': APP, 'source': 'return true;'},
            'output': output, 'status': 'completed', 'image_bytes': images}


def measure(output, images=0):
    return probe.metrics([call(output, images)], NAME)


class InstalledPackageTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.package = self.root/'candidate'
        self.package.mkdir()
        self.output = self.root/'run'; self.output.mkdir()
        self.repo = self.root/'different-development-tree'
        self.repo.mkdir()
        self.binary = self.put('bin/dtw', b'candidate executable')
        self.binary.chmod(0o755)
        self.canonical_skill = '---\nname: desktop-world\ndescription: Candidate only\n---\nCandidate instructions.\n'
        self.put('skills/desktop-world/SKILL.md', self.canonical_skill)
        self.put('skills/desktop-world/support.txt', 'Package-owned resource')
        self.put('.claude-plugin/plugin.json', json.dumps({'name':'desktop-world','version':'0.2.0','description':'Package metadata'}))
        self.build = {'schema':1, 'kind':'unpublished-rewrite-candidate', 'version':'0.2.0',
                      'target':'aarch64-apple-darwin', 'binary':{'path':'bin/dtw','sha256':probe.file_sha256(self.binary)},
                      'skill_sha256':probe.file_sha256(self.package/'skills/desktop-world/SKILL.md'),
                      'source':{'commit':'a'*40,'tree':'b'*40,'cargo_lock_sha256':'c'*64},
                      'source_archive_sha256':'d'*64}
        self.put('BUILD.json', json.dumps(self.build))
        self.checksums()

    def put(self, name, content):
        path = self.package/name; path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content if isinstance(content, bytes) else content.encode())
        return path

    def checksums(self):
        files = sorted(path for path in self.package.rglob('*') if path.is_file() and path.name != 'SHA256SUMS')
        self.put('SHA256SUMS', ''.join(hashlib.sha256(path.read_bytes()).hexdigest()+'  '
                                    +path.relative_to(self.package).as_posix()+'\n' for path in files))

    def test_installed_runtime_and_skill_use_package_without_copying_binary_or_developer_stage(self):
        with patch.object(probe.subprocess, 'run', side_effect=AssertionError('No developer staging subprocess')):
            binary, identity = probe.prepare_runtime(self.repo, self.output, self.binary, self.package)
            stage, skill = probe.stage_agent_plugin(self.repo, self.output, binary, NAME, self.package)
        self.assertEqual(binary, self.binary.resolve())
        self.assertFalse((self.output/'dtw').exists())
        self.assertFalse((stage/'bin').exists())
        self.assertFalse((stage/'.mcp.json').exists())
        self.assertEqual(skill.read_text(), self.canonical_skill.replace('name: desktop-world','name: '+NAME))
        self.assertEqual((skill.parent/'support.txt').read_text(), 'Package-owned resource')
        manifest = json.loads((stage/'.claude-plugin/plugin.json').read_text())
        self.assertEqual(manifest, {'name':NAME,'version':'0.2.0','description':'Package metadata'})
        self.assertEqual((self.package/'skills/desktop-world/SKILL.md').read_text(), self.canonical_skill)
        self.assertEqual((self.output/'installed-BUILD.json').read_bytes(), (self.package/'BUILD.json').read_bytes())
        self.assertEqual((self.output/'installed-SHA256SUMS').read_bytes(), (self.package/'SHA256SUMS').read_bytes())
        self.assertTrue(identity['installed_artifact'])
        self.assertFalse(identity['release_artifact'])
        self.assertEqual(identity['artifact_kind'], 'unpublished-rewrite-candidate')
        self.assertEqual(identity['installed_package']['source'], self.build['source'])
        self.assertTrue(probe.recheck_installed_package(identity)['unchanged'])

    def test_same_bytes_at_another_path_and_output_inside_package_are_rejected(self):
        other = self.root/'same-dtw'; other.write_bytes(self.binary.read_bytes()); other.chmod(0o755)
        with self.assertRaisesRegex(AssertionError, '--dtw'):
            probe.verify_installed_package(self.package, other)
        output = self.package/'run'; output.mkdir()
        with self.assertRaisesRegex(AssertionError, 'outside'):
            probe.prepare_runtime(self.repo, output, self.binary, self.package)
        self.assertEqual(list(output.iterdir()), [])

    def test_development_mode_still_copies_binary_and_uses_repository_skill(self):
        skill = self.repo/'skills/desktop-world/SKILL.md'
        skill.parent.mkdir(parents=True); skill.write_text('Different development Skill')
        binary, identity = probe.prepare_runtime(self.repo, self.output, self.binary)
        self.assertEqual(binary, self.output/'dtw')
        self.assertEqual(binary.read_bytes(), self.binary.read_bytes())
        self.assertFalse(identity['installed_artifact'])
        self.assertEqual(identity['skill_sha256'], probe.file_sha256(skill))
        self.assertNotEqual(identity['skill_sha256'], self.build['skill_sha256'])

    def test_candidate_mode_does_not_claim_an_arbitrary_manifest_is_a_release(self):
        self.build['kind'] = 'release'
        self.put('BUILD.json', json.dumps(self.build)); self.checksums()
        with self.assertRaisesRegex(AssertionError, 'unpublished rewrite candidate'):
            probe.verify_installed_package(self.package, self.binary)

    def test_changed_bytes_and_unlisted_or_missing_files_are_rejected(self):
        original = self.binary.read_bytes()
        self.binary.write_bytes(b'modified executable')
        with self.assertRaisesRegex(AssertionError, 'hash mismatch'):
            probe.verify_installed_package(self.package, self.binary)
        self.binary.write_bytes(original)
        extra = self.put('unlisted.txt', 'Unexpected file')
        with self.assertRaisesRegex(AssertionError, 'inventory'):
            probe.verify_installed_package(self.package, self.binary)
        extra.unlink()
        support = self.package/'skills/desktop-world/support.txt'; support.unlink()
        with self.assertRaisesRegex(AssertionError, 'inventory'):
            probe.verify_installed_package(self.package, self.binary)

    def test_recomputed_checksums_do_not_bypass_build_binary_or_skill_identity(self):
        for key, file in (('binary','bin/dtw'), ('Skill','skills/desktop-world/SKILL.md')):
            with self.subTest(asset=key):
                path = self.package/file; original = path.read_bytes()
                path.write_bytes(original+b'changed'); self.checksums()
                with self.assertRaisesRegex(AssertionError, 'BUILD '+key):
                    probe.verify_installed_package(self.package, self.binary)
                path.write_bytes(original); self.checksums()

    def test_final_recheck_rejects_consistently_rewritten_inventory_and_keeps_initial_evidence(self):
        _, identity = probe.prepare_runtime(self.repo, self.output, self.binary, self.package)
        original_sums = (self.output/'installed-SHA256SUMS').read_bytes()
        self.put('skills/desktop-world/support.txt', 'Changed after acceptance'); self.checksums()
        # The new package is internally consistent but is not the one admitted.
        probe.verify_installed_package(self.package, self.binary)
        evidence = {'passed':True,'business_passed':True}
        probe.cleanup_steps(evidence, [('installed_package_integrity', lambda:probe.recheck_installed_package(identity), None)])
        self.assertFalse(evidence['passed'])
        self.assertIn('identity changed', evidence['cleanup'][0]['error']['message'])
        self.assertEqual((self.output/'installed-SHA256SUMS').read_bytes(), original_sums)

    def test_symlink_and_duplicate_or_unsafe_manifest_paths_are_rejected(self):
        checksums = (self.package/'SHA256SUMS').read_text()
        for appended in (checksums.splitlines()[0]+'\n', 'e'*64+'  ../outside\n'):
            self.put('SHA256SUMS', checksums+appended)
            with self.assertRaises(AssertionError):
                probe.verify_installed_package(self.package, self.binary)
        self.put('SHA256SUMS', checksums)
        (self.package/'alias').symlink_to(self.binary)
        with self.assertRaisesRegex(AssertionError, 'symlink'):
            probe.verify_installed_package(self.package, self.binary)


class RouteTests(unittest.TestCase):
    def test_skill_loading_requires_exact_identity_success_and_observed_acknowledgement(self):
        root = Path('/tmp/owned-case')
        skill = {'name':'Skill', 'arguments':{'skill':NAME}, 'status':'completed',
                 'output':'Launching skill: '+NAME}
        probe.check_skill_loaded([skill], NAME, root)
        for path in probe.skill_paths(NAME, root):
            read = {'name':'read_file', 'arguments':{'Path':path}, 'status':'DONE',
                    'output':'125 lines, 13019 bytes'}
            probe.check_routes([read], NAME, root)
            probe.check_skill_loaded([read], NAME, root)
            for changed in ({'status':'ERROR'}, {'status':'RUNNING'}, {'output':None},
                            {'output':'0 lines, 0 bytes'}, {'output':'Error reading file'}):
                with self.assertRaises(AssertionError):
                    probe.check_skill_loaded([{**read, **changed}], NAME, root)
        for changed in ({'status':'failed'}, {'status':None}, {'output':'Unknown skill'},
                        {'arguments':{'skill':'other'}}, {'output':'Launching skill: other'}):
            with self.assertRaises(AssertionError):
                probe.check_skill_loaded([{**skill, **changed}], NAME, root)
        metadata = Path.home()/'.gemini/antigravity-cli/mcp'/NAME/'instructions.md'
        decoy = {'name':'read_file', 'arguments':{'Path':str(metadata),'description':'load skill'},
                 'status':'DONE', 'output':'125 lines, 13019 bytes'}
        probe.check_routes([decoy], NAME, root)
        with self.assertRaises(AssertionError): probe.check_skill_loaded([decoy], NAME, root)

    def test_normal_agy_discovery_is_scoped_to_exact_metadata_and_skill(self):
        root = Path('/tmp/owned-case')
        metadata = Path.home()/'.gemini/antigravity-cli/mcp'/NAME
        calls = [{'name':'run_command', 'arguments':{'CommandLine':'ls -la '+str(metadata)}}]
        calls += [{'name':'read_file', 'arguments':{'Path':str(metadata/file)}}
                  for file in ('instructions.md','apps.json','exec.json')]
        calls.append({'name':'Skill','arguments':{'skill':NAME}})
        probe.check_routes(calls, NAME, root)
        for command in ('ls -la '+str(metadata)+'; echo replay',
                        'cat '+str(metadata/'exec.json'), 'ls -la /tmp'):
            with self.assertRaises(AssertionError):
                probe.check_routes([{'name':'run_command','arguments':{'CommandLine':command}}], NAME, root)
        for path in (str(metadata/'../other/exec.json'), '/tmp/'+NAME+'/SKILL.md'):
            with self.assertRaises(AssertionError):
                probe.check_routes([{'name':'read_file','arguments':{'Path':path}}], NAME, root)

    def test_output_read_requires_this_runs_own_mcp_step(self):
        path = '/tmp/owned-output.txt'
        call = {'name':'call_mcp_tool','arguments':{'ServerName':NAME},'owned_output_path':path,'owned_image_paths':['/tmp/owned-media_0.png']}
        read = {'name':'read_file','arguments':{'Path':path}}
        probe.check_routes([call, read, {'name':'read_file','arguments':{'Path':'/tmp/owned-media_0.png'}}], NAME, Path('/tmp/case'))
        with self.assertRaises(AssertionError):
            probe.check_routes([call, {'name':'read_file','arguments':{'Path':'/tmp/owned-media_1.png'}}], NAME, Path('/tmp/case'))
        with self.assertRaises(AssertionError):
            probe.check_routes([read], NAME, Path('/tmp/case'))
        with self.assertRaises(AssertionError):
            probe.check_routes([{'name':'Skill','arguments':{'skill':'another'}}], NAME, Path('/tmp/case'))

    def test_metadata_listing_accepts_directory_slash_without_expanding_shell_access(self):
        root = Path('/tmp/owned-case')
        metadata = str(Path.home()/'.gemini/antigravity-cli/mcp'/NAME)
        with patch.object(probe.Path, 'resolve', side_effect=AssertionError('Must not resolve paths')):
            for prefix in ('ls', 'ls -la'):
                for suffix in ('', '/'):
                    probe.check_routes([{'name':'run_command', 'arguments':{
                        'CommandLine':prefix+' '+metadata+suffix}}], NAME, root)
            for command in ('ls -la '+metadata+'/../'+NAME+'/',
                            'ls -la '+metadata+'/ | cat', 'ls -la '+metadata+'/; echo changed',
                            'ls -la '+metadata+'/ > /tmp/output', 'ls -la '+metadata+'/ /tmp',
                            'ls -la '+metadata+'/ --', 'ls -l '+metadata+'/',
                            'ls -la '+metadata+'-other/', 'ls -la', '',
                            'cat '+metadata+'/exec.json'):
                with self.subTest(command=command), self.assertRaises(AssertionError):
                    probe.check_routes([{'name':'run_command', 'arguments':{'CommandLine':command}}], NAME, root)


class OutputTests(unittest.TestCase):
    def test_incomplete_reads_rejects_nonzero_and_invalid_counters_but_keeps_old_logs(self):
        self.assertEqual(measure(completed())['issues'], [])
        zero = completed(); zero['script']['summary']['incomplete_reads'] = 0
        self.assertEqual(measure(zero)['issues'], [])
        for counter in (1, 12, -1, True, False, 0.0, '0', None, {}, []):
            value = completed(); value['script']['summary']['incomplete_reads'] = counter
            with self.subTest(counter=counter):
                issues = measure(value)['issues']
                self.assertTrue(issues)
                self.assertIn('incomplete', issues[0]['invalid_output'])

    def test_objects_and_strings_count_the_same_compact_utf8_payload(self):
        value = completed()
        expected = len(json.dumps(value, ensure_ascii=False, separators=(',', ':')).encode())
        for output in (value, json.dumps(value, ensure_ascii=True, indent=4)):
            measured = measure(output)
            self.assertEqual(measured['mcp_json_bytes'], expected)
            self.assertEqual(measured['issues'], [])
        value['value']['result'] = 'x' * (41 * 1024)
        self.assertGreater(measure(value)['mcp_json_bytes'], 40 * 1024)

    def test_original_four_false_pass_shapes_are_rejected(self):
        failed = completed(); failed['error'] = {'code': 'cancelled'}
        cleanup = completed(); cleanup['shutdown_error'] = {'code': 'input_cleanup_failed'}
        envelope = {'structuredContent': completed(), 'content': [], 'isError': True}
        for output in (failed, json.dumps(envelope), 'not-json', json.dumps(cleanup)):
            with self.subTest(output_type=type(output).__name__):
                self.assertTrue(measure(output)['issues'])

    def test_missing_summary_and_each_failure_channel_are_rejected(self):
        variants = []
        value = completed(); del value['script']['summary']; variants.append(value)
        value = completed(); value['script']['state_committed'] = False; variants.append(value)
        value = completed(); value['script']['failure'] = {'kind': 'exception'}; variants.append(value)
        value = completed(); value['script']['shutdown_error'] = {'code': 'failed'}; variants.append(value)
        value = completed(); value['shutdown_errors'] = [{'code': 'failed'}]; variants.append(value)
        value = completed(); value['status'] = 'running'; variants.append(value)
        value = completed(); value['execution_unsettled'] = True; variants.append(value)
        for counter in ('failed', 'pending'):
            value = completed(); value['script']['summary'][counter] = 1; variants.append(value)
        for source, kind in (('dispatch', 'unknown'), ('provider_dispatch', 'unknown'),
                             ('evidence', 'mismatch'), ('provider_evidence', 'unrecorded')):
            value = completed()
            actions = {'dispatch': {}, 'provider_dispatch': {}, 'evidence': {},
                       'provider_evidence': {}, 'interrupted': 0}
            actions[source][kind] = 1
            value['script']['summary']['actions'] = actions
            variants.append(value)
        for value in variants:
            with self.subTest(value=value):
                self.assertTrue(measure(value)['issues'])

    def test_mcp_envelope_is_unwrapped_and_images_are_counted_once(self):
        data = completed(); data['value']['last_capture'] = {'step': 1}
        envelope = {'structuredContent': data, 'isError': False, 'content': [
            {'type': 'text', 'text': json.dumps(data)},
            {'type': 'image', 'data': base64.b64encode(b'png-test').decode(), 'mimeType': 'image/png'}]}
        result = measure(envelope, images=8)
        self.assertEqual(result['issues'], [])
        self.assertEqual(result['mcp_json_bytes'], probe.json_bytes(data))
        self.assertEqual(result['image_bytes'], 8)
        self.assertEqual(result['image_measurement'], 'complete')
        wrapped = {'jsonrpc': '2.0', 'id': 1, 'result': envelope}
        self.assertEqual(measure(wrapped)['mcp_json_bytes'], probe.json_bytes(data))
        envelope['content'][0]['text'] = json.dumps({'error': {'code': 'failed'}})
        self.assertTrue(measure(envelope)['issues'])

    def test_missing_image_bytes_are_unknown_even_without_capture_in_source(self):
        data = completed(); data['value']['last_capture'] = {'step': 1}
        result = measure(data, images=None)
        self.assertIsNone(result['image_bytes'])
        self.assertNotEqual(result['image_measurement'], 'complete')
        self.assertEqual(result['issues'], [])
        self.assertEqual(measure(completed(), images=None)['image_bytes'], 0)

    def test_opaque_json_and_non_json_constants_do_not_pass(self):
        for output in ('null', '[]', '{"bad":NaN}', {}, {'content': [{'type': 'text', 'text': 'failure'}]}):
            self.assertTrue(measure(output)['issues'])

    def test_script_calls_use_actual_field_and_missing_is_not_zero(self):
        data = completed(); data['value']['calls'] = 5
        result = measure(data)
        self.assertEqual(result['script_host_calls'], 5)
        self.assertEqual(result['script_calls_by_exec'][0]['calls'], 5)
        self.assertIsNone(measure(completed())['script_host_calls'])
        mixed = probe.metrics([call(data), call(completed())], NAME)
        self.assertIsNone(mixed['script_host_calls'])
        self.assertEqual(mixed['script_calls_by_exec'][0]['calls'], 5)
        for count in (True, -1, '5', None):
            data['value']['calls'] = count
            self.assertTrue(measure(data)['issues'])


class TerminalAndNormalizationTests(unittest.TestCase):
    def claude_image_fixture(self, home, directory):
        session = 'abdf93be-f7e7-45b8-a7b0-49433a95e77c'
        data = completed(); data['value']['last_capture'] = {'step': 1}; data['value']['calls'] = 3
        project = probe.re.sub(r'[^A-Za-z0-9]', '-', str(directory))
        image = home/'.claude/projects'/project/session/'tool-results'/f'mcp-{NAME}-blob-1793204637587-71rge1.png'
        blocks = [{'type': 'image', 'source': {'type': 'base64', 'media_type': 'image/png',
                                             'data': base64.b64encode(b'inline-png').decode()}},
                  {'type': 'text', 'text': '[Image: source: '+str(image)+']'},
                  {'type': 'text', 'text': json.dumps(data)}]
        events = [{'id': 1, 'method': 'session/new', 'params': {'cwd': str(directory)}},
                  {'id': 1, 'result': {'sessionId': session}},
                  {'params': {'sessionId': session, 'update': {
                      'sessionUpdate': 'tool_call', 'toolCallId': 'image-call',
                      'name': f'mcp__{NAME}__exec', 'status': 'completed',
                      'rawInput': {'app': APP, 'source': 'return true;'}, 'rawOutput': blocks}}}]
        return data, image, events

    def test_claude_inline_image_and_scoped_caption_count_once_without_file_read(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); home = root/'home'
            data, image, events = self.claude_image_fixture(home, root)
            path = root/'agent.jsonl'; path.write_text('\n'.join(map(json.dumps, events)))
            self.assertFalse(image.exists())
            with patch.object(probe.Path, 'home', return_value=home), \
                 patch.object(probe.Path, 'read_bytes', side_effect=AssertionError('No image file reads')):
                calls, _ = probe.normalize_acp(path)
                result = probe.metrics(calls, NAME)
            self.assertEqual(result['issues'], [])
            self.assertEqual(result['image_bytes'], len(b'inline-png'))
            self.assertEqual(result['mcp_json_bytes'], probe.json_bytes(data))
            self.assertEqual(result['script_host_calls'], 3)
            self.assertEqual(calls[0]['output'], events[-1]['params']['update']['rawOutput'])

    def test_claude_caption_cannot_hide_foreign_paths_missing_images_or_extra_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); home = root/'home'
            _, image, original = self.claude_image_fixture(home, root)
            blocks = original[-1]['params']['update']['rawOutput']
            variants = []
            for old, new in ((NAME, 'another-server'),
                             ('abdf93be-f7e7-45b8-a7b0-49433a95e77c', 'ba7a444b-9cd4-4e58-bab6-ec70d2f72c57'),
                             (str(image.parent), str(image.parent.parent/'other')),
                             ('/tool-results/', '/tool-results/../tool-results/')):
                bad = copy.deepcopy(blocks); bad[1]['text'] = bad[1]['text'].replace(old, new); variants.append(bad)
            variants.append(copy.deepcopy(blocks[1:]))
            bad = copy.deepcopy(blocks); bad[0]['source']['data'] = 'not base64'; variants.append(bad)
            bad = copy.deepcopy(blocks); bad[0]['source']['type'] = 'url'; variants.append(bad)
            variants.append(copy.deepcopy(blocks)+[{'type': 'text', 'text': 'arbitrary non-JSON text'}])
            variants.append(copy.deepcopy(blocks)+[copy.deepcopy(blocks[-1])])
            variants.append({'content': copy.deepcopy(blocks), 'isError': True})
            path = root/'agent.jsonl'
            for output in variants:
                events = copy.deepcopy(original); events[-1]['params']['update']['rawOutput'] = output
                path.write_text('\n'.join(map(json.dumps, events)))
                with patch.object(probe.Path, 'home', return_value=home):
                    calls, _ = probe.normalize_acp(path)
                    self.assertTrue(probe.metrics(calls, NAME)['issues'])
            # A plausible path is insufficient without the matching session/new RPC.
            for events in (original[1:], copy.deepcopy(original)):
                if len(events) == len(original): events[0]['params']['cwd'] = str(root/'other-cwd')
                path.write_text('\n'.join(map(json.dumps, events)))
                with patch.object(probe.Path, 'home', return_value=home):
                    calls, _ = probe.normalize_acp(path)
                    self.assertTrue(probe.metrics(calls, NAME)['issues'])

    def test_only_observed_success_terminal_shapes_pass(self):
        probe.check_terminal('claude', {'stopReason': 'end_turn'}, 0)
        probe.check_terminal('agy', {'event': 'result', 'result': {'status': 'SUCCESS'}}, 0)
        for client, terminal, code in (
                ('claude', None, 0), ('claude', {'stopReason': 'max_tokens'}, 0),
                ('claude', {'stopReason': 'max_turn_requests'}, 0),
                ('claude', {'stopReason': 'end_turn'}, 1),
                ('agy', {'event': 'done'}, 0),
                ('agy', {'event': 'result', 'result': {'status': 'ERROR'}}, 0),
                ('agy', {'event': 'result', 'result': {'status': 'SUCCESS', 'error': 'late failure'}}, 0)):
            with self.subTest(client=client, terminal=terminal), self.assertRaises(AssertionError):
                probe.check_terminal(client, terminal, code)

    def test_agy_print_timeout_overrides_success_and_zero_exit(self):
        terminal = {'event':'result', 'result':{'status':'SUCCESS', 'response':''}}
        for warning in ('[agy] print timeout after 4m0s with turn in progress; returning partial output',
                        '  [agy] print timeout after 10m0s\n',
                        '[agy] turn in progress; returning partial output'):
            with self.subTest(warning=warning), self.assertRaisesRegex(AssertionError, 'unfinished turn'):
                probe.check_terminal('agy', terminal, 0, warning)
        probe.check_terminal('agy', terminal, 0, '[agy] ordinary shutdown complete')
        probe.check_terminal('agy', terminal, 0, 'documentation mentions print timeout')

    def test_acp_partial_updates_keep_original_arguments_and_object_output(self):
        events = [
            {'params': {'update': {'sessionUpdate': 'tool_call', 'toolCallId': 'call-1',
                                  'name': f'mcp__{NAME}__exec', 'content': [],
                                  'rawInput': {'app': APP, 'source': 'return true;'}}}},
            {'params': {'update': {'sessionUpdate': 'tool_call_update', 'toolCallId': 'call-1',
                                  'status': 'completed', 'rawOutput': completed()}}},
            {'result': {'stopReason': 'end_turn'}}]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'agent.jsonl'
            path.write_text('\n'.join(map(json.dumps, events)))
            calls, terminal = probe.normalize_acp(path)
        self.assertEqual(len(calls), 1)
        self.assertEqual(probe.metrics(calls, NAME)['issues'], [])
        probe.check_terminal('claude', terminal, 0)

    def test_agy_partial_updates_and_owned_cache_match_actual_stream_shape(self):
        conversation = '5fd1c99e-31e0-4804-9131-580fd99ef54e'
        first = {'conversation_id': conversation, 'step_index': 10, 'state': 'ACTIVE',
                 'tool_name': 'call_mcp_tool', 'tool_info': {'parameters': {
                     'ServerName': NAME, 'ToolName': 'exec', 'Arguments': {'app': APP, 'source': 'return true;'}}}}
        events = [{'step_update': first},
                  {'step_update': {'conversation_id': conversation, 'step_index': 10, 'state': 'DONE'}},
                  {'event': 'result', 'result': {'status': 'SUCCESS'}}]
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            cache = home/'.gemini/antigravity-cli/brain'/conversation/'.system_generated/steps/10/output.txt'
            cache.parent.mkdir(parents=True); cache.write_text(json.dumps(completed()))
            path = home/'agent.jsonl'; path.write_text('\n'.join(map(json.dumps, events)))
            with patch.object(probe.Path, 'home', return_value=home):
                calls, terminal = probe.normalize_agy(path)
        self.assertEqual(len(calls), 1)
        self.assertEqual(probe.metrics(calls, NAME)['issues'], [])
        probe.check_terminal('agy', terminal, 0)

    def test_agy_observed_image_trailer_is_scoped_measured_and_original_retained(self):
        conversation = '5fd1c99e-31e0-4804-9131-580fd99ef54e'
        data = completed(); data['value']['last_capture'] = {'step': 1}
        payload = json.dumps(data)
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            step = home/'.gemini/antigravity-cli/brain'/conversation/'.system_generated/steps/16'
            step.mkdir(parents=True)
            image = step/'media_0.png'; image.write_bytes(b'png-test')
            output = payload+'\n[Resource offloaded to '+image.as_uri()+']'
            event = {'step_update': {'conversation_id': conversation, 'step_index': 16,
                     'state': 'DONE', 'tool_name': 'call_mcp_tool', 'tool_info': {
                         'parameters': {'ServerName': NAME, 'ToolName': 'exec',
                                        'Arguments': {'app': APP, 'source': 'return true;'}},
                         'output': output}}}
            path = home/'agent.jsonl'; path.write_text(json.dumps(event))
            with patch.object(probe.Path, 'home', return_value=home):
                calls, _ = probe.normalize_agy(path)
            result = probe.metrics(calls, NAME)
            self.assertEqual(result['issues'], [])
            self.assertEqual(result['image_bytes'], 8)
            self.assertEqual(result['mcp_json_bytes'], probe.json_bytes(data))
            self.assertEqual(calls[0]['output'], output)
            image.unlink()
            with patch.object(probe.Path, 'home', return_value=home):
                calls, _ = probe.normalize_agy(path)
            self.assertIsNone(probe.metrics(calls, NAME)['image_bytes'])
            for invalid in (output+'\nnot-json', payload+'\nnot-json',
                            output.replace('/steps/16/', '/steps/17/'),
                            output.replace('/media_0.png', '/../../private.png')):
                event['step_update']['tool_info']['output'] = invalid
                path.write_text(json.dumps(event))
                with patch.object(probe.Path, 'home', return_value=home):
                    calls, _ = probe.normalize_agy(path)
                self.assertTrue(probe.metrics(calls, NAME)['issues'])


def full_record(value, action=False):
    record = completed()
    record.update(context_reset=False, state_reset=False)
    record['value']['result'] = value
    record['script'].pop('summary')
    receipt = {'interruption':None, 'outcome':{'dispatch':'dispatched', 'error':None,
                                            'evidence':{'kind':'unverified'}}} if action else None
    record['script']['steps'] = [{'status':'completed', 'error':None, 'receipt':receipt}]
    return record


def inventory(*windows):
    return full_record({'coverage':{'status':'available' if windows else 'empty_within_scope', 'complete_within_scope':True},
                        'windows':list(windows)})


class CleanupClient:
    def __init__(self, inventories, dispatch=None):
        self.inventories = iter(inventories)
        self.dispatch = dispatch if dispatch is not None else full_record(True, action=True)
        self.sources = []
        self.context_epochs = {}

    def execute(self, app, case, source):
        self.sources.append(source)
        return (self.dispatch if 'app.keypress' in source else next(self.inventories)), None


class WindowCleanupTests(unittest.TestCase):
    def test_complete_empty_inventory_proves_last_window_closed(self):
        original = {'window_ref':'window-owned', 'title':'Owned task'}
        client = CleanupClient([inventory(original), inventory()])
        self.assertTrue(probe.close_owned_window(client, APP, 'window-owned', {}, epoch=0)['closed'])
        contradictory = inventory(original)
        contradictory['value']['result']['coverage']['status'] = 'empty_within_scope'
        with self.assertRaisesRegex(AssertionError, 'contradicts'):
            probe.checked_windows(contradictory, APP)

    def test_close_once_then_prove_same_ref_absent_not_title_absent(self):
        original = {'window_ref':'window-owned', 'title':'Owned task'}
        renamed = {**original, 'title':'Save changes?'}
        client = CleanupClient([inventory(original), inventory(renamed)])
        proof = {}
        with self.assertRaisesRegex(AssertionError, 'still present'):
            probe.close_owned_window(client, APP, 'window-owned', proof, timeout=0, epoch=0)
        self.assertEqual(sum('app.keypress' in source for source in client.sources), 1)
        self.assertEqual(len(proof['inventories']), 2)
        self.assertIs(proof['dispatch'], client.dispatch)
        self.assertFalse(proof['closed'])
        # A delayed close may finish; all subsequent requests stay read-only.
        client = CleanupClient([inventory(original), inventory(renamed), inventory()])
        with patch.object(probe.time, 'sleep'):
            result = probe.close_owned_window(client, APP, 'window-owned', {}, epoch=0)
        self.assertTrue(result['closed'])
        self.assertEqual(client.sources[2:], ['return await app.windows();'] * 2)

    def test_incomplete_or_reset_inventory_cannot_prove_absence(self):
        original = {'window_ref':'window-owned', 'title':'Owned task'}
        variants = []
        partial = inventory(); partial['value']['result']['coverage']['status'] = 'partial'; variants.append(partial)
        incomplete = inventory(); incomplete['value']['result']['coverage']['complete_within_scope'] = False; variants.append(incomplete)
        for flag in ('context_reset', 'state_reset'):
            reset = inventory(); reset[flag] = True; variants.append(reset)
        missing = inventory(); del missing['context_reset']; variants.append(missing)
        for record in variants:
            for before_dispatch in (True, False):
                client = CleanupClient([record] if before_dispatch else [inventory(original), record])
                with self.assertRaises(AssertionError):
                    probe.close_owned_window(client, APP, 'window-owned', {}, epoch=0)
                self.assertEqual(sum('app.keypress' in source for source in client.sources),
                                 0 if before_dispatch else 1)

    def test_unknown_close_is_retained_without_replay_or_fictitious_confirmation(self):
        original = {'window_ref':'window-owned', 'title':'Owned task'}
        unknown = full_record(True, action=True)
        unknown['script']['steps'][0]['receipt']['outcome']['dispatch'] = 'unknown'
        client = CleanupClient([inventory(original)], unknown); proof = {}
        with self.assertRaises(AssertionError):
            probe.close_owned_window(client, APP, 'window-owned', proof, epoch=0)
        self.assertIs(proof['dispatch'], unknown)
        self.assertEqual(len(client.sources), 2)
        self.assertFalse(proof['closed'])
        client = CleanupClient([inventory()])
        self.assertTrue(probe.close_owned_window(client, APP, 'window-owned', {}, epoch=0)['closed'])
        self.assertEqual(client.sources, ['return await app.windows();'])

    def test_prior_reset_stays_visible_after_later_calls_report_false(self):
        client = probe.SupervisorMcp.__new__(probe.SupervisorMcp)
        client.context_epochs = {}
        other_reset = inventory(); other_reset.update(app='com.apple.TextEdit', context_reset=True)
        reset = inventory(); reset['context_reset'] = True
        with patch.object(probe.Mcp, 'execute', side_effect=[
                (inventory(), None), (other_reset, None), (reset, None), (inventory(), None)]):
            client.execute(APP, 'setup', '')
            client.execute('com.apple.TextEdit', 'other', '')
            self.assertEqual(client.context_epochs.get(APP, 0), 0)
            client.execute(APP, 'native-reset', '')
            client.execute(APP, 'later-stable', '')
        self.assertEqual(client.context_epochs[APP], 1)
        with patch.object(probe.Mcp, 'execute') as execute:
            with self.assertRaisesRegex(AssertionError, 'setup context changed'):
                probe.close_owned_window(client, APP, 'window-owned', {}, epoch=0)
            execute.assert_not_called()
        with patch.object(probe.Mcp, 'execute', side_effect=RuntimeError('receipt missing')):
            with self.assertRaises(RuntimeError): client.execute(APP, 'transport-error', '')
        self.assertEqual(client.context_epochs[APP], 2)


def business_state(case):
    items = []
    for i in range(1, 241):
        sku = f'DTW-{i:04}'
        items.append({'sku':sku, 'name':f'Field kit {i:03}',
                      'warehouse':['North','South','Central'][i % 3],
                      'quantity':probe.QUANTITIES.get(sku, 10+i%25),
                      'reviewed':sku in probe.QUANTITIES, 'note':probe.NOTES.get(sku, '')})
    return {'case':case, 'ready':True, 'items':items, 'events':[],
            'saves':[{**row, 'unix_ms':123} for row in items if row['sku'] in probe.QUANTITIES]}


class BusinessFinalTests(unittest.TestCase):
    def test_every_final_row_and_exact_save_journal_are_checked(self):
        good = business_state('case-owned')
        probe.check_business_state(good, 'case-owned')
        variants = []
        extra = copy.deepcopy(good); extra['saves'].append(extra['saves'][0]); variants.append(extra)
        target = copy.deepcopy(good); target['items'][87]['quantity'] = 99; variants.append(target)
        unrelated = copy.deepcopy(good); unrelated['items'][0]['note'] = 'changed'; variants.append(unrelated)
        missing = copy.deepcopy(good); missing['items'].pop(); variants.append(missing)
        duplicate = copy.deepcopy(good); duplicate['items'][0] = duplicate['items'][1]; variants.append(duplicate)
        wrong = copy.deepcopy(good); wrong['case'] = 'another'; variants.append(wrong)
        for value in variants:
            with self.assertRaises(AssertionError): probe.check_business_state(value, 'case-owned')

    def test_late_save_row_or_document_change_fails_gate_and_retains_initial_success(self):
        for change in ('extra_save', 'target_row', 'document', None):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as directory:
                root = Path(directory); (root/'web').mkdir()
                state = business_state('case-owned'); path = root/'web/state.json'
                path.write_text(json.dumps(state)); document = root/'report.txt'; document.write_text(probe.REPORT)
                initial = probe.business_snapshot(root, document)
                probe.check_business_snapshot(initial, 'case-owned')
                evidence = {'passed':True, 'business_passed':True, 'business_initial_passed':True,
                            'business_initial':initial, 'usability_passed':True}
                def stop_server():
                    if change == 'extra_save': state['saves'].append(state['saves'][0])
                    if change == 'target_row': state['items'][87]['quantity'] = 99
                    if change == 'document': document.write_text('changed after initial check')
                    path.write_text(json.dumps(state))
                    return {'returncode':0, 'forced':[]}
                steps = [(resource, lambda:True, None) for resource in (APP, 'com.apple.TextEdit', 'mcp')]
                steps += [('server', stop_server, probe.process_cleaned),
                          ('final_business', lambda:probe.check_final_business(
                              evidence, root, document, 'case-owned', APP), None)]
                probe.cleanup_steps(evidence, steps)
                self.assertTrue(evidence['business_initial_passed'])
                self.assertEqual(evidence['business_initial'], initial)
                self.assertEqual(len(initial['browser']['saves']), 2)
                self.assertEqual(evidence['passed'], change is None)
                self.assertEqual(evidence['business_final_passed'], change is None)
                self.assertEqual(evidence['business_passed'], change is None)
                self.assertEqual(evidence['task_passed'], change is None)
                self.assertEqual(evidence['business_final']['browser'], state)

    def test_final_read_requires_confirmed_window_and_server_cleanup(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); (root/'web').mkdir()
            (root/'web/state.json').write_text(json.dumps(business_state('case-owned')))
            document = root/'report.txt'; document.write_text(probe.REPORT)
            for failed in (APP, 'com.apple.TextEdit', 'mcp', 'server'):
                evidence = {'business_initial_passed':True,
                            'cleanup':[{'resource':resource, 'ok':resource != failed}
                                       for resource in (APP, 'com.apple.TextEdit', 'mcp', 'server')]}
                with self.assertRaisesRegex(AssertionError, 'quiescent'):
                    probe.check_final_business(evidence, root, document, 'case-owned', APP)
                self.assertIn('business_final', evidence)
                self.assertFalse(evidence['business_final_passed'])
                self.assertFalse(evidence['business_passed'])


class CleanupTests(unittest.TestCase):
    def test_business_success_is_separate_from_usability_and_cleanup(self):
        original = {'type': 'AssertionError', 'message': 'Usability gate not met'}
        evidence = {'passed': False, 'business_passed': True, 'usability_passed': False, 'failure': original}
        probe.cleanup_steps(evidence, [('mcp', lambda: {'returncode': 0, 'forced': []}, probe.process_cleaned)])
        self.assertTrue(evidence['task_passed'])
        self.assertTrue(evidence['business_passed'])
        self.assertTrue(evidence['cleanup_passed'])
        self.assertFalse(evidence['passed'])
        self.assertEqual(evidence['failure'], original)
        evidence = {'passed': True, 'business_passed': True, 'usability_passed': True}
        probe.cleanup_steps(evidence, [('mcp', lambda: {'returncode': 1, 'forced': []}, probe.process_cleaned)])
        self.assertTrue(evidence['task_passed'])
        self.assertFalse(evidence['cleanup_passed'])
        self.assertFalse(evidence['passed'])

    def test_usability_thresholds_are_unchanged(self):
        boundary = {'mcp_calls': 12, 'exec_calls': 8, 'mcp_json_bytes': 40 * 1024, 'issues': [], 'result_queries': 0}
        self.assertTrue(probe.usability_passed(boundary))
        for key, value in [('mcp_calls', 0), ('mcp_calls', 13), ('exec_calls', 9),
                           ('mcp_json_bytes', 40 * 1024 + 1), ('issues', [{'error': 'failed'}]), ('result_queries', 1)]:
            self.assertFalse(probe.usability_passed({**boundary, key: value}))

    def test_nonzero_and_forced_cleanup_cannot_pass_and_results_are_retained(self):
        for result in ({'returncode': 74, 'forced': []}, {'returncode': 0, 'forced': ['kill']},
                       {'returncode': 0, 'forced': [], 'native_workers_reaped': False}):
            evidence = {'passed': True}
            probe.cleanup_steps(evidence, [('mcp', lambda: result, probe.process_cleaned)])
            self.assertFalse(evidence['passed'])
            self.assertTrue(evidence['task_passed'])
            self.assertEqual(evidence['cleanup'][0]['result'], result)
            self.assertEqual(evidence['failure']['type'], 'CleanupFailure')

    def test_cleanup_exception_continues_other_owned_resources_and_keeps_original_failure(self):
        reached = []
        def broken():
            reached.append('broken')
            raise RuntimeError('cleanup failure')
        def healthy():
            reached.append('healthy')
            return {'returncode': 0, 'forced': []}
        original = {'type': 'AssertionError', 'message': 'original task failure'}
        evidence = {'passed': False, 'failure': copy.deepcopy(original)}
        probe.cleanup_steps(evidence, [('window', broken, None), ('server', healthy, probe.process_cleaned)])
        self.assertEqual(reached, ['broken', 'healthy'])
        self.assertFalse(evidence['passed'])
        self.assertEqual(evidence['failure'], original)
        self.assertFalse(evidence['cleanup'][0]['ok'])
        self.assertTrue(evidence['cleanup'][1]['ok'])

    def test_clean_task_and_cleanup_pass_and_cleanup_ledger_unknown_is_rejected(self):
        evidence = {'passed': True}
        probe.cleanup_steps(evidence, [('mcp', lambda: {'returncode': 0, 'forced': [], 'native_workers_reaped': True}, probe.process_cleaned)])
        self.assertTrue(evidence['passed'])
        full = completed(); full['script']['steps'] = [{'status': 'completed', 'error': None,
            'receipt': {'outcome': {'dispatch': 'unknown', 'evidence': {'kind': 'unverified'}}, 'interruption': None}}]
        with self.assertRaises(AssertionError):
            probe.check_execution(full, {'app': APP}, compact=False)


if __name__ == '__main__':
    unittest.main()
