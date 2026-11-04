"""Browser cases mixed into CoreCases; caller supplies a fresh fixture per case.

All input and capture use current AppClient apps/exec. The caller owns fixture
lifecycle and continuous focus/cursor auditing. Chrome uses a unique copied bundle,
fresh profile and independent kernel/LaunchServices instance verification.
B05 waits for an external image reviewer; it never derives an answer from disk.
"""
import copy
import hashlib
import json
import math
from pathlib import Path
import re
import time

from app_client import require
from chrome_isolation import ChromeIsolationError, IsolatedChrome
from standard_agent_probe import wait_until


class ChromeImageAnswerTimeout(RuntimeError):
    pass


def _publish(path, value):
    temporary = path.with_suffix('.tmp')
    temporary.write_text(json.dumps(value, ensure_ascii=True, indent=2) + '\n', encoding='utf-8')
    temporary.replace(path)


def wait_for_image_answer(request, *, timeout):
    """Read only the external handoff file, with a finite monotonic deadline.

    Reviewer publishes JSON atomically. A malformed or mismatched answer fails
    immediately and is retained; there is no second answer or input attempt.
    """
    require(type(timeout) in (int, float) and math.isfinite(timeout) and 0 < timeout <= 600,
            'Image answer timeout must be finite and in (0, 600] seconds')
    path = Path(request['answer_file'])
    deadline = time.monotonic() + timeout
    while True:
        try:
            raw = path.read_bytes()
        except FileNotFoundError:
            raw = None
        if raw is not None:
            require(len(raw) <= 8192, 'Image answer file exceeds 8192 bytes')
            answer = json.loads(raw)
            require(isinstance(answer, dict), 'Image answer must be a JSON object')
            require(answer.get('capture_sha256') == request['capture_sha256'],
                    'Image answer is not bound to the published DTW capture')
            require(isinstance(answer.get('answer'), str)
                    and re.fullmatch(r'[A-Za-z0-9]{8}', answer['answer']) is not None,
                    'Image answer must contain exactly eight ASCII alphanumeric characters')
            require(isinstance(answer.get('reviewer'), str) and answer['reviewer'].strip(),
                    'Image answer must identify its external reviewer')
            point = answer.get('input_point')
            image = request['view']['image']
            require(isinstance(point, dict) and set(point) == {'x', 'y'}
                    and all(type(point[key]) in (int, float) and math.isfinite(point[key])
                            and 0 <= point[key] < image[dimension]
                            for key, dimension in (('x', 'width'), ('y', 'height'))),
                    'Image answer must locate the Image code input in capture pixel coordinates')
            return answer
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return None
        time.sleep(min(0.1, remaining))


class BrowserExtendedCases:
    """Independent B01/B04/B05 methods for ``class Cases(..., CoreCases)``.

    No constructor override. Use a fresh running ComputerUseFixture for each
    method, as for the other extended mixins. B05's default answer wait is 180
    seconds; set ``vision_answer_timeout`` before invocation to change it (<=600).
    Run B05 asynchronously so the main agent can ViewImage the requested PNG and
    atomically publish the answer JSON while this method waits.
    """

    def _chrome_case(self, identifier, url, business):
        output = Path(self.output)
        result_path = output / (identifier + '-result.json')
        require(not result_path.exists(), 'Preserve the original Chrome attempt')
        result = {'case': identifier, 'classification': 'should_fill_acceptance_harness',
                  'receipts': [], 'submitted': False}
        chrome = None
        try:
            try:
                chrome = IsolatedChrome(output, url)
            except FileNotFoundError as error:
                raise ChromeIsolationError('Chrome source unavailable: ' + str(error)) from error
            chrome.start()

            def execute(source, *, allow_error=False):
                chrome.verify()
                receipt, response = self.client.execute(chrome.bundle, source, allow_error=True)
                result['receipts'].append({'source': source, 'receipt': receipt,
                                           'tool_error': response.get('isError') is True})
                _publish(result_path, result)
                if not allow_error:
                    require(response.get('isError') is not True and receipt.get('status') == 'completed', receipt)
                return receipt, response

            def window_ready():
                initial, _ = execute('const v=await app.look(); return {running:v.running,windows:v.windows,coverage:v.coverage};')
                view = initial.get('result', {})
                # Exact instance uniqueness is independently established by
                # chrome.verify(); the public view coverage counts surfaces.
                require(view.get('running') is True, {'initial_owned_view': view})
                return view if view.get('windows') else None
            initial = wait_until(window_ready, timeout=15)
            result['initial_owned_view'] = initial
            if len(initial['windows']) > 1:
                require(any(w.get('title') in ('翻译此页？', 'Translate this page?') for w in initial['windows']),
                        {'unexpected_owned_popup': initial})
                observed, _ = execute('const v=await app.look({role:"button"}); return {elements:v.elements};')
                close = [e for e in observed.get('result', {}).get('elements', [])
                         if e.get('name') in ('关闭', 'Close') and 'press' in e.get('actions', [])]
                require(len(close) == 1, {'translation_close': close})
                execute('await app.click({target:' + json.dumps(close[0]['id']) + '});')
                result['translation_popup_dismissed'] = True
            result['target'] = str(chrome.bundle)
            result.update(business(chrome, execute, result))
        except ChromeIsolationError as error:
            result.update(blocked=str(error), classification='technical_isolation_failure')
        except ChromeImageAnswerTimeout as error:
            result.update(blocked=str(error), classification='scope_external_image_review')
        except Exception as error:
            result['failed'] = str(error)
            raise
        finally:
            if chrome is not None:
                result['isolation'] = chrome.evidence
                result['cleanup'] = chrome.close()
                if result['cleanup']['errors'] or result['cleanup']['forced']:
                    result['failed'] = 'Owned Chrome cleanup failed or required forced termination'
            _publish(result_path, result)
        require('failed' not in result, result)
        return result

    def _chrome_type(self, chrome, execute, label, value, result):
        source = ('const v=await app.look(' + json.dumps({'role': 'text_field', 'text': label})
                  + '); return {elements:v.elements};')
        observed, _ = execute(source)
        fields = [e for e in observed.get('result', {}).get('elements', [])
                  if e.get('name') == label and e.get('role') == 'text_field']
        require(len(fields) <= 1, {'field_observation': fields})
        if fields and 'press' in fields[0].get('actions', []):
            target = {'target': fields[0]['id']}
        else:
            name = result['case'] + '-field-' + str(len(result['receipts']))
            target = self._chrome_pointer(chrome, result, capture_name=name, target_label=label)
            if target is None:
                raise ChromeImageAnswerTimeout('External image point deadline elapsed for ' + label)
        execute('await app.click(' + json.dumps(target) + "); await app.keypress('META+A');"
                ' await app.typeText(' + json.dumps(value) + ');')
        data, _ = execute(source)
        fields = [e for e in data.get('result', {}).get('elements', [])
                  if e.get('name') == label and e.get('role') == 'text_field']
        if fields:
            require(len(fields) == 1 and fields[0].get('value') == value, {'readback': fields})
        result.setdefault('field_readbacks', []).append({'label': label, 'elements': fields,
            'verification': 'public AX value' if fields else 'deferred to exact independent business oracle'})

    def B01(self):
        def business(chrome, execute, result):
            value = '基准 Web café العربية 🙂 — once'
            before = copy.deepcopy(self.fixture.web_state())
            self._chrome_type(chrome, execute, 'Web note', value, result)
            result.update(submission_attempted=True, submitted=None)
            execute("await app.click({selector:{role:'button',text:'Commit web note'}});")
            after = wait_until(lambda: s if len((s := self.fixture.web_state())['saves']) != len(before['saves']) else None)
            result['submitted'] = True
            require(after['note'] == value and len(after['saves']) == len(before['saves']) + 1, after)
            execute("await app.click('Load noisy catalogue');")
            wait_until(lambda: self.fixture.web_state()['catalogue_loaded'])
            observed, _ = execute("const v=await app.look({text:'Audit checkpoint 137'}); return {elements:v.elements};")
            matches = [e for e in observed.get('result', {}).get('elements', []) if e.get('name') == 'Audit checkpoint 137']
            require(len(matches) == 1, observed)
            execute('await app.click({target:' + json.dumps(matches[0]['id']) + '});')
            after = wait_until(lambda: s if len((s := self.fixture.web_state())['checkpoint_hits']) != len(before['checkpoint_hits']) else None)
            require(after['checkpoint_hits'] == before['checkpoint_hits'] + [{'id': 137, 'activation': len(before['checkpoint_hits']) + 1}]
                    and len(after['saves']) == len(before['saves']) + 1 and after['note'] == value
                    and after['records'] == before['records'] and after['catalogue_reviews'] == before['catalogue_reviews'], after)
            return {'note': value, 'saves_delta': 1, 'checkpoint_hits_delta': 1, 'oracle': after}
        return self._chrome_case('B01', self.fixture.web_url, business)

    def _chrome_pointer(self, chrome, result, *, capture_name='B04-pointer', target_label='Create test record'):
        chrome.verify()
        data = self.client.capture(chrome.bundle, capture_name)
        path = Path(self.client.output) / (capture_name + '.png')
        request = {'schema_version': 1, 'case': result.get('case', 'B04'), 'stage': 'awaiting_pointer_answer',
                   'target_label': target_label,
                   'capture': str(path.resolve()),
                   'capture_sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
                   'view': data['view'], 'answer_file': str((Path(self.output) / (capture_name + '-answer.json')).resolve()),
                   'instructions': 'Inspect DTW PNG; publish capture_sha256, reviewer, and input_point at ' + target_label + ' center.',
                   'answer_schema': {'capture_sha256': 'copy the PNG hash above', 'reviewer': 'agent identity',
                                     'input_point': {'x': target_label + ' center in PNG pixels',
                                                     'y': target_label + ' center in PNG pixels'}}}
        require(not Path(request['answer_file']).exists(), 'B04 requires a fresh pointer answer')
        _publish(Path(self.output) / (capture_name + '-request.json'), request)
        result['pointer_request'] = request
        callback = getattr(self, 'chrome_pointer_point', None)
        if callback is not None:
            answer = callback(request)
        else:
            timeout = getattr(self, 'chrome_pointer_timeout', 180)
            require(type(timeout) in (int, float) and math.isfinite(timeout) and 0 < timeout <= 600,
                    'B04 pointer timeout must be finite and in (0, 600] seconds')
            deadline = time.monotonic() + timeout
            while not Path(request['answer_file']).exists():
                if time.monotonic() >= deadline:
                    return None
                time.sleep(0.1)
            require(Path(request['answer_file']).stat().st_size <= 8192, 'Oversized B04 pointer answer')
            answer = json.loads(Path(request['answer_file']).read_bytes())
        point = answer.get('input_point')
        image = data['view']['image']
        require(answer.get('capture_sha256') == request['capture_sha256']
                and isinstance(answer.get('reviewer'), str) and answer['reviewer'].strip()
                and isinstance(point, dict) and set(point) == {'x', 'y'}
                and all(type(point[k]) in (int, float) and math.isfinite(point[k]) and 0 <= point[k] < image[d]
                        for k, d in (('x', 'width'), ('y', 'height'))), 'Invalid B04 image pointer answer')
        require(hashlib.sha256(path.read_bytes()).hexdigest() == request['capture_sha256'], 'B04 capture changed')
        result['pointer_answer'] = answer
        chrome.verify()
        current = self.client.capture(chrome.bundle, capture_name + '-preflight')['view']
        geometry = lambda view: {'image': {key: view['image'][key] for key in ('bounds', 'width', 'height')},
                                 'windows': view.get('windows', [])}
        result['pointer_geometry'] = {'requested': geometry(data['view']), 'current': geometry(current)}
        require(result['pointer_geometry']['requested'] == result['pointer_geometry']['current'],
                'Chrome pointer geometry changed during external review; no dispatch')
        result.setdefault('image_points', []).append({'request': request, 'answer': answer,
                                                      'geometry': result['pointer_geometry']})
        return point

    def B04(self):
        def oracle():
            return json.loads((self.fixture.output / 'workbench/state.json').read_text())['reliability']

        def business(chrome, execute, result):
            before = copy.deepcopy(oracle())
            require(not before['attempts'] and not before['submissions'], 'B04 requires fresh original reliability fixture')
            result['modes'] = []
            for index, mode in enumerate(('normal', 'trusted-pointer')):
                label = 'B04-' + mode
                self._chrome_type(chrome, execute, 'Run label', label, result)
                self._chrome_type(chrome, execute, 'Record title', label + ' 中文 café', result)
                if mode == 'trusted-pointer':
                    execute("await app.click({selector:{role:'checkbox',text:'Require trusted pointer sequence'}});")
                    point = self._chrome_pointer(chrome, result)
                    if point is None:
                        return {'blocked': 'External image pointer answer deadline elapsed',
                                'classification': 'scope_external_image_review', 'modes': result['modes']}
                    source = 'await app.click(' + json.dumps(point) + ');'
                else:
                    source = "await app.click({selector:{role:'button',text:'Create test record'}});"
                result.update(submission_attempted=True, submitted=None)
                receipt, response = execute(source, allow_error=True)
                row = {'mode': mode, 'receipt': receipt, 'tool_error': response.get('isError') is True}
                result['modes'].append(row)
                _publish(Path(self.output) / 'B04-result.json', result)
                dispatch = receipt.get('effects', {}).get('dispatch', {})
                require(any(type(dispatch.get(k)) is int and dispatch[k] > 0 for k in ('dispatched', 'unknown')), row)
                after = wait_until(lambda: s if len((s := oracle())['attempts']) > index else None)
                result['submitted'] = True
                row.update(attempts=after['attempts'], events=after['events'], submissions=after['submissions'],
                           attempts_delta=len(after['attempts']) - index,
                           submissions_delta=len(after['submissions']) - index)
                _publish(Path(self.output) / 'B04-result.json', result)
                attempt = after['attempts'][index]
                events = [e for e in after['events'] if e.get('run') == label]
                row['activation_counts'] = {
                    'attempts': row['attempts_delta'], 'submissions': row['submissions_delta'],
                    'submit_events': sum(e['kind'] == 'submit' and e.get('target') == 'form'
                                         and e.get('trusted') is True for e in events),
                    'create_clicks': sum(e['kind'] == 'click' and e.get('target') == 'create' for e in events)}
                _publish(Path(self.output) / 'B04-result.json', result)
                require(all(count == 1 for count in row['activation_counts'].values()), row)
                require(len(after['attempts']) == index + 1 and len(after['submissions']) == index + 1
                        and attempt['run'] == label and attempt['title'] == label + ' 中文 café'
                        and attempt['mode'] == mode and attempt['accepted'] is True
                        and attempt['duplicate'] is False, row)
                if mode == 'trusted-pointer':
                    require(all(attempt['gesture'].values()) and all(any(e['kind'] == kind and e.get('trusted') is True
                            and e.get('target') == 'create' and e.get('run') == label for e in after['events'])
                            for kind in ('pointerdown', 'pointerup', 'click')), row)
                require(not response.get('isError') and receipt.get('status') == 'completed', row)
            require(after['icon_activations'] == before['icon_activations']
                    and len({a['nonce'] for a in after['attempts']}) == 2, after)
            return {'modes': result['modes'], 'submitted': True, 'oracle': after}
        return self._chrome_case('B04', self.fixture.workbench_url.rstrip('/') + '/reliability', business)

    def B05(self):
        """Capture through DTW, wait for image answer, submit once, then read oracle."""
        timeout = getattr(self, 'vision_answer_timeout', 180)
        require(type(timeout) in (int, float) and math.isfinite(timeout) and 0 < timeout <= 600,
                'Image answer timeout must be finite and in (0, 600] seconds')
        output = Path(self.output)
        request_path = output / 'B05-answer-request.json'
        answer_path = output / 'B05-answer.json'
        capture_path = Path(self.client.output) / 'B05-challenge.png'
        require(not any(path.exists() for path in (request_path, answer_path, capture_path)),
                'B05 requires fresh handoff and capture paths; preserve the original attempt')
        # web_ready reads native state only. Do not call fixture.web_state here:
        # that private document contains the expected image token.
        self.web_ready()
        self.run("await app.click({selector:{role:'link',text:'Image challenge'}});")
        wait_until(lambda: s if (s := self.state())['web']['loaded']
                   and s['web']['url'].rstrip('/').endswith('/vision') else None, timeout=10)
        data = self.client.capture(self.app, 'B05-challenge')
        payload = capture_path.read_bytes()
        require(payload.startswith(b'\x89PNG\r\n\x1a\n'), 'B05 requires an actual DTW PNG')
        request = {'schema_version': 1, 'case_id': 'B05', 'stage': 'awaiting_image_answer',
                   'capture': str(capture_path.resolve()),
                   'capture_sha256': hashlib.sha256(payload).hexdigest(),
                   'view': data['view'], 'answer_file': str(answer_path.resolve()),
                   'timeout_seconds': timeout,
                   'instructions': 'Inspect capture with ViewImage. Publish answer JSON atomically.',
                   'answer_schema': {'capture_sha256': 'copy capture_sha256 above',
                                     'answer': 'eight characters read from the PNG',
                                     'reviewer': 'agent identity', 'notes': 'optional visual observations',
                                     'input_point': {'x': 'Image code input center in PNG pixels',
                                                     'y': 'Image code input center in PNG pixels'}}}
        _publish(request_path, request)
        answer = wait_for_image_answer(request, timeout=timeout)
        if answer is None:
            return {'blocked': 'External image answer did not arrive before the bounded deadline',
                    'request': str(request_path), 'capture': request['capture'],
                    'capture_sha256': request['capture_sha256'], 'submitted': False}
        require(hashlib.sha256(capture_path.read_bytes()).hexdigest() == request['capture_sha256'],
                'DTW capture changed during the image answer handoff')
        fields = [element for element in data['view'].get('elements', [])
                  if element.get('role') == 'text_field' and element.get('name') == 'Image code']
        if (len(fields) == 1 and 'press' in fields[0].get('actions', [])
                and isinstance(fields[0].get('id'), str)):
            target = {'target': fields[0]['id']}
            input_route = {'kind': 'semantic_press', 'target': fields[0]['id'],
                           'actions': fields[0]['actions']}
        else:
            target = answer['input_point']
            input_route = {'kind': 'model_point', 'point': target}
        evidence = {'request': str(request_path), 'capture': request['capture'],
                    'capture_sha256': request['capture_sha256'], 'reviewer': answer['reviewer'],
                    'notes': answer.get('notes', ''), 'input_point': answer['input_point'],
                    'input_route': input_route, 'readback': None, 'submitted': False,
                    'evidence_class': 'external-agent/image-channel/current-MCP', 'stack': 'WebKit/PNG'}
        _publish(output / 'B05-result.json', evidence)
        # Choose the route before dispatch; never fall back after uncertain input.
        self.run('await app.click(' + json.dumps(target) + ');'
                 " await app.keypress('META+A'); await app.typeText(" + json.dumps(answer['answer']) + ');')
        readback = self.run("const v=await app.look({role:'text_field',text:'Image code'});"
                            ' return {elements:v.elements};')
        observed = readback.get('result', {}).get('elements', [])
        matching = [element for element in observed
                    if element.get('role') == 'text_field' and element.get('name') == 'Image code']
        evidence['readback'] = {'elements': observed, 'matched': len(matching) == 1
                                and matching[0].get('value') == answer['answer']}
        _publish(output / 'B05-result.json', evidence)
        require(evidence['readback']['matched'], evidence)
        # Exactly one submission, after exact public readback. Unknown input is never replayed.
        receipt, response = self.client.execute(self.app,
            "await app.click({selector:{role:'button',text:'Verify image'}});", allow_error=True)
        evidence.update(submission_receipt=receipt, tool_error=response.get('isError') is True)
        _publish(output / 'B05-result.json', evidence)
        dispatch = receipt.get('effects', {}).get('dispatch', {})
        require(any(type(dispatch.get(key)) is int and dispatch[key] > 0
                    for key in ('dispatched', 'unknown')), receipt)
        # First private oracle read is strictly after the external answer and submit.
        after = wait_until(lambda: v if (v := self.fixture.web_state()['vision'])['attempts'] else None)
        attempts = after['attempts']
        evidence.update(attempts=attempts, submitted=True)
        _publish(output / 'B05-result.json', evidence)
        require(len(attempts) == 1 and attempts[0]['submitted'] == answer['answer']
                and attempts[0]['passed'] is True and after['expected_token'] == answer['answer'], evidence)
        require(response.get('isError') is not True and not receipt.get('error')
                and not receipt.get('failure'), evidence)
        return evidence
