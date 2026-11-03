"""Current Windows acceptance transport and read-only owned-process helpers.

No desktop API or legacy execute/result ledger protocol lives in this module.
AppClient owns the two-tool contract and response/image decoding.
"""
import json
import os
from pathlib import Path
import queue
import subprocess
import threading
import time


def require(value, message):
    if not value:
        raise AssertionError(message)


def read(path):
    try:
        if os.name=='nt':
            # CRT open() omits FILE_SHARE_DELETE and can make the independent
            # fixture's File.Replace fail. Observe an immutable publication
            # without blocking its next atomic replacement.
            import ctypes
            import msvcrt
            from ctypes import wintypes as w
            k=ctypes.WinDLL('kernel32',use_last_error=True)
            k.CreateFileW.argtypes=[w.LPCWSTR,w.DWORD,w.DWORD,w.LPVOID,w.DWORD,w.DWORD,w.HANDLE]
            k.CreateFileW.restype=w.HANDLE
            k.CloseHandle.argtypes=[w.HANDLE]
            handle=k.CreateFileW(str(path),0x80000000,7,None,3,0,None)
            if handle==w.HANDLE(-1).value:
                return None
            try:
                fd=msvcrt.open_osfhandle(handle,os.O_RDONLY|os.O_BINARY)
            except BaseException:
                k.CloseHandle(handle)
                raise
            with os.fdopen(fd,'rb') as stream:
                data=stream.read().decode('utf-8-sig')
        else:
            data=Path(path).read_text(encoding='utf-8-sig')
        return json.loads(data)
    except (OSError, ValueError):
        return None


class Mcp:
    def __init__(self, binary, output, options=(), *, env=None, process_factory=None):
        self.output = output
        output.mkdir()
        self.serial = 0
        self.messages = queue.Queue()
        self.pending = {}
        self.err = (output/'stderr.log').open('wb')
        self.proc = (process_factory or subprocess.Popen)(
            [str(binary), 'mcp', *options], stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=self.err, env=env)
        def receive():
            try:
                with (output/'stdout.jsonl').open('wb') as log:
                    for line in self.proc.stdout:
                        log.write(line)
                        log.flush()
                        self.messages.put(json.loads(line))
            except BaseException as error:
                self.messages.put(error)
            finally:
                self.messages.put(EOFError('MCP stdout closed'))
        self.reader = threading.Thread(target=receive, daemon=True)
        self.reader.start()
        try:
            self.rpc('initialize', {'protocolVersion':'2025-06-18', 'capabilities':{},
                                    'clientInfo':{'name':'dtw-windows-app-acceptance','version':'1'}})
            self.send({'jsonrpc':'2.0', 'method':'notifications/initialized'})
        except BaseException:
            self.close()
            raise

    def send(self, message):
        with (self.output/'requests.jsonl').open('a', encoding='utf-8') as log:
            log.write(json.dumps(message) + '\n')
        self.proc.stdin.write((json.dumps(message) + '\n').encode('utf-8'))
        self.proc.stdin.flush()

    def begin(self, method, params):
        self.serial += 1
        self.send({'jsonrpc':'2.0', 'id':self.serial, 'method':method, 'params':params})
        return self.serial

    def receive(self, key, timeout=40):
        if key in self.pending:
            return self.pending.pop(key)
        until = time.monotonic() + timeout
        while time.monotonic() < until:
            message = self.messages.get(timeout=max(.01, until-time.monotonic()))
            if isinstance(message, BaseException):
                raise message
            if message.get('id') == key:
                return message
            if 'id' in message:
                self.pending[message['id']] = message
        raise TimeoutError('MCP response')

    def rpc(self, method, params):
        response = self.receive(self.begin(method, params))
        require('error' not in response, response)
        return response['result']

    def close(self, expected_exit=0):
        if self.proc.poll() is None:
            self.proc.stdin.close()
        forced=False
        try:
            code = self.proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            self.proc.terminate()  # Popen retains this exact owned process handle.
            code = self.proc.wait(timeout=5)
            forced=True
        self.reader.join(timeout=2)
        self.err.close()
        require(not forced and not self.reader.is_alive() and code == expected_exit, f'MCP cleanup failed: exit={code}, expected={expected_exit}, forced={forced}')
        return {'pid':self.proc.pid, 'exit_code':code, 'forced':False}


class ProcessHandle:
    """Read-only supervision, plus explicit termination of a proven owned helper.

    Holding the kernel handle prevents a recycled PID from becoming our target.
    No application UI is observed or driven through this helper.
    """
    def __init__(self, pid, binary, birth=None, terminate=False):
        import ctypes
        from ctypes import wintypes as w
        self.ctypes, self.w = ctypes, w
        k = self.k = ctypes.WinDLL('kernel32', use_last_error=True)
        k.OpenProcess.argtypes = [w.DWORD,w.BOOL,w.DWORD]
        k.OpenProcess.restype = w.HANDLE
        k.GetProcessTimes.argtypes = [w.HANDLE,*([ctypes.POINTER(w.FILETIME)]*4)]
        k.QueryFullProcessImageNameW.argtypes = [w.HANDLE,w.DWORD,w.LPWSTR,ctypes.POINTER(w.DWORD)]
        k.WaitForSingleObject.argtypes = [w.HANDLE,w.DWORD]
        k.GetExitCodeProcess.argtypes = [w.HANDLE,ctypes.POINTER(w.DWORD)]
        k.TerminateProcess.argtypes = [w.HANDLE,w.UINT]
        k.CloseHandle.argtypes = [w.HANDLE]
        self.handle = k.OpenProcess(0x100000|0x1000|(1 if terminate else 0),False,pid)
        require(self.handle, 'Cannot bind owned process handle')
        self.pid, self.allow_terminate = pid, terminate
        created, exited, kernel, user = (w.FILETIME() for _ in range(4))
        require(k.GetProcessTimes(self.handle,*(ctypes.byref(v) for v in [created,exited,kernel,user])), 'Process birth unavailable')
        self.birth = created.dwHighDateTime*2**32+created.dwLowDateTime
        if birth is not None:
            require(self.birth == birth, 'Owned process birth changed before binding')
        path = ctypes.create_unicode_buffer(32768)
        length = w.DWORD(len(path))
        require(k.QueryFullProcessImageNameW(self.handle,0,path,ctypes.byref(length)) and path.value.casefold() == str(binary).casefold(),
                'Owned process executable path changed before binding')

    def wait(self, timeout=5):
        require(self.k.WaitForSingleObject(self.handle,int(timeout*1000)) == 0, 'Owned process did not exit in time')
        code = self.w.DWORD()
        require(self.k.GetExitCodeProcess(self.handle,self.ctypes.byref(code)), 'Original process exit unavailable')
        return code.value

    def terminate(self):
        require(self.allow_terminate and self.k.TerminateProcess(self.handle,1), 'Intentional owned-helper termination failed')

    def alive(self):
        return self.k.WaitForSingleObject(self.handle,0) == 258

    def close(self):
        if self.handle:
            self.k.CloseHandle(self.handle)
            self.handle = None
