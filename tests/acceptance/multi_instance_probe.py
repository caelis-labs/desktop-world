#!/usr/bin/env python3
"""Opt-in macOS GUI acceptance: one logical app, two actual process instances.

Run with exclusive desktop access and a freshly built DTW binary. Both processes
execute the exact same owned .app executable and bundle ID. Only app/source go
over ordinary MCP; public window/observation refs route every operation. Fixture
state and saved files independently prove effects. Startup waits only for the
fixture's own WindowServer publication; no DTW call or input is retried. No permission repair,
external documents, Agent configuration, or live-model evidence are involved.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import time
import traceback
import uuid

from standard_agent_probe import Fixture, require, save_png, successful, wait_until, write_json
from native_input_probe import event_pairs, initialized, saved_equals


TITLE_A = "DTW Multi Instance A"
TITLE_B = "DTW Multi Instance B"
MARKER = "same-logical-app-state"
SELECT = """
const inventory=await app.windows();
const one=title=>{const found=inventory.windows.filter(w=>w.title===title);
if(found.length!==1)throw new Error('Owned title not unique: '+title);return found[0].window_ref;};
"""
SAVE_OBSERVATION = """
async function saveObservation(window_ref){
const p=await app.observe({window_ref,role:'button',text:'Save',limit:10});
const elements=p.elements.filter(e=>e.name==='Save'&&e.can_press);
if(elements.length!==1)throw new Error('Owned Save action not unique');
return {observation_ref:p.observation_ref,element_ref:elements[0].element_ref};}
"""
STATE_CHECK = f"if(state.marker!=={json.dumps(MARKER)})throw new Error('Logical app state was reset');"


def configure(root, title, x):
    root.mkdir(parents=True, exist_ok=True)
    write_json(root / 'fixture.json', {'title': title, 'x': x})


def wait_for_published_surface(fixture, initial, evidence, label, timeout=5):
    """Read fixture state until its exact PID/window is published and on screen."""
    started = time.monotonic()
    trace = {'timeout_seconds': timeout, 'samples': [], 'passed': False}
    evidence.setdefault('publication_waits', {})[label] = trace
    state = initial
    while True:
        elapsed = time.monotonic() - started
        surface = state.get('window_server', {})
        windows = state.get('windows', [])
        sample = {key: state.get(key) for key in ('pid', 'ready', 'active', 'key_window', 'windows', 'window_server')}
        sample['elapsed_ms'] = round(elapsed * 1000, 3)
        trace['samples'].append(sample)
        require(fixture.child.poll() is None, 'Owned fixture exited while awaiting WindowServer publication')
        if (elapsed <= timeout and state.get('pid') == fixture.child.pid
                and len(windows) == 1 and windows[0].get('visible') is True
                and surface.get('query_succeeded') is True and surface.get('present') is True
                and surface.get('owner_pid') == fixture.child.pid
                and surface.get('window_id') == windows[0].get('id')
                and surface.get('on_screen') is True):
            trace.update({'passed': True, 'elapsed_ms': sample['elapsed_ms']})
            return state
        require(elapsed < timeout, f'Owned surface was not published/on-screen within {timeout}s; see publication_waits.{label}')
        time.sleep(min(0.02, timeout - elapsed))
        state = fixture.state()


def fingerprint(fixture):
    state = fixture.state()
    return {key: state[key] for key in ('text', 'saved', 'saves', 'pad')}


def invoke(mcp, app_id, case, source, evidence):
    record, response = mcp.execute(app_id, case, source)
    evidence[case] = {'record': record}
    value = successful(record)
    return value, response, record


def no_reset(record):
    require(record.get('state_reset') is False and record.get('context_reset') is False,
            'Membership change replaced the logical app context or state')


def denied(mcp, app_id, case, expression, evidence, expected_operation):
    source = STATE_CHECK + "try{" + expression + ";return {rejected:false};}catch(e){return {rejected:true,code:e.code};}"
    value, _, record = invoke(mcp, app_id, case, source, evidence)
    no_reset(record)
    require(value.get('rejected') and isinstance(value.get('code'), str), record)
    steps = record['script']['steps']
    require(len(steps) == 1 and steps[0]['operation'] == expected_operation and steps[0].get('error'), record)
    receipt = steps[0].get('receipt')
    require(receipt and receipt['outcome']['dispatch'] == 'not_dispatched', record)
    provider = receipt.get('provider_outcome')
    require(provider is None or provider['dispatch'] == 'not_dispatched', record)


def point_in_capture(fixture, shot):
    state = fixture.state()
    bounds, point = state['window_bounds'], state['pad_screen_point']
    x = (point[0] - bounds[0]) / bounds[2] * shot['image']['width']
    y = (point[1] - bounds[1]) / bounds[3] * shot['image']['height']
    require(0 < x < shot['image']['width'] and 0 < y < shot['image']['height'], 'Pad outside owned capture')
    return x, y


def edit(mcp, app_id, slot, text, fixture, peer, evidence):
    before = fingerprint(peer)
    source = STATE_CHECK + f"""
const window_ref=state.{slot};
await app.focus({{window_ref}});await app.keypress({{window_ref,keys:['META','A']}});
await app.typeText({{window_ref,text:{json.dumps(text)}}});
return await app.keypress({{window_ref,keys:['META','S']}});
"""
    _, _, record = invoke(mcp, app_id, 'edit_' + slot, source, evidence)
    no_reset(record)
    evidence['edit_' + slot]['independent_state'] = saved_equals(fixture, text)
    require(fingerprint(peer) == before, 'Input changed the peer instance')


def exercise(mcp, app_id, a, b, evidence, resources, closed):
    catalog, _ = mcp.tool('apps', {})
    evidence['catalog'] = catalog
    own = [row for row in catalog['apps'] if row['app'] == app_id]
    require(len(own) == 1 and own[0].get('instances') == 2, 'Two processes were not grouped under one app')
    source = SELECT + f"state.a=one({json.dumps(TITLE_A)});state.b=one({json.dumps(TITLE_B)});state.marker={json.dumps(MARKER)};return {{inventory,a:state.a,b:state.b}};"
    initial, _, _ = invoke(mcp, app_id, 'aggregate', source, evidence)
    require(initial['a'] != initial['b'], 'Distinct process windows share a public ref')
    edit(mcp, app_id, 'a', 'Instance A 中文 saved', a, b, evidence)
    edit(mcp, app_id, 'b', 'Instance B café saved', b, a, evidence)

    source = STATE_CHECK + SAVE_OBSERVATION + """
state.aSave=await saveObservation(state.a);state.bSave=await saveObservation(state.b);
state.aShot=await app.capture({window_ref:state.a});state.bShot=await app.capture({window_ref:state.b});
return {aSave:state.aSave,bSave:state.bSave,aShot:state.aShot,bShot:state.bShot};
"""
    refs, response, record = invoke(mcp, app_id, 'retain_both_instances', source, evidence)
    no_reset(record)
    image_dir = mcp.output / 'retained-b-capture'; image_dir.mkdir()
    evidence['retain_both_instances']['png'] = save_png(image_dir, response, refs['bShot'])
    before = [fingerprint(a), fingerprint(b)]
    denied(mcp, app_id, 'cross_instance_semantic',
           'await app.press({window_ref:state.b,observation_ref:state.aSave.observation_ref,element_ref:state.aSave.element_ref})',
           evidence, 'press')
    denied(mcp, app_id, 'cross_instance_pixels',
           'await app.click({window_ref:state.b,observation_ref:state.aShot.observation_ref,x:20,y:20})',
           evidence, 'click')
    require([fingerprint(a), fingerprint(b)] == before, 'Cross-instance ref rejection changed an owned document')
    evidence['cross_instance_independent_state'] = [a.state(), b.state()]

    result = a.close(); closed.add(id(a)); evidence['departed_a_cleanup'] = result
    require(result.get('returncode') == 0 and not result.get('forced'), result)
    x, y = point_in_capture(b, refs['bShot'])
    before_pad = b.state()['pad']['left_up']
    source = STATE_CHECK + SELECT + f"""
if(inventory.windows.some(w=>w.title==={json.dumps(TITLE_A)}))throw new Error('Departed instance remains');
if(one({json.dumps(TITLE_B)})!==state.b)throw new Error('Surviving window ref changed');
await app.click({{window_ref:state.b,observation_ref:state.bShot.observation_ref,x:{x},y:{y}}});
""" + SAVE_OBSERVATION + "state.bSave=await saveObservation(state.b);return {b:state.b,marker:state.marker};"
    _, _, record = invoke(mcp, app_id, 'survivor_after_a_exit', source, evidence)
    no_reset(record)
    evidence['survivor_after_a_exit']['independent_state'] = wait_until(
        lambda: b.state() if b.state()['pad']['left_up'] == before_pad + 1 else None)

    restart_root = a.root.parent / 'instance-a-restarted'
    configure(restart_root, TITLE_A, 120)
    # Reuse the exact bundle/executable. Only the process birth identity changes.
    restarted = Fixture(restart_root, app_id, a.executable); resources.append(('fixture-a-restarted', restarted))
    evidence['restart'] = restarted.start()
    require(restarted.child.pid != a.child.pid, 'Restart did not create a distinct process')
    wait_for_published_surface(restarted, evidence['restart'], evidence, 'restarted_a')
    source = STATE_CHECK + SELECT + f"""
if(one({json.dumps(TITLE_B)})!==state.b)throw new Error('Surviving ref changed after restart');
state.aNew=one({json.dumps(TITLE_A)});
if(state.aNew===state.a)throw new Error('New process reused departed window ref');
return {{aOld:state.a,aNew:state.aNew,b:state.b,marker:state.marker}};
"""
    _, _, record = invoke(mcp, app_id, 'restart_inventory', source, evidence)
    no_reset(record)
    before = [fingerprint(restarted), fingerprint(b)]
    denied(mcp, app_id, 'departed_window_ref', 'await app.focus({window_ref:state.a})', evidence, 'focus')
    denied(mcp, app_id, 'departed_semantic_ref',
           'await app.press({window_ref:state.aNew,observation_ref:state.aSave.observation_ref,element_ref:state.aSave.element_ref})',
           evidence, 'press')
    denied(mcp, app_id, 'departed_pixel_ref',
           'await app.click({window_ref:state.aNew,observation_ref:state.aShot.observation_ref,x:20,y:20})',
           evidence, 'click')
    require([fingerprint(restarted), fingerprint(b)] == before, 'Departed ref was retargeted to the new process')
    evidence['departed_ref_independent_state'] = [restarted.state(), b.state()]
    before_saves = b.state()['saves']
    _, _, record = invoke(mcp, app_id, 'surviving_semantic_ref_after_restart', STATE_CHECK +
        'return await app.press({window_ref:state.b,observation_ref:state.bSave.observation_ref,element_ref:state.bSave.element_ref});', evidence)
    no_reset(record)
    evidence['surviving_semantic_ref_after_restart']['independent_state'] = wait_until(
        lambda: b.state() if b.state()['saves'] == before_saves + 1 else None)
    edit(mcp, app_id, 'aNew', 'Restarted A 新进程 saved', restarted, b, evidence)
    evidence['key_pairs'] = [event_pairs(b), event_pairs(restarted)]
    evidence['final_state'] = [restarted.state(), b.state()]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    require(sys.platform == 'darwin', 'Requires exclusive real macOS GUI access')
    output = args.output.absolute(); output.mkdir(mode=0o700)
    binary = args.dtw.resolve(strict=True)
    source = Path(__file__).resolve().parents[1] / 'fixtures/native-input/main.swift'
    app_id = 'dev.dtw.multi.' + uuid.uuid4().hex
    evidence = {'kind':'same-bundle-same-executable-multi-instance-standard-MCP', 'live_llm_host':False,
                'app':app_id, 'started_unix_ms':int(time.time()*1000),
                'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
                'fixture_source_sha256':hashlib.sha256(source.read_bytes()).hexdigest()}
    resources, closed = [], set(); failure = None
    try:
        configure(output/'instance-a', TITLE_A, 120)
        a = Fixture.prepare(output/'instance-a', app_id, source); resources.append(('fixture-a', a))
        configure(output/'instance-b', TITLE_B, 730)
        b = Fixture(output/'instance-b', app_id, a.executable); resources.append(('fixture-b', b))
        evidence['initial_state'] = []
        for label, fixture in [('initial_a', a), ('initial_b', b)]:
            state = fixture.start()
            evidence['initial_state'].append(state)
            wait_for_published_surface(fixture, state, evidence, label)
        require(a.child.pid != b.child.pid and a.executable == b.executable, 'Not two instances of one executable')
        mcp = initialized(binary, output/'mcp'); resources.append(('mcp', mcp))
        exercise(mcp, app_id, a, b, evidence, resources, closed)
    except BaseException as error:
        failure = {'type':type(error).__name__, 'message':str(error)}
        (output/'failure.txt').write_text(traceback.format_exc())
    finally:
        evidence['cleanup'] = []
        # Reap the MCP/native owner before terminating surviving target apps.
        for name, resource in sorted(resources, key=lambda pair: pair[0] != 'mcp'):
            if id(resource) in closed:
                continue
            try:
                result = resource.close(); evidence['cleanup'].append({'resource':name, **result})
                require(not result.get('forced') and result.get('returncode', 0) == 0, result)
            except BaseException as error:
                failure = failure or {'type':'CleanupFailure', 'message':str(error)}
        evidence.update({'failure':failure, 'passed':failure is None, 'finished_unix_ms':int(time.time()*1000)})
        write_json(output/'evidence.json', evidence)
    print(json.dumps({'passed':evidence['passed'], 'evidence':str(output/'evidence.json'), 'failure':failure}, ensure_ascii=False))
    return 0 if evidence['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
