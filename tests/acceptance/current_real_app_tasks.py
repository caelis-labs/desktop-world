#!/usr/bin/env python3
"""Opt-in current apps/exec real Agent tasks (macOS Chrome/WPS/Obsidian).

Example: python3 tests/acceptance/current_real_app_tasks.py --dtw /path/to/dtw
  --output /new/private/evidence --task chrome --acp-adapter /path/to/dsh-wrapper

Defaults to the isolated DSH harness. Reuses historical disposable fixtures,
independent business oracles and Agent launch plumbing, never its GUI protocol.
No task automatically passes GUI review. WPS documents remain open for review
and manual close; the current API does not target inactive tabs by window_ref.
"""
from pathlib import Path
import sys

from browser_workbench_probe import check_routes as historical_routes, skill_paths

from app_client import AppClient, require
import current_task_metrics
import real_app_tasks as historical


def initialize(binary, path):
    return AppClient(binary, path)


def inventory(client, app):
    # Wrap the projection so it is retained as task JSON in result, even when
    # compact view is unchanged or the full snapshot is otherwise coalesced.
    data, _ = client.execute(app, 'const s=await app.look({full:true});'
                            'return {task_windows:s.windows,coverage:s.coverage};')
    result = data.get('result') or {}
    require(isinstance(result, dict) and isinstance(result.get('task_windows'), list),
            'Current task window inventory missing')
    snapshot = {'windows': result['task_windows'], 'coverage': result.get('coverage')}
    require(inventory_complete(snapshot), 'Incomplete current window inventory')
    require(all(isinstance(row, dict) and isinstance(row.get('title'), str)
                and row.get('title_truncated') is False for row in snapshot['windows']),
            'Window title unavailable or truncated; cannot prove task ownership')
    return snapshot


def inventory_complete(snapshot):
    coverage = snapshot.get('coverage') or {}
    return coverage.get('complete_within_scope') is True and coverage.get('windows_truncated') is False


def check_routes(calls, name, root):
    # Current Skill can load its API reference through ordinary read-only tools.
    api_paths = {str(Path(path).with_name('API.md')) for path in skill_paths(name, root)}
    remaining = []
    for call in calls:
        arguments = call.get('arguments') or {}
        path = arguments.get('Path', arguments.get('file_path'))
        if call.get('name') in ('Read', 'read_file', 'read') and path in api_paths:
            continue
        remaining.append(call)
    historical_routes(remaining, name, root)


def run_agent(repo, root, binary, client, adapter, name, prompt, evidence, **kwargs):
    historical.run_agent(repo, root, binary, client, adapter, name, prompt, evidence,
                         **kwargs, tools=('apps', 'exec'),
                         measure=current_task_metrics.metrics,
                         measure_recovery=current_task_metrics.recovery_metrics,
                         current_routes=check_routes)
    evidence['business_oracle'] = 'original task fixture/readback; duplicate writes checked by journal'


def main():
    return historical.main(current=sys.modules[__name__])


if __name__ == '__main__':
    raise SystemExit(main())
