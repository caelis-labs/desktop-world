#!/usr/bin/env python3
"""Manual GUI POC session. Preserve original receipts and image bytes per call."""
import base64
import json
import os
from pathlib import Path
import socket
import subprocess
import sys

from probe import call, parse_exec_result, result, structured_result


def serve(root, binary, worker):
    root.mkdir(parents=True, exist_ok=True)
    proc = subprocess.Popen([binary, '--worker', worker], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=(root/'stderr.log').open('w'),
                            text=True)
    call(proc, {'jsonrpc':'2.0','id':1,'method':'initialize','params':{
        'protocolVersion':'2025-06-18','capabilities':{},
        'clientInfo':{'name':'associated-scene-poc','version':'1'}}})
    server = socket.socket(socket.AF_UNIX)
    server.bind(str(root/'socket'))
    os.chmod(root/'socket', 0o600)
    server.listen(1)
    number = 0
    try:
        while True:
            connection, _ = server.accept()
            with connection:
                request = json.loads(connection.makefile().readline())
                if request.get('stop'):
                    connection.sendall(b'{"stopped":true}\n')
                    break
                number += 1
                ident = number*100
                args = {'wait_ms':1000, **request}
                folder = root/f'{number:03d}'
                folder.mkdir()
                (folder/'request.json').write_text(json.dumps(args,ensure_ascii=False,indent=2))
                try:
                    response = call(proc, {'jsonrpc':'2.0','id':ident,'method':'tools/call',
                        'params':{'name':'exec','arguments':args}}, timeout=150)
                    done = parse_exec_result(response['result'])
                    if done.get('status') == 'running':
                        done = result(proc,ident+1,'scene-poc',done['id'])
                    (folder/'receipt.json').write_text(json.dumps(done,ensure_ascii=False,indent=2))
                    if 'id' in done:
                        native = structured_result(proc,ident+2,'scene-poc',done['id'])
                        (folder/'native.json').write_text(json.dumps(native,ensure_ascii=False,indent=2))
                        # DTW pages images individually. Fetch all retained frames;
                        # otherwise a 3-window scene can look like a single screenshot.
                        for index in range(17):
                            images = call(proc, {'jsonrpc':'2.0','id':ident+3+index,'method':'tools/call',
                                'params':{'name':'exec','arguments':{'operation':'result','id':done['id'],'part':'image','image_index':index}}})
                            pixels = [item for item in images['result'].get('content',[]) if item['type']=='image']
                            if not pixels:
                                break
                            for item in pixels:
                                (folder/f'image-{index}.png').write_bytes(base64.b64decode(item['data']))
                    connection.sendall((json.dumps({'folder':str(folder),'receipt':done},ensure_ascii=False)+'\n').encode())
                except Exception as error:
                    connection.sendall((json.dumps({'folder':str(folder),'error':str(error)})+'\n').encode())
    finally:
        proc.stdin.close()
        try:
            proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
        server.close()
        (root/'socket').unlink(missing_ok=True)


if __name__ == '__main__':
    if sys.argv[1] == 'serve':
        serve(Path(sys.argv[2]), sys.argv[3], sys.argv[4])
    else:
        client = socket.socket(socket.AF_UNIX)
        client.connect(str(Path(sys.argv[2])/'socket'))
        client.sendall((sys.stdin.read().strip()+'\n').encode())
        print(client.makefile().readline())
