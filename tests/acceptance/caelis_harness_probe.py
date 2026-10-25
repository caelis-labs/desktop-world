"""Optional real-model Caelis acceptance in an isolated Store and workspace.

Uses the user's existing local, unauthenticated model gateway configuration.
No credentials, personal sessions or production plugin registrations are copied.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess

def run_caelis(root, package, manifest, downloads):
    executable = shutil.which('caelis')
    assert executable, 'Caelis CLI is required for --caelis'
    original = Path(os.environ.get('CAELIS_ACCEPTANCE_CONFIG', Path.home()/'.caelis/config.json'))
    source = json.loads(original.read_text())
    endpoints = source['models']['provider_endpoints']
    assert all(e.get('auth_type') == 'none' and e['base_url'].startswith('http://127.0.0.1:') for e in endpoints), 'This fixture only supports a local unauthenticated model gateway'
    config = {k:source[k] for k in ['schema_version','models','model_profiles','runtime']}
    # Drop ACP profiles; this acceptance uses the configured native provider.
    config['model_profiles']['profiles'] = [p for p in config['model_profiles']['profiles'] if 'provider' in p.get('backend',{})]
    store = root/'caelis-store'; store.mkdir(mode=0o700)
    (store/'config.json').write_text(json.dumps(config))
    workspace = root/'caelis-workspace'; workspace.mkdir()
    plugin = root/'caelis-plugin'; shutil.copytree(package, plugin)
    # Host setup only. Keep distributed manifests unmodified; this fixture copy
    # supplies the exact launch scope using the ordinary native CUA flags.
    mcp = json.loads((plugin/'mcp.json').read_text())
    mcp['mcpServers']['desktop-world']['args'] = ['--capability-manifest',str(manifest),'--approve-capability-manifest']
    (plugin/'mcp.json').write_text(json.dumps(mcp))
    (plugin/'SHA256SUMS').unlink(missing_ok=True)
    environment = os.environ.copy()
    for name in ['CAELIS_CONTROL_URL','CAELIS_STORE_DIR','DTW_CUA_WORKER','DTW_STOP_FILE']:
        environment.pop(name, None)
    installed = subprocess.run([executable,'plugin','install',str(plugin),'--json','--embedded','--store-dir',str(store)],
        cwd=workspace, env=environment, capture_output=True, text=True, encoding='utf-8', timeout=60)
    (root/'caelis-install.json').write_text(installed.stdout)
    (root/'caelis-install.stderr').write_text(installed.stderr)
    assert installed.returncode == 0, installed.stderr[-2000:]
    prompt = f'''Use the installed Desktop World Skill and DTW to operate only the host-configured owned browser fixture. Enter DTW-Harness-verified in Message, click Apply, and verify the page displays that value. Download the Download fixture link once to {downloads}. Report the observed result. Use only DTW for browser actions and observations; do not use shell, HTTP, filesystem inspection of the fixture, or another browser tool. Read the installed Skill/reference files as needed. The host has already configured the authorized browser and destination; discover the target through DTW. Do not change any configuration. Stop and report a real blocker instead of fabricating success.'''
    (root/'caelis-prompt.txt').write_text(prompt)
    with (root/'caelis-transcript.jsonl').open('w') as output, (root/'caelis.stderr').open('w') as error:
        agent = subprocess.run([executable,'--embedded','--store-dir',str(store),'-format','jsonl','-p',prompt],
            cwd=workspace, env=environment, stdout=output, stderr=error, text=True, encoding='utf-8', timeout=360)
    assert agent.returncode == 0, (root/'caelis.stderr').read_text()[-3000:]
    calls = [json.loads(line).get('envelope',{}).get('update',{})
             for line in (root/'caelis-transcript.jsonl').read_text().splitlines()]
    calls = [c for c in calls if c.get('sessionUpdate') == 'tool_call']
    assert calls, 'No tool trace in real Harness output'
    assert all(c.get('name') in ['Skill','ToolSearch','Read','desktop_world__exec'] for c in calls), calls
    for c in calls:
        if c['name'] == 'Read':
            assert Path(c['rawInput']['path']).is_relative_to(plugin/'skills'), c
    operations = [c['rawInput'] for c in calls if c['name'] == 'desktop_world__exec']
    assert operations[0] == {'operation':'describe'}, operations
    assert not any(c.get('part') == 'structured' or 'script' in c for c in operations), operations
    assert sum(c.get('operation') == 'browser_download' for c in operations) == 1, operations
    return {'exit_code':agent.returncode, 'dtw_calls':len(operations),
        'operations':[c.get('operation') for c in operations], 'model_profile':config['model_profiles']['default_profile_id'],
        'store':str(store),'transcript':str(root/'caelis-transcript.jsonl')}
