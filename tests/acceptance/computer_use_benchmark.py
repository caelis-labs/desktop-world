#!/usr/bin/env python3
"""Current MCP baseline for the unified owned Computer Use app (direct scripts)."""
import argparse
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import time
import traceback

from app_client import AppClient, artifact_identity, require
from standard_agent_probe import wait_until

FIXTURE = Path(__file__).resolve().parents[1] / 'fixtures/computer-use-app'


def fixture_module():
    spec = importlib.util.spec_from_file_location('computer_use_fixture', FIXTURE / 'fixture.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load_targets(path=FIXTURE / 'scenarios.json'):
    data = json.loads(path.read_text(encoding='utf-8'))
    identifiers = [row['id'] for row in data['scenarios']]
    if data['schema_version'] != 1 or len(set(identifiers)) != len(identifiers):
        raise ValueError('invalid benchmark target manifest')
    if not identifiers or not any(row['profile'] == 'core' for row in data['scenarios']):
        raise ValueError('benchmark requires core targets')
    return data


class BenchmarkReport:
    def __init__(self, targets, identity):
        self.targets = targets
        self.data = {**identity, 'baseline': targets['baseline'], 'evidence_class': 'direct-script/current-MCP',
            'limits': ['Supervisor geometry is used for pixel tasks; this is not model vision evidence.',
                'Portable pages do not establish Chromium, Windows or Linux native coverage.',
                'Endpoint foreground samples cannot exclude transient focus changes.'],
            'cases': [{**row, 'status': 'not_run', 'detail': 'Target not executed by this run'}
                      for row in targets['scenarios']], 'visual_reviews': [], 'cleanup': None, 'setup_error': None, 'tool_errors': [], 'clean_run': None}

    def record(self, identifier, status, detail):
        if status not in {'passed', 'failed', 'unsupported', 'blocked', 'pending_review'}:
            raise ValueError('invalid executed status')
        row = next(row for row in self.data['cases'] if row['id'] == identifier)
        if row['status'] != 'not_run':
            raise ValueError('attempt already recorded; preserve it and use a new run directory')
        row.update(status=status, detail=detail)

    def snapshot(self):
        return summarize(self.data)


def summarize(original):
    data = copy.deepcopy(original)
    for row in data['cases']:
        if row['status'] == 'pending_review' or row.get('original_status') == 'pending_review':
            digest = row['detail']['capture_sha256']
            reviews = [review for review in data.get('visual_reviews', [])
                       if review['case_id'] == row['id'] and review['capture_sha256'] == digest]
            if reviews:
                row.update(status='passed' if reviews[-1]['outcome'] == 'passed' else 'failed',
                           original_status='pending_review')
    data['counts'] = {status: sum(row['status'] == status for row in data['cases'])
        for status in ('passed', 'failed', 'unsupported', 'blocked', 'pending_review', 'not_run')}
    clean = data['cleanup'] is not None and data['cleanup'].get('passed') is True and not data['setup_error']
    core = [row for row in data['cases'] if row['profile'] == 'core']
    data['core_automation_passed'] = clean and all(row['status'] in {'passed', 'pending_review'}
        or row.get('original_status') == 'pending_review' for row in core)
    data['core_passed'] = clean and data.get('clean_run') is True and all(row['status'] == 'passed' for row in core)
    data['target_baseline_passed'] = clean and data.get('clean_run') is True and all(row['status'] == 'passed' for row in data['cases'])
    pointer = {row['id']: row['status'] for row in data['cases'] if row['profile'] == 'pointer'}
    data['acceptance_passed'] = (data['core_passed'] and pointer.get('P01') == 'passed'
        and pointer.get('P02') == 'passed' and pointer.get('P03') == 'passed'
        and data.get('cursor_policy', {}).get('passed') is True)
    return data


def review_capture(report_path, identifier, reviewer, notes, *, outcome='passed'):
    report_path = Path(report_path).resolve()
    data = json.loads(report_path.read_text(encoding='utf-8'))
    row = next(row for row in data['cases'] if row['id'] == identifier)
    require(row['status'] == 'pending_review' or row.get('original_status') == 'pending_review',
            'Only captures requiring review can receive a visual review')
    path = (report_path.parent / row['detail']['capture']).resolve()
    require(path.is_relative_to(report_path.parent), 'Capture must belong to this run')
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    require(digest == row['detail']['capture_sha256'], 'Capture bytes changed after the original attempt')
    require(outcome in {'passed', 'failed'} and reviewer.strip() and notes.strip(), 'Review requires outcome, reviewer and observations')
    data.setdefault('visual_reviews', []).append({'case_id': identifier, 'capture_sha256': digest,
        'reviewer': reviewer, 'notes': notes, 'outcome': outcome})
    result = summarize(data)
    fixture_module().write_json(report_path, result)
    return result


def utf16_fingerprint(text):
    units = text.encode('utf-16-le', errors='surrogatepass')
    value = 14695981039346656037
    for index in range(0, len(units), 2):
        value = ((value ^ (units[index] | units[index + 1] << 8)) * 1099511628211) & ((1 << 64) - 1)
    return format(value, 'x')


def image_point(view, screen):
    image = view['image']
    left, top, width, height = image['bounds']
    point = {'x': (screen[0] - left) * image['width'] / width,
             'y': (screen[1] - top) * image['height'] / height}
    require(0 <= point['x'] < image['width'] and 0 <= point['y'] < image['height'],
            {'reason': 'fixture anchor is outside returned capture', 'screen': screen, 'image': image})
    return point


def complete_text_source(name):
    return '''const v=await app.look({text:NAME});
const candidates=v.elements.filter(x=>x.name===NAME);
if(candidates.length!==1)throw new Error('Expected one named text target: '+JSON.stringify(v));
let offset=0,total=null,text='',pages=[];
for(let i=0;i<32;i++){
 const p=await app.readText({target:candidates[0].id,offset,limit:4096});
 if(p.unit!=='utf16'||p.offset!==offset||p.next_offset!==null&&p.next_offset!==offset+p.text.length)
  throw new Error('Invalid text page continuity');
 if(total!==null&&total!==p.total_length)throw new Error('Text revision length changed');
 total=p.total_length;text+=p.text;pages.push({offset:p.offset,next_offset:p.next_offset,length:p.text.length});
 if(p.next_offset===null){
  if(text.length!==total)throw new Error('Incomplete read');
  let hash=14695981039346656037n;
  for(let j=0;j<text.length;j++)hash=BigInt.asUintN(64,(hash^BigInt(text.charCodeAt(j)))*1099511628211n);
  return {length:total,tail:text.slice(-80),fingerprint:hash.toString(16),pages};
 }
 offset=p.next_offset;
}
throw new Error('Text paging budget exhausted');'''.replace('NAME', json.dumps(name))


class CoreCases:
    def __init__(self, fixture, client, output, *, overlay_observer=None, focus_observer=None):
        self.fixture, self.client, self.output = fixture, client, output
        self.app = fixture.bundle
        self.overlay_observer = overlay_observer
        self.focus_observer = focus_observer

    def run(self, source):
        return self.client.execute(self.app, source)[0]

    def state(self):
        return self.fixture.state()

    def native(self):
        return self.state()['native']

    def events(self, kind):
        rows = [json.loads(line) for line in (self.fixture.output / 'native-events.jsonl').read_text(encoding='utf-8').splitlines()]
        return [row for row in rows if row.get('kind') == kind]

    def chat(self):
        return self.state()['chat']

    def capture(self, name):
        return self.client.capture(self.app, name)['view']

    def anchor(self, name, view):
        return image_point(view, self.state()['anchors'][name])

    def type_at(self, name, value, capture_name):
        view = self.capture(capture_name)
        point = self.anchor(name, view)
        self.run('await app.click(' + json.dumps(point) + "); await app.keypress('META+A'); await app.typeText(" + json.dumps(value) + ');')

    def N01(self):
        before = self.native()
        # First app operation in this MCP connection: deliberately no look.
        self.run("await app.click({selector:{role:'button',text:'Increment counter'}});")
        after = wait_until(lambda: s if (s := self.native())['counter'] != before['counter'] else None)
        require(after['counter'] == before['counter'] + 1 and after['saves'] == before['saves'], after)
        return {'counter_before': before['counter'], 'counter_after': after['counter'], 'initial_look': False}

    def N02(self):
        before = self.native()
        observed = self.run("const v=await app.look({text:'Audit checkpoint 137'}); return {elements:v.elements,semantics:v.semantics};")
        require(any(e.get('name') == 'Audit checkpoint 137' for e in observed['result']['elements']), observed)
        self.run("await app.click({selector:{role:'button',text:'Audit checkpoint 137'}});")
        after = self.native()
        require(after['audit_index'] == 137 and after['audit_count'] == before['audit_count'] + 1, after)
        require(after['counter'] == before['counter'], after)
        return {'audit_index': after['audit_index'], 'audit_count': after['audit_count']}

    def N03(self):
        value = '基准 Native café العربية 🙂 — exact commit'
        before = self.native()
        self.type_at('native-note', value, 'native-note')
        self.run("await app.click('Commit note');")
        after = self.native()
        require(after['saved_note'] == value and after['saves'] == before['saves'] + 1, after)
        return {'saved_note': after['saved_note'], 'saves_delta': after['saves'] - before['saves']}

    def N04(self):
        data = self.run(complete_text_source('Long reference'))
        result = data['result']
        expected = (self.fixture.output / 'native-reference.txt').read_text(encoding='utf-8')
        require(result['length'] > 8000 and result['tail'].endswith('NATIVE-END-137-中文🙂'), result)
        require(result['fingerprint'] == utf16_fingerprint(expected), result)
        return result

    def N05(self):
        before = self.native()['reviewed']
        events_before = len(self.events('reviewed'))
        self.run("await app.click({selector:{role:'checkbox',text:'Reviewed'}});")
        after = self.native()['reviewed']
        require(after is not before and len(self.events('reviewed')) == events_before + 1, {'before': before, 'after': after})
        return {'before': before, 'after': after, 'activations_delta': 1}

    def N06(self):
        view = self.capture('native-slider')
        points = [self.anchor(name, view) for name in ('slider-start', 'slider-end')]
        self.run('await app.drag({path:' + json.dumps(points) + '});')
        after = wait_until(lambda: s if (s := self.native())['slider'] >= 90 else None)
        require(after['slider_changes'] > 0 and self.state()['pressed_mouse_buttons'] == 0, after)
        return {'slider': after['slider'], 'changes': after['slider_changes'], 'pressed_mouse_buttons': 0}

    def N07(self):
        before = self.native()
        view = self.capture('native-scroll')
        point = self.anchor('native-left-scroll', view)
        self.run('await app.scroll(' + json.dumps({**point, 'delta_y': 80}) + ');')
        after = wait_until(lambda: s if (s := self.native())['left_scroll_y'] != before['left_scroll_y'] else None)
        require(after['right_scroll_y'] == before['right_scroll_y'], after)
        return {key: [before[key], after[key]] for key in ('left_scroll_y', 'right_scroll_y')}

    def N08(self):
        before = self.state()
        data = self.run("const v=await app.look({scope:'menu',text:'File'}); return {elements:v.elements,semantics:v.semantics};")
        require(data['result']['elements'], data)
        after = self.state()
        require(after['native'] == before['native'], after)
        require(after['environment']['frontmost_pid'] == before['environment']['frontmost_pid'], after)
        return {'menu_names': [e.get('name') for e in data['result']['elements']], 'foreground_endpoints_equal': True}

    def N09(self):
        before = self.native()
        self.run("await app.click('New peer window');")
        wait_until(lambda: len(self.native()['peers']) == len(before['peers']) + 1)
        self.run("await app.click('Increment peer counter');")
        after = wait_until(lambda: s if (s := self.native())['peers'][-1]['counter'] == 1 else None)
        require(after['counter'] == before['counter'], after)
        view = self.capture('peer-close')
        close = self.anchor('peer-close-' + str(len(after['peers']) - 1), view)
        closing, response = self.client.execute(self.app, 'await app.click(' + json.dumps(close) + ');', allow_error=True)
        if response.get('isError'):
            require(closing.get('effects', {}).get('dispatch', {}).get('unknown') == 1, closing)
        wait_until(lambda: not any(' — Peer ' in w['title'] for w in self.state()['windows']))
        require(self.native()['counter'] == before['counter'], self.native())
        return {'peer_counter': after['peers'][-1]['counter'], 'main_counter_unchanged': True,
            'peer_closed': True, 'closing_receipt': closing.get('failure')}

    def N10(self):
        opening, response = self.client.execute(self.app, "await app.click('Open file…');", allow_error=True)
        if response.get('isError'):
            require(opening.get('failure', {}).get('code') == 'ax_action_failed'
                and opening.get('effects', {}).get('dispatch', {}).get('unknown') == 1, opening)
        # A sheet may appear even when AXPress returns cannot-complete. Read its
        # independent effect, never repeat the opening action; retain the error.
        wait_until(lambda: self.native()['panel']['visible'])
        self.capture('native-open-panel')
        observed = self.run("const v=await app.look({role:'button',text:'Cancel'}); return {elements:v.elements};")
        cancel = [e for e in observed['result']['elements'] if e.get('name') == 'Cancel']
        require(len(cancel) == 1, observed)
        closing, closed = self.client.execute(self.app, 'await app.click({target:' + json.dumps(cancel[0]['id']) + '});', allow_error=True)
        if closed.get('isError'):
            require(closing.get('effects', {}).get('dispatch', {}).get('unknown') == 1, closing)
        after = wait_until(lambda: p if (p := self.native()['panel'])['result'] == 'cancel' else None)
        require(after['file_saves'] == 0 and not after['opened_text'], after)
        return {**after, 'opening_receipt': {'dispatch': opening.get('effects', {}).get('dispatch'),
            'failure': opening.get('failure'), 'tool_error': response.get('isError') is True},
            'closing_receipt': {'dispatch': closing.get('effects', {}).get('dispatch'),
            'failure': closing.get('failure'), 'tool_error': closed.get('isError') is True}}

    def C01(self):
        self.run("await app.click('Canvas chat');")
        wait_until(lambda: self.state()['mode'] == 'chat')
        self.capture('native-chat')
        data = self.run("const v=await app.look({text:'Release check 137'}); return {elements:v.elements,semantics:v.semantics};")
        require(data['result']['semantics']['complete'] is True, data)
        require(not any('Release check 137' in str(e) for e in data['result']['elements']), data)
        image_path = self.output / 'mcp/native-chat.png'
        return {'message_text_in_ax': False, 'capture': 'mcp/native-chat.png',
            'capture_sha256': hashlib.sha256(image_path.read_bytes()).hexdigest(), 'gui_review': 'required'}

    def C02(self):
        before = self.chat()
        view = self.capture('native-chat-before-menu')
        message = self.anchor('chat-message-137', view)
        self.run('await app.click(' + json.dumps({**message, 'button': 'right'}) + ');')
        wait_until(lambda: self.chat()['menu_open'])
        # New popup geometry from the oracle; no intermediate capture/look.
        pin = self.anchor('chat-pin', view)
        self.run('await app.click(' + json.dumps(pin) + '); await app.click(' + json.dumps(message) + ');')
        after = wait_until(lambda: s if (s := self.chat())['pins'] != before['pins'] else None)
        require(after['pinned'] == 'message-137' and after['pins'] == before['pins'] + 1 and not after['menu_open'], after)
        return {'pinned': after['pinned'], 'pins_delta': after['pins'] - before['pins'], 'intermediate_capture': False}

    def C03(self):
        value = '基准 reply — café 🙂 العربية'
        before = self.chat()
        menu_view = self.capture('native-chat-before-reply')
        message = self.anchor('chat-message-137', menu_view)
        self.run('await app.click(' + json.dumps({**message, 'button': 'right'}) + ');')
        wait_until(lambda: self.chat()['menu_open'])
        self.run('await app.click(' + json.dumps(self.anchor('chat-reply', menu_view)) + ');')
        self.type_at('chat-compose', value, 'native-chat-compose')
        view = self.capture('native-chat-send')
        self.run('await app.click(' + json.dumps(self.anchor('chat-send', view)) + ');')
        after = wait_until(lambda: s if (s := self.chat())['sends'] != before['sends'] else None)
        require(after['sends'] == before['sends'] + 1 and after['replies'] == before['replies'] + [value], after)
        require(after['reply_to'] == 'message-137' and after['reply_count'] == before['reply_count'] + 1, after)
        return {'sends_delta': after['sends'] - before['sends'], 'reply_to': after['reply_to'], 'text': after['replies'][-1]}

    def web_ready(self):
        self.run("await app.click('Web workspace');")
        return wait_until(lambda: s if (s := self.state())['mode'] == 'web' and s['web']['loaded'] else None, timeout=10)

    def W01(self):
        self.web_ready()
        value = '基准 Web café العربية 🙂 — once'
        before = self.fixture.web_state()
        web = wait_until(lambda: s if (s := self.fixture.web_state()).get('layout', {}).get('anchors', {}).get('web-note') else None)
        view = self.capture('web-note')
        origin = self.state()['anchors']['web-origin']
        local = web['layout']['anchors']['web-note']
        point = image_point(view, [origin[0] + local[0], origin[1] + local[1]])
        self.run('await app.click(' + json.dumps(point) + "); await app.keypress('META+A'); await app.typeText(" + json.dumps(value) + ');')
        self.run("await app.click({selector:{role:'button',text:'Commit web note'}});")
        after = wait_until(lambda: s if len((s := self.fixture.web_state())['saves']) != len(before['saves']) else None)
        require(after['note'] == value and len(after['saves']) == len(before['saves']) + 1, after)
        return {'note': after['note'], 'saves_delta': len(after['saves']) - len(before['saves'])}

    def W02(self):
        before = self.fixture.web_state()
        self.run("await app.click('Load noisy catalogue');")
        wait_until(lambda: self.fixture.web_state()['catalogue_loaded'])
        data = self.run("const v=await app.look({text:'Audit checkpoint 137'}); return {elements:v.elements,semantics:v.semantics};")
        require(any(e.get('name') == 'Audit checkpoint 137' for e in data['result']['elements']), data)
        target = next(e['id'] for e in data['result']['elements'] if e.get('name') == 'Audit checkpoint 137')
        self.run('await app.click({target:' + json.dumps(target) + '});')
        after = wait_until(lambda: s if len((s := self.fixture.web_state())['checkpoint_hits']) != len(before['checkpoint_hits']) else None)
        require(len(after['checkpoint_hits']) == len(before['checkpoint_hits']) + 1 and len(after['saves']) == len(before['saves']), after)
        return {'checkpoint_hits_delta': len(after['checkpoint_hits']) - len(before['checkpoint_hits']), 'catalogue_loaded': True}

    def W03(self):
        data = self.run(complete_text_source('Web reference'))
        result = data['result']
        require(result['length'] > 8000 and result['tail'].endswith('WEB-END-137-中文🙂'), result)
        require(result['fingerprint'] == utf16_fingerprint(self.fixture.web_state()['reference']), result)
        return result

    def P01(self):
        from computer_use_cursor import observe_cursor
        require(self.overlay_observer is not None, 'Virtual cursor acceptance needs its independent observer')
        self.run("await app.click('Native controls');")
        wait_until(lambda: self.state()['mode'] == 'native')
        proofs = []
        for operation in ('click', 'move', 'drag', 'scroll'):
            view = self.capture('pointer-binding-' + operation)
            anchors = self.state()['anchors']
            desk = anchors['gesture-desk']
            moved = [desk[0] + 30, desk[1]]
            start, end = anchors['slider-start'], anchors['slider-end']
            scrolling = anchors['native-left-scroll']
            source, expected = {
                'click': ('await app.click(' + json.dumps(image_point(view, desk)) + ');', desk),
                'move': ('await app.move(' + json.dumps(image_point(view, moved)) + ');', moved),
                'drag': ('await app.drag({path:' + json.dumps([image_point(view, start), image_point(view, end)]) + '});', end),
                'scroll': ('await app.scroll(' + json.dumps({**image_point(view, scrolling), 'delta_y': 20}) + ');', scrolling)
            }[operation]
            data, response = self.client.execute(self.app, source, allow_error=True)
            if operation == 'move' and response.get('isError') and data.get('failure', {}).get('code') == 'foreground_required' \
                    and data.get('effects', {}).get('dispatch', {}).get('not_dispatched') == 1:
                proofs.append({'operation': operation, 'passed': False, 'unsupported': 'foreground_required',
                    'failure': data['failure']})
                continue
            require(not response.get('isError'), data)
            proof = observe_cursor(self.client, self.overlay_observer, 'virtual-' + operation, expected)
            proofs.append({'operation': operation, **proof})
        available = [proof for proof in proofs if not proof.get('unsupported')]
        require(available and all(proof['passed'] for proof in available), proofs)
        require(len({proof['identity']['pid'] for proof in available}) == 1, proofs)
        return {'operations': proofs, 'unavailable': [proof['operation'] for proof in proofs if proof.get('unsupported')],
            'scope': 'actual authenticated helper bounds and colored pixels after current MCP input'}

    def P03(self):
        from computer_use_focus import run_background_contract
        require(self.focus_observer is not None, 'Focus acceptance needs its continuous independent observer')
        self.run("await app.click('Native controls');")
        binary = self.client.transport.child.args[0]
        proof = run_background_contract(self.fixture, Path(binary), self.focus_observer,
            self.output / 'focus', case=self.fixture.case + '-focus')
        require(proof['passed'], proof)
        return proof


def run_benchmark(args):
    module = fixture_module()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    output.chmod(0o700)
    identity = artifact_identity(args.dtw)
    identity.update(source_revision=args.source_revision,
        runner_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        target_manifest_sha256=hashlib.sha256((FIXTURE / 'scenarios.json').read_bytes()).hexdigest())
    report = BenchmarkReport(load_targets(), identity)
    fixture = module.ComputerUseFixture(output / 'fixture', args.case)
    client = None
    def publish():
        module.write_json(output / 'report.json', report.snapshot())
    publish()
    try:
        fixture.prepare()
        fixture.start()
        report.data['fixture_source'] = module.read_json(fixture.output / 'fixture-source.json')
        client = AppClient(args.dtw.resolve(), output / 'mcp')
        cases = CoreCases(fixture, client, output, overlay_observer=args.overlay_observer, focus_observer=args.focus_observer)
        for target in report.targets['scenarios']:
            if target['profile'] != 'core' and not (target['id'] == 'P01' and args.overlay_observer) \
                    and not (target['id'] == 'P03' and args.focus_observer):
                continue
            started = time.monotonic()
            required_mode = {'C02': 'chat', 'C03': 'chat', 'W02': 'web', 'W03': 'web'}.get(target['id'])
            if required_mode and fixture.state()['mode'] != required_mode:
                report.record(target['id'], 'blocked', {'reason': 'Previous navigation did not establish the required mode',
                    'required_mode': required_mode, 'actual_mode': fixture.state()['mode']})
                publish()
                continue
            if fixture.state()['native']['panel']['visible']:
                report.record(target['id'], 'blocked', {'reason': 'Previous system panel remains open; no additional input dispatched'})
                publish()
                continue
            if fixture.state()['chat']['menu_open']:
                report.record(target['id'], 'blocked', {'reason': 'Previous sparse popup did not close; no additional input dispatched'})
                publish()
                continue
            try:
                detail = getattr(cases, target['id'])()
                status = 'unsupported' if detail.get('unavailable') else 'pending_review' if detail.get('gui_review') == 'required' else 'passed'
                report.record(target['id'], status, detail)
            except Exception as error:
                report.record(target['id'], 'failed', {'error': str(error), 'traceback': traceback.format_exc(),
                    'classification': 'unclassified; inspect fixture/original MCP evidence before changing DTW'})
            report.data['cases'][next(i for i, row in enumerate(report.data['cases']) if row['id'] == target['id'])]['seconds'] = time.monotonic() - started
            publish()
    except BaseException as error:
        report.data['setup_error'] = {'error': str(error), 'traceback': traceback.format_exc()}
        raise
    finally:
        try:
            fixture_cleanup = fixture.close()
        except Exception as error:
            fixture_cleanup = {'passed': False, 'error': str(error)}
        finally:
            transport = None
            try:
                if client is not None:
                    transport = client.close()
            except Exception as error:
                transport = {'error': str(error)}
        transport_passed = (transport is not None and transport.get('returncode') == 0
            and not transport.get('forced') and transport.get('native_workers_reaped') is True)
        report.data['cleanup'] = {'fixture': fixture_cleanup, 'mcp': transport,
            'passed': fixture_cleanup['passed'] and transport_passed}
        report.data['tool_errors'] = [entry for entry in client.metrics if entry['is_error']] if client else []
        report.data['clean_run'] = (report.data['cleanup']['passed'] and not report.data['tool_errors']
            and report.data['setup_error'] is None)
        publish()
    result = report.snapshot()
    print(json.dumps({key: result[key] for key in ('baseline', 'counts', 'core_passed', 'target_baseline_passed')}))
    return 0 if result['core_passed'] else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--case', default='core')
    parser.add_argument('--source-revision', default='unrecorded', help='Source revision of the chosen frozen binary')
    parser.add_argument('--overlay-observer', type=Path, help='Independent exact-window cursor observer for P01')
    parser.add_argument('--focus-observer', type=Path, help='Independent continuous focus observer for P03')
    args = parser.parse_args()
    if sys.platform != 'darwin':
        parser.error('Native core profile currently requires macOS; web-only fixture is portable')
    raise SystemExit(run_benchmark(args))


if __name__ == '__main__':
    main()
