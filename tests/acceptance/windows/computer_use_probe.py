"""Windows core of the shared Computer Use workbench; public apps/exec only.

Native UIA, dense controls, UTF-16 paging, owned modals and real Open/Save panels
use the same Windows fixture as the smoke audits. Unselected 42-target scenarios
remain not_run. This direct-script probe does not claim model/image acceptance.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from app_client import AppClient, artifact_identity, require
from windows.app_protocol_probe import wait
from windows.transport import read


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--fixture-binary', type=Path)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    here = Path(__file__).resolve().parents[2] / 'fixtures/computer-use-app'
    spec = importlib.util.spec_from_file_location('windows_computer_use_fixture', here / 'fixture.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    targets = json.loads((here / 'scenarios.json').read_text(encoding='utf-8'))
    report = {'passed': False, 'artifact': artifact_identity(args.dtw), 'scope': 'Windows direct-script core',
              'target_baseline_passed': False,
              'cases': [{**target, 'status': 'not_run'} for target in targets['scenarios']]}
    fixture = module.ComputerUseFixture(out / 'fixture', 'windows-core')
    client = None
    stage = 'setup'

    def state():
        # Atomic replacement can briefly report no publication. Only a real
        # same-process state is an oracle; absence is bounded readiness polling.
        return wait(lambda: fixture.state())

    def execute(source, allow_error=False):
        return client.execute(fixture.bundle, source, allow_error=allow_error)[0]

    def record(case_id, detail):
        next(row for row in report['cases'] if row['id'] == case_id).update(status='passed', detail=detail)
        print(case_id + ': passed', flush=True)

    try:
        fixture.prepare(build=False)
        fixture.build(compiled_native=args.fixture_binary)
        fixture.start()
        client = AppClient(args.dtw.resolve(), out / 'protocol')
        stage = 'N01'
        # A genuinely cold connection: no look/capture before the known input.
        cold = execute('await app.click({selector:{role:"button",text:"Commit document"}});')
        wait(lambda: state()['commits'] == 1)
        record(stage, cold)
        stage = 'N02'
        focused = execute('const v=await app.look({role:"button",text:"Checkpoint 137"});return {elements:v.elements,query:v.semantics.query};')
        require(len(focused['result']['elements']) == 1, focused)
        execute('await app.click({selector:{role:"button",text:"Checkpoint 137"}});')
        wait(lambda: state()['checkpoint'] == 137)
        record(stage, focused)
        stage = 'N03'
        # Commit explicitly focuses the editor. Checkpoint does not authorize typing.
        execute('await app.click({selector:{role:"button",text:"Commit document"}});')
        payload = 'Windows 超级 APP 😀\r\nsecond line café 🚀'
        execute('await app.keypress("CTRL+A");await app.typeText(' + json.dumps(payload) + ');await app.click({selector:{role:"button",text:"Commit document"}});')
        wait(lambda: state()['saved'] == payload and state()['commits'] == 3)
        record(stage, {'exact_text': payload, 'commits': 3})
        stage = 'N04'
        looked = execute('const v=await app.look({role:"text_field",text:"Long native reference"});return v.elements;')
        elements = looked['result']
        require(len(elements) == 1, looked)
        element = elements[0]
        require(element.get('value_truncated') is True or element.get('name_truncated') is True,
                {'missing_truncation_evidence': element})
        target = element['id']
        chunks, pages, offset = [], [], 0
        while True:
            page = execute('return await app.readText(' + json.dumps({'target': target, 'offset': offset, 'limit': 4095}) + ');')['result']
            require(page['offset'] == offset and page['unit'] == 'utf16', page)
            chunks.append(page['text'])
            pages.append({key: page[key] for key in ('offset', 'next_offset', 'total_length', 'unit')})
            count = len(page['text'].encode('utf-16-le')) // 2
            if page['next_offset'] is None:
                require(offset + count == page['total_length'], page)
                break
            require(page['next_offset'] == offset + count and count > 0, page)
            offset = page['next_offset']
        reference = (fixture.output / 'native-reference.txt').read_bytes().decode('utf-8')
        actual = ''.join(chunks)
        require(actual == reference, {'expected_length': len(reference), 'actual_length': len(actual),
                                      'expected_tail': reference[-80:], 'actual_tail': actual[-80:]})
        execute('await app.look();')
        old = execute('return await app.readText(' + json.dumps(target) + ');', True)
        require(old.get('error', {}).get('code') in ('target_changed', 'stale_observation', 'stale_scene', 'element_not_found'), old)
        record(stage, {'pages': pages, 'sha256': hashlib.sha256(actual.encode('utf-8')).hexdigest(), 'stale': old})
        stage = 'N05'
        execute('await app.click({selector:{role:"checkbox",text:"Reviewed"}});')
        wait(lambda: state()['reviewed'] and state()['checks'] == 1)
        record(stage, state())
        stage = 'N08'
        menu = execute('return await app.look({scope:"menu",text:"Workbench File"});')
        require(menu['view']['elements'], menu)
        record(stage, menu)
        stage = 'owned_popup_sequence'
        sequence = execute('await app.click({selector:{role:"button",text:"Show owned modal"}});await app.click({selector:{role:"button",text:"Close owned modal"}});')
        wait(lambda: not state()['modal'])
        report['popup_sequence'] = sequence
        stage = 'N10'
        execute('await app.click({selector:{role:"button",text:"Open document"}});')
        wait(lambda: state()['modal'])
        opened = client.capture(fixture.bundle, 'native-open')
        require(opened['view']['windows'][0]['title'] == 'Open owned document', opened)
        execute('await app.keypress("ESC");')
        wait(lambda: not state()['modal'])
        require(state()['opens'] == 0 and state()['text'] == payload, state())
        record(stage, opened)
        stage = 'N11'
        destination = fixture.output / 'saved-owned.txt'
        execute('await app.click({selector:{role:"button",text:"Save document as"}});')
        wait(lambda: state()['modal'])
        execute('await app.keypress("ALT+N");await app.typeText(' + json.dumps(str(destination)) + ');await app.keypress("ENTER");')
        wait(lambda: not state()['modal'] and state()['saves'] == 1)
        require(destination.read_bytes().decode('utf-8') == payload, 'save bytes differ')
        execute('await app.click({selector:{role:"button",text:"Open document"}});')
        wait(lambda: state()['modal'])
        execute('await app.keypress("ALT+N");await app.typeText(' + json.dumps(str(destination)) + ');await app.keypress("ENTER");')
        wait(lambda: not state()['modal'] and state()['opens'] == 1)
        require(state()['text'] == payload, 'reopened document differs')
        record(stage, {'saved_sha256': hashlib.sha256(destination.read_bytes()).hexdigest(), 'reopened': state()})
        stage = 'N12'
        execute('await app.click({selector:{role:"button",text:"Save document as"}});')
        wait(lambda: state()['modal'])
        execute('await app.keypress("ALT+N");await app.typeText(' + json.dumps(str(destination)) + ');await app.keypress("ENTER");')
        confirm = execute('await app.look({full:true});')
        require(len(confirm['view']['windows']) >= 3, confirm)
        # The standard overwrite panel has Yes/No, without an IDCANCEL button;
        # Escape is not a rejection on this Windows dialog. Choose the actual
        # observed No control, then cancel the still-open Save panel separately.
        reject = next((row for row in confirm['view']['elements']
                       if row['role'] == 'button' and row['name'] in ('No', '否(N)', '&No')), None)
        require(reject, confirm)
        execute('await app.click(' + json.dumps(reject['id']) + ');')
        save_panel = execute('await app.look({full:true});')
        require(save_panel['view']['windows'][0]['title'] == 'Save owned document', save_panel)
        execute('await app.keypress("ESC");')
        wait(lambda: not state()['modal'])
        require(state()['saves'] == 1 and destination.read_bytes().decode('utf-8') == payload, state())
        record(stage, confirm)
        execute('await app.quit();')
        fixture.child.wait(timeout=8)
        report['passed'] = True
    except BaseException as error:
        report.update(failed_stage=stage, error=type(error).__name__ + ': ' + str(error))
        if stage in [row['id'] for row in report['cases']]:
            next(row for row in report['cases'] if row['id'] == stage).update(status='failed', detail=report['error'])
        raise
    finally:
        try:
            if client:
                report['mcp_cleanup'] = client.close()
        except BaseException as error:
            report.update(passed=False, cleanup_error=str(error))
        report['fixture_cleanup'] = fixture.close()
        report['passed'] &= report['fixture_cleanup']['passed']
        (out / 'report.json').write_text(json.dumps(report, ensure_ascii=True, indent=2) + '\n', encoding='utf-8')
    require(report['passed'], report)


if __name__ == '__main__':
    main()
