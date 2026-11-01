"""Opt-in read-only Windows queue observations, without attachment or input."""
import ctypes
import json
import threading
import time


class QueueObserver:
    def __init__(self, owned_title, output):
        self.owned_title, self.output = owned_title, output
        self.stop_event = threading.Event()
        self.thread = threading.Thread(target=self.run, daemon=True)
        from ctypes import wintypes as w
        self.w = w
        self.user = ctypes.WinDLL('user32', use_last_error=True)
        self.callback_type = ctypes.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)

        class GUIInfo(ctypes.Structure):
            _fields_ = [('cbSize', w.DWORD), ('flags', w.DWORD), ('active', w.HWND),
                        ('focus', w.HWND), ('capture', w.HWND), ('menu_owner', w.HWND),
                        ('move_size', w.HWND), ('caret', w.HWND), ('caret_rect', w.RECT)]
        self.GUIInfo = GUIInfo
        signatures = {
            'EnumWindows': ([self.callback_type, w.LPARAM], w.BOOL),
            'GetWindowTextW': ([w.HWND, w.LPWSTR, ctypes.c_int], ctypes.c_int),
            'GetWindowThreadProcessId': ([w.HWND, ctypes.POINTER(w.DWORD)], w.DWORD),
            'GetForegroundWindow': ([], w.HWND),
            'GetAsyncKeyState': ([ctypes.c_int], ctypes.c_short),
            'GetGUIThreadInfo': ([w.DWORD, ctypes.POINTER(GUIInfo)], w.BOOL),
            'GetAncestor': ([w.HWND, w.UINT], w.HWND),
        }
        for name, (arguments, result) in signatures.items():
            function = getattr(self.user, name)
            function.argtypes, function.restype = arguments, result

    def identity(self, hwnd):
        process = self.w.DWORD()
        thread = self.user.GetWindowThreadProcessId(hwnd, ctypes.byref(process))
        return {'hwnd': hwnd, 'thread': thread, 'pid': process.value}

    def gui(self, thread):
        result = self.GUIInfo()
        result.cbSize = ctypes.sizeof(result)
        ctypes.set_last_error(0)
        if not self.user.GetGUIThreadInfo(thread, ctypes.byref(result)):
            return {'thread': thread, 'error': ctypes.get_last_error()}
        return {'thread': thread, 'flags': result.flags, 'active': result.active,
                'focus': self.identity(result.focus), 'focus_root': self.user.GetAncestor(result.focus, 2),
                'capture': result.capture, 'menu_owner': result.menu_owner}

    def sample(self):
        matches = []

        @self.callback_type
        def collect(hwnd, _):
            text = ctypes.create_unicode_buffer(1024)
            self.user.GetWindowTextW(hwnd, text, len(text))
            if self.owned_title in text.value:
                row = self.identity(hwnd)
                row['title'] = text.value
                row['top_gui'] = self.gui(row['thread'])
                focus_thread = row['top_gui'].get('focus', {}).get('thread')
                if focus_thread and focus_thread != row['thread']:
                    row['focus_gui'] = self.gui(focus_thread)
                matches.append(row)
            return True

        ctypes.set_last_error(0)
        raw_return = self.user.EnumWindows(collect, 0)
        error = ctypes.get_last_error()
        return {'unix_ns': time.time_ns(), 'monotonic_ns': time.monotonic_ns(),
                'foreground': self.user.GetForegroundWindow(),
                'async_marker': self.user.GetAsyncKeyState(0xE8),
                'enum_return': raw_return, 'enum_error': error, 'owned_windows': matches}

    def start(self):
        self.thread.start()

    def run(self):
        with self.output.open('w', encoding='utf-8') as log:
            while not self.stop_event.is_set():
                try:
                    row = self.sample()
                except BaseException as error:
                    row = {'unix_ns': time.time_ns(), 'observer_error': repr(error)}
                log.write(json.dumps(row) + '\n')
                log.flush()
                self.stop_event.wait(.01)

    def close(self):
        self.stop_event.set()
        self.thread.join(timeout=3)
        if self.thread.is_alive():
            raise RuntimeError('Read-only queue observer did not exit')
