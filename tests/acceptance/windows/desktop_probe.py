#!/usr/bin/env python3
"""Opt-in MCP inventory on an owned invisible Windows desktop.

Creates a uniquely named desktop with the current user's ordinary ACL, without
SwitchDesktop, SetThreadDesktop, foreground changes or input. Only this MCP
process is launched there, using real STARTUPINFOW.lpDesktop. Native BOOL/error/
count and actual worker context are retained independently of the public result.
The named parent's desktop and the child's default desktop are measured apart;
Windows does not automatically make a STARTUPINFO-opened desktop inheritable.
Run serially with no other process using the same output, with python -X utf8.
"""
import argparse
import ctypes
from ctypes import wintypes as w
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import queue
import threading
import time
import traceback
import uuid

from cursor_probe import Mcp, ProcessHandle, require, wait, write
from desktop_process import DesktopProcess


class Desktop:
    def __init__(self):
        self.user = ctypes.WinDLL('user32', use_last_error=True)
        self.kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        u, k = self.user, self.kernel
        u.GetProcessWindowStation.restype = w.HANDLE
        u.GetThreadDesktop.argtypes = [w.DWORD]
        u.GetThreadDesktop.restype = w.HANDLE
        u.GetUserObjectInformationW.argtypes = [w.HANDLE, ctypes.c_int, w.LPVOID, w.DWORD, ctypes.POINTER(w.DWORD)]
        u.CreateDesktopW.argtypes = [w.LPCWSTR, w.LPCWSTR, w.LPVOID, w.DWORD, w.DWORD, w.LPVOID]
        u.CreateDesktopW.restype = w.HANDLE
        u.CloseDesktop.argtypes = [w.HANDLE]
        u.CloseDesktop.restype = w.BOOL
        self.window_callback = ctypes.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
        u.EnumWindows.argtypes = [self.window_callback, w.LPARAM]
        u.EnumWindows.restype = w.BOOL
        u.EnumDesktopWindows.argtypes = [w.HANDLE, self.window_callback, w.LPARAM]
        u.EnumDesktopWindows.restype = w.BOOL
        self.desktop_callback = ctypes.WINFUNCTYPE(w.BOOL, w.LPWSTR, w.LPARAM)
        u.EnumDesktopsW.argtypes = [w.HANDLE, self.desktop_callback, w.LPARAM]
        u.EnumDesktopsW.restype = w.BOOL
        self.station = u.GetProcessWindowStation()
        require(self.station, 'Cannot read current process window station')
        self.station_name = self.name(self.station)
        self.desktop_name = 'DTW.Acceptance.' + uuid.uuid4().hex
        # READOBJECTS | CREATEWINDOW | ENUMERATE | WRITEOBJECTS. No switch,
        # hook/journal rights, privilege changes, ACL changes or secure desktop.
        self.handle = u.CreateDesktopW(self.desktop_name, None, None, 0, 0xc3, None)
        require(self.handle, f'CreateDesktopW failed: {ctypes.get_last_error()}')

    def name(self, handle):
        value = ctypes.create_unicode_buffer(512)
        require(self.user.GetUserObjectInformationW(handle, 2, value, ctypes.sizeof(value), None),
                f'Cannot read object name: {ctypes.get_last_error()}')
        return value.value

    def thread_name(self, tid):
        # Foreign thread desktop handles may be unassigned or unreadable from
        # this observer. The native worker's own trace is authoritative for it.
        try:
            handle = self.user.GetThreadDesktop(tid)
            return self.name(handle) if handle else None
        except AssertionError as error:
            return {'unavailable': str(error)}

    def windows(self, owned=False):
        rows = []
        @self.window_callback
        def callback(hwnd, _):
            rows.append(int(hwnd))
            return True
        ctypes.set_last_error(0)
        raw = self.user.EnumDesktopWindows(self.handle, callback, 0) if owned else self.user.EnumWindows(callback, 0)
        return {'raw_return': int(raw), 'last_error': ctypes.get_last_error(), 'windows': rows, 'callbacks': len(rows)}

    def exists(self):
        names = []
        @self.desktop_callback
        def callback(name, _):
            names.append(name)
            return True
        require(self.user.EnumDesktopsW(self.station, callback, 0), 'Cannot verify owned desktop cleanup')
        return self.desktop_name in names

    def close(self):
        if self.handle:
            require(self.user.CloseDesktop(self.handle), f'CloseDesktop failed: {ctypes.get_last_error()}')
            self.handle = None


def children(parent):
    k = ctypes.WinDLL('kernel32', use_last_error=True)
    class Entry(ctypes.Structure):
        _fields_ = [('size', w.DWORD), ('usage', w.DWORD), ('pid', w.DWORD),
                    ('heap', ctypes.c_size_t), ('module', w.DWORD), ('threads', w.DWORD),
                    ('parent', w.DWORD), ('priority', w.LONG), ('flags', w.DWORD), ('exe', w.WCHAR * 260)]
    k.CreateToolhelp32Snapshot.argtypes = [w.DWORD, w.DWORD]
    k.CreateToolhelp32Snapshot.restype = w.HANDLE
    k.Process32FirstW.argtypes = k.Process32NextW.argtypes = [w.HANDLE, ctypes.POINTER(Entry)]
    k.CloseHandle.argtypes = [w.HANDLE]
    handle = k.CreateToolhelp32Snapshot(2, 0)
    require(handle != w.HANDLE(-1).value, 'Cannot snapshot owned worker identities')
    rows = []
    try:
        row = Entry(size=ctypes.sizeof(Entry))
        more = k.Process32FirstW(handle, ctypes.byref(row))
        while more:
            if row.parent == parent:
                rows.append({'pid': row.pid, 'parent': row.parent, 'exe': row.exe})
            more = k.Process32NextW(handle, ctypes.byref(row))
    finally:
        k.CloseHandle(handle)
    return rows


def traces(path):
    prefix = 'DTW Windows enumeration: '
    rows = []
    for line in path.read_text(encoding='utf-8', errors='replace').splitlines():
        if line.startswith(prefix):
            rows.append(json.loads(line[len(prefix):]))
    return rows


def native_catalog(binary, desktop, out, env):
    """Read-only native worker on the actual empty desktop, not a simulated BOOL."""
    identity = ProcessHandle(os.getpid(), Path(sys.executable).resolve())
    try:
        birth = identity.birth
    finally:
        identity.close()
    result = {'passed': False}
    proc = None
    with (out / 'native.stderr.log').open('wb') as stderr:
        try:
            proc = DesktopProcess([str(binary), '--dtw-native-worker', str(os.getpid()), str(birth)],
                                  desktop=desktop.station_name + '\\' + desktop.desktop_name,
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr, env=env)
            result['pid'], result['initial_thread_id'] = proc.pid, proc.tid
            request = {'request_id': 'empty-desktop-catalog', 'operation': 'catalog',
                       'deadline_unix_ms': int(time.time() * 1000) + 20000}
            write(out / 'native-request.json', request)
            proc.stdin.write((json.dumps(request) + '\n').encode('utf-8'))
            proc.stdin.flush()
            replies = queue.Queue()
            def receive():
                try:
                    replies.put(proc.stdout.readline())
                except BaseException as error:
                    replies.put(error)
            reader = threading.Thread(target=receive, daemon=True)
            reader.start()
            reply = replies.get(timeout=25)
            if isinstance(reply, BaseException):
                raise reply
            (out / 'native-reply.jsonl').write_bytes(reply)
            payload = json.loads(reply)
            require(payload['request_id'] == request['request_id'], 'Native reply identity mismatch')
            raw = wait(lambda: traces(out / 'native.stderr.log'))
            result['enumerations'] = raw
            require(all(row['context']['thread_desktop'] == desktop.desktop_name for row in raw), 'Native read moved desktop')
            result['observed_thread_desktop'] = desktop.thread_name(proc.tid)
            value = payload['result']
            if isinstance(value.get('apps'), list):
                require(value['apps'] == [] and all(row['raw_return'] for row in raw), 'Failed/foreign enumeration disguised as empty success')
                result['outcome'] = 'successful_empty_current_desktop_inventory'
            else:
                require(value.get('error') in ['desktop_enumeration_unproven', 'desktop_enumeration_failed'], value)
                require(desktop.desktop_name in value.get('message', ''), 'Failure omitted actual worker desktop')
                require('操作成功完成' not in value.get('message', '') and
                        'operation completed successfully' not in value.get('message', '').casefold(), 'Misleading success-string failure')
                result['outcome'] = 'explicit_unproven_current_desktop_diagnostic'
            result['passed'] = True
        except BaseException:
            result['failure'] = traceback.format_exc()
        finally:
            if proc:
                proc.stdin.close()
                try:
                    result['exit_code'] = proc.wait(timeout=8)
                    require(result['exit_code'] == 0, 'Native worker shutdown failed')
                except BaseException:
                    result['passed'] = False
                    result['cleanup_error'] = traceback.format_exc()
                    proc.terminate()
                    proc.wait(timeout=5)
                proc.stdout.close()
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    require(os.name == 'nt' and sys.flags.utf8_mode, 'Use Windows: python -X utf8')
    binary, out = args.dtw.resolve(strict=True), args.output.resolve()
    require(not out.exists(), 'Use a new evidence directory')
    out.mkdir(parents=True)
    result = {'passed': False, 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'desktop_switch': False, 'input': False, 'acl_changes': False, 'cleanup': []}
    desktop = client = None
    handles = []
    try:
        desktop = Desktop()
        result['desktop'] = desktop.station_name + '\\' + desktop.desktop_name
        host, isolated = desktop.windows(), desktop.windows(owned=True)
        result['host_before'], result['isolated_before'] = host, isolated
        require(not isolated['windows'], 'Fresh owned desktop unexpectedly contains a window')
        env = dict(os.environ, DTW_AX_READINESS_TRACE='1')
        result['native_empty_desktop'] = native_catalog(binary, desktop, out, env)
        require(result['native_empty_desktop']['passed'], 'Owned empty-desktop native catalog failed its contract')
        # Bind before initializing so cleanup retains the Popen handle even if
        # startup/initialize fails. The transport never sends app input.
        client = Mcp.__new__(Mcp)
        client.__init__(binary, out / 'mcp', env=env,
                        process_factory=lambda command, **kwargs: DesktopProcess(command, desktop=result['desktop'], **kwargs))
        result['mcp_pid'] = client.proc.pid
        # A non-GUI MCP thread may not yet have connected to any desktop. Do not
        # mistake an unassigned thread for evidence of failed lpDesktop setup.
        result['mcp_initial_thread_desktop'] = desktop.thread_name(client.proc.tid)
        result['mcp_startup_desktop'] = result['desktop']
        listing = client.rpc('tools/list', {})
        require(any(tool['name'] == 'apps' for tool in listing['tools']), 'apps tool missing')
        write(out / 'tools.json', listing)
        response = client.rpc('tools/call', {'name': 'apps', 'arguments': {}})
        write(out / 'apps-raw.json', response)
        native = wait(lambda: traces(out / 'mcp/stderr.log'))
        result['native_enumerations'] = native
        for row in native:
            context = row['context']
            require(context['window_station'] == desktop.station_name, 'Worker changed window station')
            require(context['pid'] != client.proc.pid, 'Trace did not identify the native worker')
        result['worker_processes'] = children(client.proc.pid)
        for child in result['worker_processes']:
            if child['exe'].casefold() != binary.name.casefold():
                # Windows may own a conhost for this console process. Never
                # bind it as a DTW worker or apply an executable-path exception.
                continue
            handle = ProcessHandle(child['pid'], binary, terminate=True)
            handles.append(handle)
            child['birth'] = handle.birth
        texts = [part['text'] for part in response.get('content', []) if part.get('type') == 'text']
        payload = response.get('structuredContent') or (json.loads(texts[0]) if len(texts) == 1 else None)
        require(isinstance(payload, dict), 'No structured apps result')
        worker_desktops = {row['context']['thread_desktop'] for row in native}
        if worker_desktops == {'Default'}:
            # No inherited desktop handle or child lpDesktop: the documented OS
            # default-desktop connection is distinct from the named MCP parent.
            require(isinstance(payload.get('apps'), list), 'Default-desktop MCP inventory failed')
            result['outcome'] = 'named_mcp_parent_with_default_desktop_native_worker'
            result['mcp_app_count'] = len(payload['apps'])
        elif isinstance(payload.get('apps'), list):
            require(worker_desktops == {desktop.desktop_name}, 'Unexpected worker desktop')
            require(payload['apps'] == [], 'Owned-desktop inventory leaked host applications')
            require(all(row['raw_return'] != 0 for row in native), 'Failed enumeration was disguised as complete')
            result['outcome'] = 'successful_empty_current_desktop_inventory'
        else:
            require(worker_desktops == {desktop.desktop_name}, 'Unexpected worker desktop')
            encoded = json.dumps(payload, ensure_ascii=False)
            require('desktop_enumeration_unproven' in encoded or 'desktop_enumeration_failed' in encoded,
                    'Failure did not carry actionable desktop enumeration diagnosis')
            require(desktop.desktop_name in encoded, 'Failure omitted actual worker desktop')
            require('操作成功完成' not in encoded and 'operation completed successfully' not in encoded.casefold(),
                    'Failure incorrectly says the operation succeeded')
            result['outcome'] = 'explicit_unproven_current_desktop_diagnostic'
        after = desktop.windows(owned=True)
        result['isolated_after'] = after
        require(not set(host['windows']).intersection(after['windows']), 'Owned desktop exposed host windows')
        result['passed'] = True
    except BaseException:
        result['failure'] = traceback.format_exc()
    finally:
        if client and hasattr(client, 'proc'):
            try:
                result['cleanup'].append({'mcp': client.close()})
            except BaseException:
                result['passed'] = False
                result['cleanup'].append({'mcp_error': traceback.format_exc()})
        for handle in handles:
            try:
                code = handle.wait(timeout=8)
                result['cleanup'].append({'worker_pid': handle.pid, 'exit_code': code})
                require(code == 0, 'Owned native worker shutdown was unclean')
            except BaseException:
                result['passed'] = False
                result['cleanup'].append({'worker_error': traceback.format_exc()})
                if handle.alive():
                    handle.terminate()
                    handle.wait()
            finally:
                handle.close()
        if desktop:
            try:
                desktop.close()
                wait(lambda: not desktop.exists())
                result['cleanup'].append({'owned_desktop_removed': True})
            except BaseException:
                result['passed'] = False
                result['cleanup'].append({'desktop_error': traceback.format_exc()})
        write(out / 'summary.json', result)
        print(json.dumps(result))
    return 0 if result['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
