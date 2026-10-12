#!/usr/bin/env python3
"""Read-only live check that an opted-in screenshot returns in one exec reply."""

import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time
import uuid

from accept_chrome import exact_chrome_pid
from probe import call, parse_exec_result


def main():
    package = Path(sys.argv[1]).resolve()
    root = Path('/private/tmp') / f'dtw-cua-inline-image-{uuid.uuid4().hex[:8]}'
    root.mkdir(mode=0o700)
    html = root / 'visual_large.html'
    shutil.copy2(Path(__file__).parent / 'fixtures/visual_large.html', html)
    profile = root / 'chrome-profile'
    opened = subprocess.run(['open', '-g', '-na', '/Applications/Google Chrome.app', '--args',
                             f'--user-data-dir={profile}', '--no-first-run',
                             '--no-default-browser-check', '--disable-background-mode',
                             '--force-renderer-accessibility', '--disable-translate',
                             f'--app={html.as_uri()}'], capture_output=True, text=True)
    if opened.returncode:
        raise RuntimeError(opened.stderr)
    pid = None
    try:
        for _ in range(100):
            try:
                pid = exact_chrome_pid(profile)
                break
            except (subprocess.CalledProcessError, RuntimeError):
                time.sleep(0.1)
        if pid is None:
            raise RuntimeError('isolated Chrome PID not found')
        proc = subprocess.Popen([str(package / 'bin/dtw')], stdin=subprocess.PIPE,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                env={'PATH': '/usr/bin:/bin', 'TMPDIR': '/private/tmp'})
        try:
            call(proc, {'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {
                'protocolVersion': '2025-06-18', 'capabilities': {},
                'clientInfo': {'name': 'inline-image-probe', 'version': '1'}}})
            script = (f'const a=await cua.getApp({{pid:{pid},title:"DTW Visual Acceptance"}});'
                      'const s=await a.getState({include_accessibility_tree:false,'
                      'include_screenshot:true,max_image_dimension:1000});'
                      'return {window_id:a.windowId,capture_id:s.capture_id,'
                      'width:s.screenshot_width,height:s.screenshot_height};')
            reply = call(proc, {'jsonrpc': '2.0', 'id': 2, 'method': 'tools/call', 'params': {
                'name': 'exec', 'arguments': {'session': 'inline-image', 'script': script,
                                             'include_image': True, 'wait_ms': 5000}}})
            contents = reply['result']['content']
            text = parse_exec_result(reply['result'])
            images = [part for part in contents if part['type'] == 'image']
            report = {'status': text.get('status'), 'id': text.get('id'),
                      'inline_images': len(images),
                      'mime_type': images[0]['mimeType'] if images else None,
                      'value': text.get('value')}
            assert report['status'] == 'completed' and report['inline_images'] == 1, report
            assert report['value']['capture_id'] and report['value']['window_id'] > 0, report
            (root / 'record.json').write_text(json.dumps(report, indent=2))
            print(json.dumps({'record': str(root / 'record.json'), **report}))
        finally:
            proc.stdin.close()
            try:
                proc.wait(timeout=8)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
    finally:
        if pid is not None:
            try:
                if exact_chrome_pid(profile) == pid:
                    os.kill(pid, signal.SIGTERM)
            except (subprocess.CalledProcessError, RuntimeError, ProcessLookupError):
                pass
            for _ in range(50):
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    if profile.exists():
                        shutil.rmtree(profile)
                    break
                time.sleep(0.1)


if __name__ == '__main__':
    main()
