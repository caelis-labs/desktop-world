"""Bounded evaluation facts from DSH's public ACP journal, without private prose.

These facts supplement independent business/desktop oracles. An admitted image
part does not prove its pixels, and a completed tool does not prove consumption.
"""
import collections
import json
from pathlib import Path


def summarize(path):
    calls, parse_errors, unmatched_updates = {}, 0, 0
    for line in Path(path).read_text(encoding='utf-8').splitlines():
        try:
            row = json.loads(line)
            update = row.get('message', {}).get('params', {}).get('update', {})
        except (ValueError, AttributeError):
            parse_errors += 1
            continue
        if not isinstance(update, dict):
            parse_errors += 1
            continue
        kind, key = update.get('sessionUpdate'), update.get('toolCallId')
        if kind == 'tool_call':
            calls[key] = {'tool': update.get('title'), 'status': update.get('status'),
                'admitted_image_parts': 0, 'error_codes': [], 'dispatch': {}}
        elif kind == 'tool_call_update' and key not in calls:
            unmatched_updates += 1
        elif kind == 'tool_call_update':
            call = calls[key]
            call['status'] = update.get('status')
            for content in update.get('content', []):
                content = content.get('content', {})
                if content.get('type') == 'image':
                    call['admitted_image_parts'] += 1
                elif content.get('type') == 'text':
                    try:
                        # DSH prefixes MCP isError replies and may append the
                        # explicit image-not-admitted note after their JSON.
                        result, _ = json.JSONDecoder().raw_decode(
                            content.get('text', '').removeprefix('Error: ').lstrip())
                    except ValueError:
                        continue
                    if isinstance(result, dict) and isinstance(result.get('error'), dict):
                        call['error_codes'].append(result['error'].get('code', 'unknown'))
                    if isinstance(result, dict):
                        effects = result.get('effects')
                        dispatch = effects.get('dispatch') if isinstance(effects, dict) else None
                        dispatch = dispatch if isinstance(dispatch, dict) else {}
                        call['dispatch'] = {key: value for key, value in dispatch.items()
                            if key in ('dispatched', 'not_dispatched', 'unknown') and type(value) is int}
    statuses = collections.Counter(call['status'] for call in calls.values())
    return {'tool_calls': len(calls),
        'tools': dict(collections.Counter(call['tool'] for call in calls.values())),
        'statuses': dict(statuses),
        'failed_error_codes': dict(collections.Counter(code for call in calls.values()
            if call['status'] == 'failed' for code in call['error_codes'])),
        'admitted_image_parts': sum(call['admitted_image_parts'] for call in calls.values()),
        'journal_parse_errors': parse_errors,
        'unmatched_tool_updates': unmatched_updates,
        'dispatch_counts': dict(sum((collections.Counter(call['dispatch'])
            for call in calls.values()), collections.Counter())),
        'tools_with_unknown_dispatch': sum(bool(call['dispatch'].get('unknown')) for call in calls.values()),
        'tool_error_free': bool(calls) and not parse_errors and not unmatched_updates
            and all(call['status'] == 'completed' for call in calls.values()),
        'scope': 'Public ACP tool facts only; business effects, image pixels and cleanup require independent oracles'}


if __name__ == '__main__':
    import sys
    print(json.dumps(summarize(sys.argv[1]), indent=2))
