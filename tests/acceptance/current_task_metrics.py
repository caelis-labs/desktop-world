"""Current apps/exec transcript checks; business effects require independent oracles."""
from browser_workbench_probe import decode_output, denied_route_attempt, json_bytes
from app_client import require


def selected_calls(calls, name):
    for number, call in enumerate(calls, 1):
        arguments = call.get('arguments') or {}
        if call.get('name') == 'call_mcp_tool' and arguments.get('ServerName') == name:
            yield number, call, arguments.get('ToolName'), arguments.get('Arguments')
        elif str(call.get('name', '')).startswith('mcp__' + name + '__'):
            yield number, call, call['name'][len('mcp__' + name + '__'):], arguments


def check_effects(effects):
    require(isinstance(effects, dict), 'Invalid effects')
    for field in ('dispatch', 'evidence'):
        if field in effects:
            require(isinstance(effects[field], dict) and all(
                type(count) is int and count >= 0 for count in effects[field].values()), 'Invalid effects counter')
    dispatch = effects.get('dispatch', {})
    require(set(dispatch) <= {'dispatched', 'not_dispatched', 'unknown'}, 'Unknown dispatch state')
    require(not dispatch.get('unknown') and not effects.get('interrupted'), 'Uncertain or interrupted effects')


def check_execution(data, arguments, allowed_apps):
    require(isinstance(arguments, dict) and set(arguments) == {'app', 'source'}, 'exec requires exactly app/source')
    require(arguments['app'] in allowed_apps, 'Unexpected target app')
    require(isinstance(arguments['source'], str) and len(arguments['source'].encode()) <= 65536, 'Invalid source')
    require(not ({'value', 'script', 'receipt'} & data.keys()), 'Historical execution payload')
    require(data.get('app') == arguments['app'], 'Execution app mismatch')
    require(isinstance(data.get('execution_id'), str) and data['execution_id'], 'Missing execution identity')
    require(data.get('status') == 'completed', 'Execution has not settled')
    if 'view' in data:
        require(isinstance(data['view'], dict), 'Invalid compact view')
    if 'effects' in data:
        effects = data['effects']
        check_effects(effects)
        if 'focus_preparation' in effects:
            check_effects(effects['focus_preparation'])
    require(not data.get('failure'), 'Operation failure retained')


def metrics(calls, name, *, allowed_apps):
    issues = [{'agent_call': number, 'kind': 'denied_route_attempt'}
              for number, call in enumerate(calls, 1) if denied_route_attempt(call)]
    executions = []; total_json = total_images = image_count = source_bytes = 0
    images_complete = True; selected = list(selected_calls(calls, name)); previous_failed = False
    durations = []
    for number, call, method, arguments in selected:
        row = {'agent_call': number, 'execution_id': None, 'follows_failed_exec': previous_failed}
        failed = False
        try:
            require(not call.get('normalization_error'), call.get('normalization_error'))
            scope = call.get('acp_image_scope')
            require(scope is None or scope['namespace'] == name, 'Image namespace mismatch')
            data, is_error, images = decode_output(call.get('payload_output', call.get('output')), scope)
            total_json += json_bytes(data)
            if images is None:
                images = call.get('image_bytes')
            if call.get('image_unavailable') or images is None:
                images_complete = False
            else:
                total_images += images
            # Image bytes/count describe exposed payloads, not model admission.
            if images:
                image_count += 1
            if method == 'exec':
                row.update(execution_id=data.get('execution_id'), status=data.get('status'),
                           error=data.get('error'), failure=data.get('failure'), effects=data.get('effects'))
                check_execution(data, arguments, allowed_apps)
                source_bytes += len(arguments['source'].encode())
            elif method == 'apps':
                require(isinstance(arguments, dict) and set(arguments) <= {'query', 'state', 'offset', 'limit'}, 'Invalid apps arguments')
                require('version' not in data and isinstance(data.get('apps'), list)
                        and isinstance(data.get('coverage'), dict)
                        and 'next' in data, 'Current application catalog missing')
                for app in data['apps']:
                    require(isinstance(app, dict) and isinstance(app.get('app'), str)
                            and isinstance(app.get('name'), str) and type(app.get('running')) is bool, 'Invalid app row')
            else:
                raise AssertionError('Non-current MCP method: ' + str(method))
            require(not is_error and not data.get('error') and not data.get('shutdown_error')
                    and not data.get('shutdown_errors'), 'Tool or cleanup failure')
            require(call.get('status') in ('completed', 'DONE'), 'Agent tool call did not complete')
        except (AssertionError, ValueError, TypeError, KeyError, UnicodeError) as error:
            failed = True
            row['normalization_error'] = str(error)
            issues.append({'agent_call': number, 'method': method, 'invalid_output': str(error)})
            images_complete = False
        duration = call.get('duration_seconds')
        if isinstance(duration, (int, float)) and duration >= 0:
            durations.append(duration)
        if method == 'exec':
            row['failed'] = failed
            previous_failed = failed
            executions.append(row)
    if not executions:
        issues.append({'kind': 'missing_task_execution'})
    return {'protocol': 'apps/exec', 'agent_tool_calls': len(calls), 'mcp_calls': len(selected),
            'exec_calls': len(executions), 'mcp_json_bytes': total_json, 'source_bytes': source_bytes,
            'image_bytes': total_images if images_complete else None,
            'image_bearing_calls': image_count, 'image_measurement': 'exposed_payload; model admission requires harness evidence',
            'mcp_call_seconds': sum(durations) if len(durations) == len(selected) else None,
            'timing_measurement': 'harness tool duration; includes transport, not isolated native time',
            'read_refreshes': None, 'script_host_calls': None,
            'counter_boundary': 'compact output has no method/read-refresh counters; do not infer them from source',
            'duplicate_business_writes': None, 'write_measurement': 'independent fixture journal, never dispatch counts',
            'executions': executions, 'issues': issues}


def recovery_metrics(calls, name):
    apps = {arguments.get('app') for _, _, method, arguments in selected_calls(calls, name)
            if method == 'exec' and isinstance(arguments, dict) and isinstance(arguments.get('app'), str)}
    rows = metrics(calls, name, allowed_apps=apps)['executions']
    return {'executions': rows, 'attempted_exec': len(rows),
            'failed_exec': sum(row['failed'] for row in rows),
            'successful_exec': sum(not row['failed'] for row in rows),
            'followup_exec_after_failure': sum(row['follows_failed_exec'] for row in rows),
            'retry_measurement': 'Following a failure is a review candidate, not proof of replay; inspect source and independent journal.'}
