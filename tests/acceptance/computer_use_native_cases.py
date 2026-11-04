"""Extended native cases for a CoreCases-compatible harness.

Each public method starts from a fresh, running fixture. All application input
uses apps/exec. Disk access is independent evidence, except N12's owned seed.
Returned receipts retain uncertain dispatch; no action is automatically replayed.
"""
import ctypes
import errno
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time

from app_client import require
from standard_agent_probe import wait_until


def _digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


class _BSDInfo(ctypes.Structure):
    # macOS sys/proc_info.h: proc_bsdinfo (PROC_PIDTBSDINFO = 3).
    _fields_ = [(name, ctypes.c_uint32) for name in (
        'flags', 'status', 'xstatus', 'pid', 'ppid', 'uid', 'gid', 'ruid',
        'rgid', 'svuid', 'svgid', 'reserved')] + [
        ('comm', ctypes.c_char * 16), ('name', ctypes.c_char * 32)] + [
        (name, ctypes.c_uint32) for name in ('nfiles', 'pgid', 'pjobc', 'tdev', 'tpgid', 'nice')] + [
        ('start_seconds', ctypes.c_uint64), ('start_microseconds', ctypes.c_uint64)]


def _process_identity(pid):
    library = ctypes.CDLL('/usr/lib/libproc.dylib', use_errno=True)
    library.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_int]
    library.proc_pidinfo.restype = ctypes.c_int
    info = _BSDInfo()
    ctypes.set_errno(0)
    size = library.proc_pidinfo(pid, 3, 0, ctypes.byref(info), ctypes.sizeof(info))
    if size == 0 and ctypes.get_errno() == errno.ESRCH:
        return None
    require(size == ctypes.sizeof(info) and info.pid == pid and info.start_microseconds < 1_000_000,
            {'pid': pid, 'identity_bytes': size, 'errno': ctypes.get_errno()})
    return {'birth': info.start_seconds * 1_000_000 + info.start_microseconds, 'zombie': info.status == 5}


class ProcessObserver:
    """Exact PID + kernel birth observer for the exec-launched restarted process.

    poll() returns 'exit_observed' on positive exit evidence; returncode remains
    None because this process is not our Popen child. Cleanup may consume
    cleanup_evidence(), but must not treat an unknown exit status as zero.
    """
    def __init__(self, pid, birth, client, app):
        self.pid, self.birth, self.client, self.app = pid, birth, client, app
        self.returncode = None
        self.exit_observed = False
        self.exit_reason = None
        require(type(birth) is int and birth > 0, 'Kernel process birth is required')
        require(_process_identity(pid) == {'birth': birth, 'zombie': False}, 'Restarted process identity changed')

    def poll(self):
        if not self.exit_observed:
            current = _process_identity(self.pid)
            if current is None or current['birth'] != self.birth or current['zombie']:
                self.exit_observed = True
                self.exit_reason = 'absent' if current is None else 'birth_changed' if current['birth'] != self.birth else 'zombie'
        return 'exit_observed' if self.exit_observed else None

    def wait(self, timeout=None):
        deadline = time.monotonic() + (timeout if timeout is not None else 15)
        while self.poll() is None:
            if time.monotonic() >= deadline:
                raise subprocess.TimeoutExpired(str(self.app), timeout)
            time.sleep(0.02)
        return None

    def cleanup_evidence(self):
        self.poll()
        return {'started': True, 'pid': self.pid, 'birth': self.birth, 'returncode': None,
                'exit_observed': self.exit_observed, 'exit_reason': self.exit_reason,
                'expected_detached_exit': True}

    def _signal(self, number):
        # Recheck the exact kernel birth before each forced cleanup operation.
        # An already exited original process never authorizes a reused PID.
        if self.poll() is None:
            try:
                os.kill(self.pid, number)
            except ProcessLookupError:
                self.poll()

    def terminate(self):
        self._signal(signal.SIGTERM)

    def kill(self):
        self._signal(signal.SIGKILL)


class NativeExtendedCases:
    """Mixin requiring CoreCases.run/capture/anchor/type_at/native/chat/state/events.

    fixture/client/output and app are the CoreCases instance attributes. A case
    raises on failed evidence and returns a detail dict on success. A returned
    ``unavailable`` means unsupported, never pass. Cursor/focus audits belong to
    the caller and must surround every method, including setup and cleanup.
    """
    def _native_mode(self):
        self.run("await app.click('Native controls');")
        wait_until(lambda: self.state()['mode'] == 'native')

    def _chat_mode(self):
        self.run("await app.click('Canvas chat');")
        wait_until(lambda: self.state()['mode'] == 'chat')

    def _native_attempt(self, source):
        data, response = self.client.execute(self.app, source, allow_error=True)
        if response.get('isError'):
            require(data.get('effects', {}).get('dispatch', {}).get('unknown') == 1, data)
        return {'failure': data.get('failure'), 'dispatch': data.get('effects', {}).get('dispatch'),
                'tool_error': response.get('isError') is True}

    def _named_button(self, name):
        observed = self.run('const v=await app.look({role:"button",text:' + json.dumps(name)
                            + '}); return {elements:v.elements};')
        matches = [row for row in observed['result']['elements']
                   if row.get('role') == 'button' and row.get('name') == name]
        require(len(matches) == 1, observed)
        return self._native_attempt('await app.click({target:' + json.dumps(matches[0]['id']) + '});')

    def _open_save(self):
        receipt = self._native_attempt("await app.click('Save file…');")
        wait_until(lambda: self.native()['panel']['visible'])
        require(self.native()['panel']['kind'] == 'save', self.native())
        return receipt

    def N11(self):
        """Commit Unicode through GUI, save once via NSSavePanel, compare bytes/journal."""
        self._native_mode()
        path = self.fixture.output / 'owned-note.txt'
        require(not path.exists(), 'N11 requires a fresh fixture without owned-note.txt')
        value = 'Native extended save — 中文 café العربية 🙂'
        before = self.native()
        journal = len(self.events('save-file'))
        self.type_at('native-note', value, 'n11-note')
        self.run("await app.click('Commit note');")
        require(self.native()['saved_note'] == value and self.native()['saves'] == before['saves'] + 1,
                self.native())
        opening = self._open_save()
        self.capture('n11-save-panel')
        saving = self._named_button('Save')
        after = wait_until(lambda: p if (p := self.native()['panel'])['result'] == 'ok' else None)
        require(path.read_bytes() == value.encode('utf-8'), 'saved bytes differ')
        require(after['file_saves'] == before['panel']['file_saves'] + 1 and not after['error'], after)
        rows = self.events('save-file')[journal:]
        require(len(rows) == 1 and rows[0]['result'] == 'ok', rows)
        require(self.native()['counter'] == before['counter'], self.native())
        return {'file': path.name, 'sha256': _digest(path), 'file_saves_delta': 1,
                'opening_receipt': opening, 'saving_receipt': saving}

    def N12(self):
        """Seed only the owned destination; cancel real overwrite then underlying panel."""
        self._native_mode()
        path = self.fixture.output / 'owned-note.txt'
        require(not path.exists(), 'N12 seed must not overwrite existing user state')
        path.write_bytes(b'Owned N12 seed: preserve exactly.\n')
        digest = _digest(path)
        before = self.native()
        journal = len(self.events('save-file'))
        opening = self._open_save()
        self.capture('n12-save-panel')
        saving = self._named_button('Save')
        confirmation = self.run("const v=await app.look({role:'button',text:'Replace'}); return {elements:v.elements};")
        require(any(e.get('name') == 'Replace' for e in confirmation['result']['elements']), confirmation)
        self.capture('n12-overwrite-confirmation')
        overwrite_cancel = self._named_button('Cancel')
        require(self.native()['panel']['result'] == 'pending', self.native())
        self.capture('n12-underlying-save-panel')
        panel_cancel = self._named_button('Cancel')
        after = wait_until(lambda: p if (p := self.native()['panel'])['result'] == 'cancel' else None)
        require(not after['visible'] and after['file_saves'] == before['panel']['file_saves'], after)
        require(_digest(path) == digest, 'overwrite cancellation changed seeded file')
        rows = self.events('save-file')[journal:]
        require(len(rows) == 1 and rows[0]['result'] == 'cancel', rows)
        return {'seed_sha256': digest, 'unchanged': True, 'file_saves_delta': 0,
                'opening_receipt': opening, 'saving_receipt': saving,
                'overwrite_cancel_receipt': overwrite_cancel, 'panel_cancel_receipt': panel_cancel}

    def N13(self):
        """Require real hover callback and double-click; unsupported hover never activates."""
        self._native_mode()
        before = self.native()['gestures']
        foreground = self.state()['environment']['frontmost_pid']
        view = self.capture('n13-gesture-desk')
        point = self.anchor('gesture-desk', view)
        data, response = self.client.execute(self.app, 'await app.move(' + json.dumps(point) + ');', allow_error=True)
        if response.get('isError'):
            failure = data.get('failure', {})
            require(failure.get('code') == 'foreground_required', data)
            require(not data.get('effects', {}).get('focus_preparation'), data)
            require(data.get('effects', {}).get('dispatch', {}).get('not_dispatched') == 1, data)
            require(self.state()['environment']['frontmost_pid'] == foreground, self.state())
            require(self.native()['gestures'] == before, self.native())
            return {'unavailable': 'Background hover unsupported; no activation or replay.', 'receipt': data}
        hovered = wait_until(lambda: g if (g := self.native()['gestures']).get('hover', 0) > before.get('hover', 0) else None)
        require(hovered['hover'] == before.get('hover', 0) + 1, hovered)
        double = self.run('await app.click(' + json.dumps({**point, 'count': 2}) + ');')
        after = wait_until(lambda: g if (g := self.native()['gestures']).get('double_click', 0) != before.get('double_click', 0) else None)
        require(after['double_click'] == before.get('double_click', 0) + 1, after)
        require(self.state()['pressed_mouse_buttons'] == 0, self.state())
        return {'hover_delta': 1, 'double_click_delta': 1, 'hover_receipt': data,
                'double_click_receipt': double, 'cursor_route': 'caller must classify retained receipts with cursor audit'}

    def N14(self):
        """Move by titlebar drag; report unsupported stale proof when exec renews its image."""
        self._native_mode()
        view = self.capture('n14-before-move')
        before = self.state()
        main = next(w for w in before['windows'] if ' — Peer ' not in w['title'])
        left, top, width, _ = main['bounds']
        image = view['image']
        x, y, w, h = image['bounds']
        title = {'x': (left + width / 2 - x) * image['width'] / w,
                 'y': (top + 10 - y) * image['height'] / h}
        end = {**title, 'x': title['x'] + 35 * image['width'] / w}
        require(all(0 <= p['x'] < image['width'] and 0 <= p['y'] < image['height'] for p in (title, end)),
                'Titlebar drag endpoints must lie within captured image')
        self.run('await app.drag({path:' + json.dumps([title, end]) + '});')
        moved = wait_until(lambda: s if (s := self.state())['windows'][0]['bounds'] != main['bounds'] else None)
        require(self.native()['counter'] == before['native']['counter'], self.native())
        # The successful GUI action's returned image replaces Scene.viewport.
        # Simplified x/y points have no old observation token. Sending the old
        # numbers now would be new-image input, not a valid stale-image test.
        return {'unavailable': 'GUI movement refreshes the canvas baseline; simplified points cannot retain old image identity.',
                'bounds_before': main['bounds'], 'bounds_after': moved['windows'][0]['bounds'],
                'stale_point_dispatched': False}

    def N15(self):
        """Edit, cancel NSApp quit, commit, quit exact child, relaunch via same exec connection."""
        self._native_mode()
        value = 'Unsaved quit cancellation — 中文 🙂'
        old_pid = self.fixture.child.pid
        before = self.native()
        self.type_at('native-note', value, 'n15-unsaved-note')
        require(self.native()['dirty'], self.native())
        first_quit = self._native_attempt('await app.quit();')
        wait_until(lambda: self.native()['quit']['visible'])
        self.capture('n15-unsaved-quit-confirmation')
        cancelling = self._named_button('Keep Open')
        cancelled = wait_until(lambda: n if (n := self.native())['quit']['cancellations'] == before['quit']['cancellations'] + 1 else None)
        require(cancelled['note'] == value and self.fixture.child.poll() is None and not cancelled['quit']['visible'], cancelled)
        self.run("await app.click('Commit note');")
        require(not self.native()['dirty'] and self.native()['saved_note'] == value, self.native())
        final_quit = self._native_attempt('await app.quit();')
        require(self.fixture.child.wait(timeout=10) == 0, 'original owned process did not exit cleanly')
        # Restart goes through exec. Read the new oracle directly until the fixture
        # can bind its lifecycle observer to that exact newly published process.
        restarted = self.run('await app.look();')
        def new_state():
            state = json.loads((self.fixture.output / 'native-state.json').read_text(encoding='utf-8'))
            return state if state['pid'] != old_pid and state['ready'] else None
        state = wait_until(new_state)
        self.fixture.child = ProcessObserver(state['pid'], state.get('process_birth'), self.client, self.app)
        require(restarted.get('view', {}).get('reset') is True, restarted)
        require(self.native()['note'] == 'Initial native note' and self.native()['saves'] == 0, self.native())
        self.capture('n15-restarted-baseline')
        return {'old_pid': old_pid, 'new_pid': state['pid'], 'cancelled_note': value,
                'first_quit_receipt': first_quit, 'cancelling_receipt': cancelling,
                'final_quit_receipt': final_quit, 'restart_reset': True,
                'expected_detached_exit': True, 'process_birth': state['process_birth'],
                'restart_cleanup': 'ProcessObserver proves PID/birth exit; returncode remains None'}

    def _chat_menu(self, identifier, action, name):
        view = self.capture(name)
        self.run('await app.click(' + json.dumps({**self.anchor('chat-' + identifier, view), 'button': 'right'}) + ');')
        wait_until(lambda: self.chat()['menu_open'])
        require(self.chat()['selected_id'] == identifier, self.chat())
        popup = self.capture(name + '-menu')
        self.run('await app.click(' + json.dumps(self.anchor('chat-' + action, popup)) + ');')
        wait_until(lambda: not self.chat()['menu_open'])

    def C04(self):
        """Pixel context-menu edit of message-137; exact edit map and journal delta."""
        self._chat_mode()
        before = self.chat()
        count = len(self.events('chat-edit'))
        value = 'Release check 137 — edited 中文 café 🙂'
        self._chat_menu('message-137', 'edit', 'c04-target')
        require(self.chat()['editing_id'] == 'message-137', self.chat())
        self.type_at('chat-edit-field', value, 'c04-editor')
        view = self.capture('c04-apply')
        self.run('await app.click(' + json.dumps(self.anchor('chat-apply-edit', view)) + ');')
        after = wait_until(lambda: c if (c := self.chat())['edits'] != before['edits'] else None)
        require(after['edited_messages'] == {**before['edited_messages'], 'message-137': value}
                and after['edits'] == before['edits'] + 1, after)
        require(after['replies'] == before['replies'] and after['pins'] == before['pins'], after)
        rows = self.events('chat-edit')[count:]
        require(len(rows) == 1 and rows[0]['message_id'] == 'message-137' and rows[0]['text'] == value, rows)
        return {'message_id': 'message-137', 'text': value, 'edits_delta': 1, 'neighbours_unchanged': True}

    def C05(self):
        """Scroll older to named item 132, then pin that exact message through pixels."""
        self._chat_mode()
        before = self.chat()
        target = 'message-132'  # Trace samples attached · item 132
        require('chat-' + target not in self.state()['anchors'], 'Older target must start offscreen')
        after_scroll = before
        for step in range(32):
            if 'chat-' + target in self.state()['anchors']:
                break
            view = self.capture('c05-feed-' + str(step))
            previous_offset = after_scroll['scroll_offset']
            self.run('await app.scroll(' + json.dumps({**self.anchor('chat-feed', view), 'delta_y': -1}) + ');')
            after_scroll = wait_until(lambda: c if (c := self.chat())['scroll_offset'] != previous_offset else None)
            require(after_scroll['scroll_offset'] < previous_offset, after_scroll)
        require('chat-' + target in self.state()['anchors'],
                'Exact older item 132 did not become visible; no nearest-message substitution')
        self._chat_menu(target, 'pin', 'c05-older-message')
        after = wait_until(lambda: c if (c := self.chat())['pins'] != before['pins'] else None)
        require(after['pinned'] == target and after['pins'] == before['pins'] + 1, after)
        require(after['edited_messages'] == before['edited_messages'] and after['replies'] == before['replies'], after)
        return {'message_id': target, 'name': 'Trace samples attached · item 132',
                'scroll_before': before['scroll_offset'], 'scroll_after': after_scroll['scroll_offset'], 'pins_delta': 1}
