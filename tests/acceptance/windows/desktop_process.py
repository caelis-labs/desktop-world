"""Test-only CreateProcessW stdio launcher with an actual lpDesktop.

Python subprocess.STARTUPINFO implements only a subset of STARTUPINFOW; setting
an arbitrary lpDesktop attribute there does not pass it to CreateProcessW. This
owned-resource probe uses the documented Win32 ABI and a strict stdio handle list.
It does not change the calling thread's desktop, ACLs, privileges or foreground.
"""
import ctypes
from ctypes import wintypes as w
import msvcrt
import os
import subprocess


class Startup(ctypes.Structure):
    _fields_ = [('cb', w.DWORD), ('reserved', w.LPWSTR), ('desktop', w.LPWSTR), ('title', w.LPWSTR),
                ('x', w.DWORD), ('y', w.DWORD), ('width', w.DWORD), ('height', w.DWORD),
                ('chars_x', w.DWORD), ('chars_y', w.DWORD), ('fill', w.DWORD),
                ('flags', w.DWORD), ('show', w.WORD), ('reserved_size', w.WORD),
                ('reserved_bytes', w.LPVOID), ('stdin', w.HANDLE), ('stdout', w.HANDLE), ('stderr', w.HANDLE)]


class StartupEx(ctypes.Structure):
    _fields_ = [('startup', Startup), ('attributes', w.LPVOID)]


class ProcessInfo(ctypes.Structure):
    _fields_ = [('process', w.HANDLE), ('thread', w.HANDLE), ('pid', w.DWORD), ('tid', w.DWORD)]


class DesktopProcess:
    def __init__(self, command, *, desktop, stdin, stdout, stderr, env=None):
        assert stdin == subprocess.PIPE and stdout == subprocess.PIPE
        self.k = k = ctypes.WinDLL('kernel32', use_last_error=True)
        k.GetCurrentProcess.restype = w.HANDLE
        k.CloseHandle.argtypes = [w.HANDLE]
        k.WaitForSingleObject.argtypes = [w.HANDLE, w.DWORD]
        k.GetExitCodeProcess.argtypes = [w.HANDLE, ctypes.POINTER(w.DWORD)]
        k.TerminateProcess.argtypes = [w.HANDLE, w.UINT]
        k.DuplicateHandle.argtypes = [w.HANDLE, w.HANDLE, w.HANDLE, ctypes.POINTER(w.HANDLE),
                                     w.DWORD, w.BOOL, w.DWORD]
        k.InitializeProcThreadAttributeList.argtypes = [w.LPVOID, w.DWORD, w.DWORD, ctypes.POINTER(ctypes.c_size_t)]
        k.UpdateProcThreadAttribute.argtypes = [w.LPVOID, w.DWORD, ctypes.c_size_t, w.LPVOID,
                                               ctypes.c_size_t, w.LPVOID, w.LPVOID]
        k.DeleteProcThreadAttributeList.argtypes = [w.LPVOID]
        k.CreateProcessW.argtypes = [w.LPCWSTR, w.LPWSTR, w.LPVOID, w.LPVOID, w.BOOL, w.DWORD,
                                    w.LPVOID, w.LPCWSTR, w.LPVOID, ctypes.POINTER(ProcessInfo)]
        k.CreateProcessW.restype = w.BOOL
        self.args, self.returncode, self.handle = command, None, None
        self.stdin = self.stdout = None
        fds, duplicates, initialized = [], [], False
        try:
            stdin_read, stdin_write = os.pipe()
            fds.extend([stdin_read, stdin_write])
            stdout_read, stdout_write = os.pipe()
            fds.extend([stdout_read, stdout_write])
            current = k.GetCurrentProcess()
            for fd in [stdin_read, stdout_write, stderr.fileno()]:
                handle = w.HANDLE()
                self.check(k.DuplicateHandle(current, msvcrt.get_osfhandle(fd), current,
                                             ctypes.byref(handle), 0, True, 2))
                duplicates.append(handle.value)
            size = ctypes.c_size_t()
            k.InitializeProcThreadAttributeList(None, 1, 0, ctypes.byref(size))
            attributes = ctypes.create_string_buffer(size.value)
            self.check(k.InitializeProcThreadAttributeList(attributes, 1, 0, ctypes.byref(size)))
            initialized = True
            handles = (w.HANDLE * len(duplicates))(*duplicates)
            self.check(k.UpdateProcThreadAttribute(attributes, 0, 0x20002,
                                                   handles, ctypes.sizeof(handles), None, None))
            startup = StartupEx()
            startup.startup.cb = ctypes.sizeof(startup)
            startup.startup.desktop = desktop
            startup.startup.flags = 0x100  # STARTF_USESTDHANDLES
            startup.startup.stdin, startup.startup.stdout, startup.startup.stderr = duplicates
            startup.attributes = ctypes.addressof(attributes)
            info = ProcessInfo()
            environment = None if env is None else ctypes.create_unicode_buffer(
                '\0'.join(key + '=' + value for key, value in sorted(env.items(), key=lambda item: item[0].casefold())) + '\0\0')
            # EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT |
            # CREATE_NO_WINDOW. This console MCP never opens a visible console.
            self.check(k.CreateProcessW(str(command[0]), ctypes.create_unicode_buffer(subprocess.list2cmdline(command)),
                                        None, None, True, 0x80000 | 0x400 | 0x8000000,
                                        environment, None, ctypes.byref(startup), ctypes.byref(info)))
            self.handle, self.pid, self.tid = info.process, info.pid, info.tid
            k.CloseHandle(info.thread)
            self.stdin = os.fdopen(stdin_write, 'wb', buffering=0)
            fds.remove(stdin_write)
            self.stdout = os.fdopen(stdout_read, 'rb', buffering=0)
            fds.remove(stdout_read)
        except BaseException:
            if self.handle:
                k.TerminateProcess(self.handle, 1)
                k.WaitForSingleObject(self.handle, 5000)
                k.CloseHandle(self.handle)
                self.handle = None
            for stream in [self.stdin, self.stdout]:
                if stream:
                    stream.close()
            raise
        finally:
            if initialized:
                k.DeleteProcThreadAttributeList(attributes)
            for handle in duplicates:
                k.CloseHandle(handle)
            for fd in fds:
                os.close(fd)

    @staticmethod
    def check(value):
        if not value:
            raise ctypes.WinError(ctypes.get_last_error())

    def poll(self):
        if self.handle and self.k.WaitForSingleObject(self.handle, 0) == 0:
            return self.wait(timeout=0)
        return self.returncode

    def wait(self, timeout=None):
        if self.handle:
            status = self.k.WaitForSingleObject(self.handle, 0xffffffff if timeout is None else int(timeout * 1000))
            if status == 258:
                raise subprocess.TimeoutExpired(self.args, timeout)
            self.check(status == 0)
            code = w.DWORD()
            self.check(self.k.GetExitCodeProcess(self.handle, ctypes.byref(code)))
            self.returncode = code.value
            self.k.CloseHandle(self.handle)
            self.handle = None
        return self.returncode

    def terminate(self):
        if self.poll() is None:
            self.check(self.k.TerminateProcess(self.handle, 1))

    kill = terminate
