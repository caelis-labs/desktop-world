"""Browser cases mixed into CoreCases; caller supplies a fresh fixture per case.

All input and capture use current AppClient apps/exec. The caller owns fixture
lifecycle and continuous focus/cursor auditing. Chrome remains blocked until an
existing owned-profile facility can establish exact current DTW process binding.
B05 waits for an external image reviewer; it never derives an answer from disk.
"""
import hashlib
import json
import math
from pathlib import Path
import re
import time

from app_client import require
from standard_agent_probe import wait_until


CHROME_ISOLATION_BLOCK = (
    'Existing owned Chrome profile launchers do not establish exact process/profile '
    'binding through current apps/exec: authorization accepts a bundle ID or .app '
    'path, while discovery exposes instance counts without an owned PID selector. '
    'Historical window-selection APIs cannot be reused. No Chrome was launched '
    'or contacted; an owned profile alone is insufficient isolation evidence.'
)


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

    def B01(self):
        return {'blocked': CHROME_ISOLATION_BLOCK, 'case': 'B01',
                'required_tasks': ['Unicode web-note commit once', 'noisy catalogue checkpoint 137']}

    def B04(self):
        return {'blocked': CHROME_ISOLATION_BLOCK, 'case': 'B04',
                'required_tasks': ['normal submission', 'explicit trusted-pointer submission'],
                'oracle': 'original reliability attempts/events and distinct business effects'}

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
