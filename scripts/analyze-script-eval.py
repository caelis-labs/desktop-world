#!/usr/bin/env python3
"""Summarize script/transport costs without emitting desktop content or code.
Bytes are transport measurements, never estimates of model tokens.
"""
import argparse
from collections import Counter
from datetime import datetime
import json
from pathlib import Path

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('run', type=Path, help='packet containing participant/ and evaluator/')
p.add_argument('--output', type=Path)
a = p.parse_args()
h = a.run / 'participant/harness'
def rows(path):
    if not path.exists(): return []
    return [json.loads(s) for s in path.read_text().splitlines() if s.strip()]
def instant(s): return datetime.fromisoformat(s.replace('Z', '+00:00'))
submission = json.loads((a.run/'evaluator/submission.json').read_text())
started = instant(submission['dispatch_started_utc'])
def seconds(s): return round((instant(s)-started).total_seconds(), 3) if s else None
scripts = rows(h/'scripts.jsonl')
wire = rows(h/'wire.jsonl')
requests = {}
operations = Counter()
faults = Counter()
action_outcomes = Counter()
first_observe = first_action = None
native_ms = 0
for row in wire:
    data = row['data']
    if row['direction'] == 'request':
        requests[data['id']] = row
        operations[data['op']] += 1
    elif data.get('id') in requests:
        req = requests[data['id']]
        native_ms += 1000*(instant(row['at'])-instant(req['at'])).total_seconds()
        result = data.get('result', {})
        if data.get('error'): faults[data['error']['code']] += 1
        if req['data']['op']=='observe' and result.get('objects') and not data.get('error'):
            first_observe = first_observe or row['at']
        if req['data']['op']=='act':
            action_outcomes[result.get('outcome','error')] += 1
            if result.get('outcome')=='completed': first_action = first_action or row['at']
visible = next((s['at'] for s in scripts if s.get('printed_bytes',0)>0 and not s.get('error')), None)
result = {
    'run':a.run.name, 'timing_origin':'evaluator dispatch; host preparation excluded',
    'script_count':len(scripts), 'helper_operations':dict(operations),
    'script_errors':dict(Counter(s['error'] for s in scripts if s.get('error'))),
    'helper_faults':dict(faults), 'action_outcomes':dict(action_outcomes),
    'first_native_observe_seconds':seconds(first_observe),
    'first_successful_print_seconds':seconds(visible),
    'first_completed_action_seconds':seconds(first_action),
    'last_script_seconds':seconds(scripts[-1]['at']) if scripts else None,
    'helper_elapsed_ms':round(native_ms,1),
    'totals':{k:sum(s.get(k,0) for s in scripts) for k in ['calls','elapsed_ms','request_bytes','helper_response_bytes','printed_bytes','model_response_bytes','script_bytes']},
    'model_input_tokens':None,'model_output_tokens':None,
    'limitations':['Print success needs content review to establish useful observation.', 'Completed action is not proof of task completion.', 'Byte counts exclude prompt, help, shell wrapping, model reasoning and host context.', 'Token counts and compactions require separate host evidence.'],
}
text = json.dumps(result,ensure_ascii=False,indent=2)+'\n'
if a.output: a.output.write_text(text)
print(text)
