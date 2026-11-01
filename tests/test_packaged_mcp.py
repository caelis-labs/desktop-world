"""Real owned stdio children exercise packaged metadata checks without a desktop."""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('packaged_mcp', Path(__file__).parent / 'ci/run_packaged_mcp.py')
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)

FAKE = r'''
import json,sys,time
mode=sys.argv[1]
if sys.argv[2]=='--version':
 print('dtw '+('1.0.0' if mode=='bad_version' else '9.8.7'))
 raise SystemExit(0)
assert sys.argv[2]=='mcp'
def reply(identifier,result=None,error=None):
 value={'jsonrpc':'2.0','id':identifier}
 value['error' if error else 'result']=error if error else result
 print(json.dumps(value),flush=True)
for line in sys.stdin:
 request=json.loads(line)
 method=request['method']
 if method=='notifications/initialized':
  assert set(request)=={'jsonrpc','method'}
  continue
 assert method in ('initialize','tools/list','tools/call'),method
 if method=='initialize':
  if mode=='timeout':continue
  if mode=='malformed':
   print('not-json',flush=True);continue
  if mode=='oversized':
   print('x'*(1024*1024+1),flush=True);continue
  if mode=='rpc_error':
   reply(request['id'],error={'code':-32603,'message':'owned failure'});continue
  reply(99 if mode=='wrong_id' else request['id'],{
   'protocolVersion':'2025-06-18','capabilities':{'tools':{}},
   'serverInfo':{'name':'desktop-world','version':'9.8.7'}})
 elif method=='tools/list':
  reply(request['id'],{'tools':[{'name':name} for name in ['apps','api','exec','result']]})
 else:
  assert request['params']=={'name':'api','arguments':{'methods':['windows']}}
  data={'methods':{'windows':{'parameters':None,'returns':{}}}}
  reply(request['id'],{'isError':mode=='api_error','structuredContent':data,
   'content':[{'type':'text','text':json.dumps(data)}]})
  if mode=='extra_error':reply(88,error={'code':-32603,'message':'extra owned failure'})
  if mode=='notification':print(json.dumps({'jsonrpc':'2.0','method':'notifications/message','params':{'message':'owned notice'}}),flush=True)
 print('owned diagnostic',file=sys.stderr,flush=True)
if mode=='ignore_eof':time.sleep(30)
raise SystemExit(7 if mode=='bad_eof' else 0)
'''


class PackagedMcpTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix='dtw packaged metadata ')
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.fake = self.root / 'owned_fake.py'
        self.fake.write_text(FAKE, encoding='utf-8')
        self.launched = []

    def exercise(self, mode, *, timeout=2):
        def launch(command, **kwargs):
            owned = subprocess.Popen([sys.executable, str(self.fake), mode, *command[1:]], **kwargs)
            self.launched.append(owned)
            return owned
        output = self.root / mode
        report = probe.check(self.fake, '9.8.7', output, timeout=timeout,
                             cleanup_timeout=.2, popen=launch)
        self.assertEqual(report, json.loads((output / 'report.json').read_text()))
        self.assertTrue(all(child.poll() is not None for child in self.launched))
        self.assertTrue(all(row['pipe_workers_joined'] for row in report['cleanup']))
        self.assertEqual(report['binary_sha256'], hashlib.sha256(self.fake.read_bytes()).hexdigest())
        return report, output

    def test_only_allowed_metadata_requests_and_clean_eof_are_accepted(self):
        report, output = self.exercise('success')
        self.assertTrue(report['passed'], report)
        self.assertEqual(report['desktop_tools_called'], [])
        sent = [json.loads(line) for line in (output / 'mcp/requests.jsonl').read_text().splitlines()]
        self.assertEqual([request['method'] for request in sent],
                         ['initialize', 'notifications/initialized', 'tools/list', 'tools/call'])
        self.assertEqual(sent[-1]['params'], {'name': 'api', 'arguments': {'methods': ['windows']}})
        self.assertEqual({row['role'] for row in report['cleanup']}, {'version', 'mcp'})
        self.assertTrue(all(row['exit_code'] == 0 and not row['forced'] for row in report['cleanup']))
        self.assertIn(b'owned diagnostic', (output / 'mcp/stderr.bin').read_bytes())
        self.assertEqual(set(report['metadata']['tools']), {'apps', 'api', 'exec', 'result'})

    def test_wrong_cli_version_fails_before_mcp_is_started(self):
        report, output = self.exercise('bad_version')
        self.assertFalse(report['passed'])
        self.assertIn('version mismatch', report['error'])
        self.assertEqual(len(self.launched), 1)
        self.assertFalse((output / 'mcp').exists())

    def test_jsonrpc_error_api_error_and_wrong_response_identity_fail(self):
        for mode in ('rpc_error', 'api_error', 'wrong_id', 'malformed'):
            with self.subTest(mode=mode):
                report, output = self.exercise(mode)
                self.assertFalse(report['passed'])
                self.assertTrue((output / 'mcp/stdout.bin').read_bytes())
                self.assertTrue(report.get('error'))

    def test_response_timeout_retains_original_failure_even_when_eof_is_clean(self):
        report, output = self.exercise('timeout', timeout=.4)
        self.assertFalse(report['passed'])
        self.assertIn('deadline expired', report['error'])
        self.assertEqual(report['cleanup'][-1]['exit_code'], 0)
        self.assertFalse(report['cleanup'][-1]['forced'])
        sent = (output / 'mcp/requests.jsonl').read_text().splitlines()
        self.assertEqual(len(sent), 1)

    def test_nonzero_eof_and_forced_cleanup_cannot_be_a_pass(self):
        report, _ = self.exercise('bad_eof')
        self.assertFalse(report['passed'])
        self.assertEqual(report['cleanup'][-1]['exit_code'], 7)
        self.assertFalse(report['cleanup'][-1]['forced'])
        report, _ = self.exercise('ignore_eof')
        self.assertFalse(report['passed'])
        self.assertTrue(report['cleanup'][-1]['forced'])
        self.assertIsNotNone(report['cleanup'][-1]['exit_code'])

    def test_oversized_output_is_retained_bounded_and_not_accepted(self):
        report, output = self.exercise('oversized')
        self.assertFalse(report['passed'])
        self.assertLessEqual((output / 'mcp/stdout.bin').stat().st_size, probe.MAX_OUTPUT)
        self.assertTrue(report['cleanup'][-1]['io_faults'])

    def test_extra_response_at_eof_fails_but_notification_is_retained_and_allowed(self):
        report, _ = self.exercise('extra_error')
        self.assertFalse(report['passed'])
        self.assertIn('Unexpected MCP response after metadata completion', report['cleanup'][-1]['io_faults'])
        report, output = self.exercise('notification')
        self.assertTrue(report['passed'], report)
        self.assertIn(b'notifications/message', (output / 'mcp/stdout.bin').read_bytes())

    def test_existing_evidence_is_never_overwritten(self):
        output = self.root / 'prior'
        output.mkdir()
        (output / 'keep').write_bytes(b'original')
        with self.assertRaisesRegex(ValueError, 'new output directory'):
            probe.check(self.fake, '9.8.7', output)
        self.assertEqual((output / 'keep').read_bytes(), b'original')


if __name__ == '__main__':
    unittest.main()
