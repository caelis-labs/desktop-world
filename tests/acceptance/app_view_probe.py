#!/usr/bin/env python3
"""Read-only macOS extension contract gate against an explicit running test PID.

Usage: DTW_CUA_WORKER=/path/to/official-worker python3 app_view_probe.py DTW PID
GUI effects still require the manual cases in poc/associated-capture/acceptance.md.
"""
import base64
import json
import struct
import subprocess
import sys
import tempfile
import time

from probe import call, exec_call, structured_result


def main():
    pid = int(sys.argv[2])
    stderr = tempfile.TemporaryFile(mode="w+")
    proc = subprocess.Popen([sys.argv[1]], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=stderr, text=True)
    try:
        call(proc, {'jsonrpc':'2.0','id':1,'method':'initialize','params':{
            'protocolVersion':'2025-06-18','capabilities':{},
            'clientInfo':{'name':'app-extension-contract','version':'1'}}})
        started = exec_call(proc, 2, {'operation':'app','pid':pid})
        done = started
        deadline = time.monotonic() + 60
        while done['status'] == 'running' and time.monotonic() < deadline:
            time.sleep(0.2)
            done = exec_call(proc,3,{'operation':'result','id':started['id']})
        assert done['status']=='completed' and not done.get('is_error'), done
        native = structured_result(proc,4,'app-probe',done['id'])
        assert native['scope']=='app' and native['observation_id'], native
        assert len(native['windows']) >= len(native['frames'])
        assert native['association_complete'] is False
        frames = native['frames']
        assert 1 <= len(frames) <= 2
        assert [f['stack_rank'] for f in frames] == sorted(f['stack_rank'] for f in frames)
        for index, frame in enumerate(frames):
            assert frame['image_index']==index
            if frame['backend']=='cua':
                assert frame['native']['capture_id']
            assert frame['width']>0 and frame['height']>0
            response = call(proc, {'jsonrpc':'2.0','id':5+index,'method':'tools/call',
                'params':{'name':'exec','arguments':{'operation':'result',
                    'id':done['id'],'part':'image','image_index':index}}})
            images = [c for c in response['result']['content'] if c['type']=='image']
            assert len(images)==1
            png = base64.b64decode(images[0]['data'])
            assert png[:8] == b'\x89PNG\r\n\x1a\n'
            assert struct.unpack('>II',png[16:24]) == (frame['width'],frame['height'])
        print(json.dumps({'pid':pid,'frames':[{k:f[k] for k in ('pid','window_id','layer','association')} for f in frames], 'contract':'passed'}))
    finally:
        proc.stdin.close()
        try:
            proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
        stderr.seek(0)
        error_log = stderr.read()
        stderr.close()
        if proc.returncode != 0 or sys.exc_info()[0] is not None:
            print(error_log[-4000:], file=sys.stderr)
        if proc.returncode != 0 and sys.exc_info()[0] is None:
            raise RuntimeError(f"DTW exited {proc.returncode}")


if __name__ == '__main__':
    main()
