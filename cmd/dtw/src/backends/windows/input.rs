//! Foreground segments contain their complete key/button releases. Partial
//! insertion cleans only the confirmed held prefix before returning unknown.
use crate::backends::input_sequence::{
    normalized_text, valid_text, AdmissionError, Mutation, MutationGate,
};
use crate::backends::native_client::{check, failed};
use crate::computer::Error;
use serde_json::json;
use std::{
    sync::atomic::{AtomicU8, Ordering},
    time::{Duration, Instant},
};
use windows::{
    core::w,
    Win32::{
        Foundation::{CloseHandle, HANDLE, HWND, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::Threading::{
            AttachThreadInput, CreateMutexW, GetCurrentThreadId, ReleaseMutex, WaitForSingleObject,
        },
        UI::{
            Input::KeyboardAndMouse::*,
            WindowsAndMessaging::{
                GetAncestor, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
                PeekMessageW, SetForegroundWindow, GA_ROOT, GUITHREADINFO, MSG, PM_NOREMOVE,
            },
        },
    },
};
static GATE: MutationGate = MutationGate::new();
// Publication is inside the mutation gate, so shutdown cannot observe an
// acquired-but-unpublished mutex as clean. Bit 1 is held, bit 4 is uncertainty.
static LEASE_STATE: AtomicU8 = AtomicU8::new(0);
pub fn close() {
    GATE.close();
}
pub fn cleanup_failed() -> bool {
    GATE.cleanup_failed()
}
pub fn closing() -> bool {
    GATE.closing()
}
pub(super) fn fail_delivery() {
    GATE.fail_cleanup();
    GATE.close();
}
pub fn mutation(until: Instant) -> Result<Mutation<'static>, Error> {
    GATE.admit_before(until).map_err(|error| match error {
        AdmissionError::Expired => Error::TimedOut,
        AdmissionError::Closed => Error::Closed,
    })
}
pub fn quiesce(budget: Duration) -> bool {
    quiesce_transaction(&GATE, &LEASE_STATE, budget)
}
fn quiesce_transaction(gate: &MutationGate, lease: &AtomicU8, budget: Duration) -> bool {
    let until = Instant::now() + budget;
    if !gate.quiesce(budget) {
        return false;
    }
    loop {
        let state = lease.load(Ordering::SeqCst);
        if state & 1 == 0 {
            return state & 4 == 0 && !gate.cleanup_failed();
        }
        if Instant::now() >= until
            && lease
                .compare_exchange(state, state | 4, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            gate.fail_cleanup();
            return false;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
// On an uncertain release retain the handle until the owning process exits.
unsafe fn release_handle(handle: HANDLE) -> bool {
    if ReleaseMutex(handle).is_err() {
        GATE.fail_cleanup();
        return false;
    }
    if CloseHandle(handle).is_err() {
        GATE.fail_cleanup();
        return false;
    }
    true
}
pub struct Lease(HANDLE);
impl Lease {
    pub fn acquire(until: Instant) -> Result<Self, Error> {
        unsafe {
            let handle = CreateMutexW(None, false, w!("Local\\DesktopWorld.SharedInput.v1"))
                .map_err(failed)?;
            loop {
                if let Err(error) = check(until) {
                    let _ = CloseHandle(handle);
                    return Err(error);
                }
                if closing() {
                    let _ = CloseHandle(handle);
                    return Err(Error::Closed);
                }
                // Waiting owns no active mutation. Only the nonblocking
                // acquire + held-state publication is one admitted segment.
                let admission = match mutation(until) {
                    Ok(admission) => admission,
                    Err(error) => {
                        let _ = CloseHandle(handle);
                        return Err(error);
                    }
                };
                let status = WaitForSingleObject(handle, 0);
                if status == WAIT_ABANDONED {
                    release_handle(handle);
                    GATE.fail_cleanup();
                    return Err(failed(
                        "Previous input owner was abandoned; its cleanup is unconfirmed",
                    ));
                }
                if status != WAIT_OBJECT_0 && status != WAIT_TIMEOUT {
                    let error = std::io::Error::last_os_error();
                    let _ = CloseHandle(handle);
                    return Err(failed(error));
                }
                if status == WAIT_OBJECT_0 {
                    if closing() || cleanup_failed() {
                        release_handle(handle);
                        return Err(Error::Closed);
                    }
                    LEASE_STATE.store(1, Ordering::SeqCst);
                    return Ok(Self(handle));
                }
                drop(admission);
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        // Failed paired cleanup still owns this mutex; the supervisor reaps its
        // process before another owner may acquire it. Do not close the handle.
        if !cleanup_failed() {
            unsafe {
                if release_handle(self.0) {
                    LEASE_STATE.fetch_and(!1, Ordering::SeqCst);
                } else {
                    LEASE_STATE.fetch_or(4, Ordering::SeqCst);
                }
            }
        }
    }
}
pub fn no_held_keys() -> Result<(), Error> {
    for code in 1..=255 {
        if unsafe { GetAsyncKeyState(code) } < 0 {
            return Err(Error::Busy);
        }
    }
    Ok(())
}

// Adapted from CUA 0.38.0 input/inject.rs::force_foreground_attached at
// 85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19. The native worker is not the
// foreground process, so SetForegroundWindow alone can be refused by Windows.
// Link both the foreground and target queues for this transition. Attaching
// only the foreground queue can still be refused when Chrome owns foreground.
// Never inject a key to acquire focus.
struct ForegroundAttachment {
    current: u32,
    attached: Vec<u32>,
    refused: Vec<(u32, String)>,
}
impl ForegroundAttachment {
    unsafe fn acquire(target: HWND) -> Self {
        let foreground = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let target = GetWindowThreadProcessId(target, None);
        Self::acquire_threads(&[foreground, target])
    }
    unsafe fn acquire_threads(threads: &[u32]) -> Self {
        let current = GetCurrentThreadId();
        // AttachThreadInput requires both threads to have a message queue.
        // This worker may have done only pipe IO before its first focus call.
        let mut message = MSG::default();
        let _ = PeekMessageW(&mut message, HWND::default(), 0, 0, PM_NOREMOVE);
        let mut attachment = Self {
            current,
            attached: Vec::with_capacity(2),
            refused: Vec::with_capacity(2),
        };
        for &thread in threads {
            if thread != 0 && thread != current && !attachment.attached.contains(&thread) {
                match AttachThreadInput(current, thread, true).ok() {
                    Ok(()) => attachment.attached.push(thread),
                    Err(error) => attachment.refused.push((thread, error.to_string())),
                }
            }
        }
        // Queue attachment is assistance, not authorization. Like CUA, still
        // let Windows decide one SetForegroundWindow request when attachment
        // is unavailable (for example an inaccessible current foreground).
        attachment
    }
}
impl Drop for ForegroundAttachment {
    fn drop(&mut self) {
        self.detach();
    }
}
impl ForegroundAttachment {
    fn detach(&mut self) -> bool {
        let current = self.current;
        let clean = detach_owned(&mut self.attached, |thread| unsafe {
            AttachThreadInput(current, thread, false).as_bool()
        });
        if !clean {
            GATE.fail_cleanup();
        }
        clean
    }
}
fn detach_owned(threads: &mut Vec<u32>, detach: impl FnMut(u32) -> bool) -> bool {
    // Consume the owned list even on failure. Drop never repeats a detach whose
    // state is unknown; that worker instead retains its failed input lease.
    detach_all(&std::mem::take(threads), detach)
}
fn detach_all(threads: &[u32], mut detach: impl FnMut(u32) -> bool) -> bool {
    let mut clean = true;
    for &thread in threads.iter().rev() {
        clean &= detach(thread);
    }
    clean
}
pub fn focus(hwnd: HWND, until: Instant, dispatched: &mut bool) -> Result<(), Error> {
    unsafe {
        let mut activation = None;
        if GetForegroundWindow() != hwnd {
            let _mutation = mutation(until)?;
            let attachment = ForegroundAttachment::acquire(hwnd);
            check(until)?;
            no_held_keys()?;
            *dispatched = true;
            let before = GetForegroundWindow();
            let accepted = SetForegroundWindow(hwnd).as_bool();
            activation = Some((before.0 as usize, accepted, attachment.refused.clone()));
            drop(attachment);
            if cleanup_failed() {
                return Err(failed("Foreground input queue detach is unconfirmed"));
            }
        }
        // Cross-thread activation can finish after SetForegroundWindow returns.
        // Observe that one request briefly; never replay it or send target input
        // before the exact HWND is foreground. Waiting owns no mutation gate.
        let settle = until.min(Instant::now() + Duration::from_millis(200));
        while GetForegroundWindow() != hwnd && Instant::now() < settle {
            check(until)?;
            if closing() {
                return Err(Error::Closed);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if GetForegroundWindow() != hwnd {
            return Err(failed(format!(
                "Target did not become foreground; no keyboard input issued (target={}, actual={}, activation={activation:?})",
                hwnd.0 as usize,
                GetForegroundWindow().0 as usize,
            )));
        }
        Ok(())
    }
}
fn key(vk: u16, scan: u16, mut flags: KEYBD_EVENT_FLAGS) -> INPUT {
    if matches!(vk, 33..=40 | 45 | 46 | 91 | 92) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }

    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}
pub fn keyboard(text: Option<&str>, keys: Option<&[String]>) -> Result<Vec<INPUT>, Error> {
    let mut batch = Vec::new();
    if let Some(text) = text {
        if !valid_text(text, false) {
            return Err(failed("Text exceeds the UTF-8/encoded input contract"));
        }
        let units: Vec<_> = normalized_text(text).encode_utf16().collect();
        for unit in units {
            if unit == 10 || unit == 13 {
                batch.push(key(13, 0, KEYBD_EVENT_FLAGS(0)));
                batch.push(key(13, 0, KEYEVENTF_KEYUP));
            } else {
                batch.push(key(0, unit, KEYEVENTF_UNICODE));
                batch.push(key(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
            }
        }
    } else if let Some(keys) = keys {
        if keys.is_empty() || keys.len() > 16 {
            return Err(failed("Key chord needs 1..16 distinct keys"));
        }
        let mut codes = Vec::new();
        for name in keys {
            let code = keycode(name).ok_or(Error::UnsupportedCapability)?;
            if codes.contains(&code) {
                return Err(failed("Duplicate chord key"));
            }
            codes.push(code);
        }
        for code in &codes {
            batch.push(key(*code, 0, KEYBD_EVENT_FLAGS(0)));
        }
        for code in codes.iter().rev() {
            batch.push(key(*code, 0, KEYEVENTF_KEYUP));
        }
    }
    Ok(batch)
}
// Keep surrogate pairs within one indivisible scalar segment. A chord is one
// segment as well: its reverse releases must not be interrupted by target checks.
pub fn keyboard_segments(
    text: Option<&str>,
    keys: Option<&[String]>,
) -> Result<Vec<Vec<INPUT>>, Error> {
    if let Some(text) = text {
        if !valid_text(text, false) {
            return Err(failed("Text exceeds the UTF-8/encoded input contract"));
        }
        normalized_text(text)
            .chars()
            .map(|scalar| keyboard(Some(&scalar.to_string()), None))
            .collect()
    } else {
        Ok(vec![keyboard(None, keys)?])
    }
}
pub fn keycode(name: &str) -> Option<u16> {
    let upper = name.to_ascii_uppercase();
    Some(match upper.as_str() {
        "CTRL" | "CONTROL" => 17,
        "SHIFT" => 16,
        "ALT" | "OPTION" => 18,
        "META" | "SUPER" | "WIN" => 91,
        "ENTER" | "RETURN" => 13,
        "TAB" => 9,
        "ESC" | "ESCAPE" => 27,
        "SPACE" => 32,
        "BACKSPACE" => 8,
        "DELETE" => 46,
        "LEFT" => 37,
        "UP" => 38,
        "RIGHT" => 39,
        "DOWN" => 40,
        "HOME" => 36,
        "END" => 35,
        "PAGEUP" | "PAGE_UP" => 33,
        "PAGEDOWN" | "PAGE_DOWN" => 34,
        _ => {
            if upper.len() == 1 && upper.as_bytes()[0].is_ascii_alphanumeric() {
                upper.as_bytes()[0] as u16
            } else if let Some(n) = upper
                .strip_prefix('F')
                .and_then(|s| s.parse::<u16>().ok())
                .filter(|n| (1..=24).contains(n))
            {
                111 + n
            } else {
                return None;
            }
        }
    })
}
pub fn mouse(flags: MOUSE_EVENT_FLAGS, data: u32, dx: i32, dy: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data,
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}
pub struct SegmentCompletion {
    terminal_keypress: bool,
}
impl SegmentCompletion {
    pub fn terminal_keypress(&self) -> bool {
        self.terminal_keypress
    }
}
pub fn send(
    batch: &[INPUT],
    hwnd: HWND,
    until: Instant,
    dispatched: &mut bool,
    accepted: &mut usize,
    named_keypress: bool,
) -> Result<SegmentCompletion, Error> {
    *accepted = 0;
    if batch.is_empty() {
        return Ok(SegmentCompletion {
            terminal_keypress: false,
        });
    }
    let terminal_keypress = named_keypress && complete_named_keypress(batch);
    if named_keypress && !terminal_keypress {
        return Err(Error::InvalidObservation);
    }
    if unsafe { GetForegroundWindow() } != hwnd {
        return Err(failed("Foreground changed before dispatch"));
    }
    let keyboard = batch.iter().any(|event| event.r#type == INPUT_KEYBOARD);
    let mut queue = None;
    {
        let _mutation = mutation(until)?;
        let events = if keyboard {
            let attached = KeyboardQueue::attach(hwnd)?;
            no_held_keys()?;
            attached.validate(hwnd)?;
            let events = keyboard_drain_batch(batch);
            queue = Some(attached);
            events
        } else {
            batch.to_vec()
        };
        let insertion = checked_insert(
            &events,
            batch.len(),
            || {
                // Queue preparation can take time after mutation admission. A stop
                // or expiry before the first payload must still issue zero input.
                if keyboard {
                    keyboard_preflight(until, Instant::now(), closing(), cleanup_failed())?;
                }
                *dispatched = true;
                Ok(())
            },
            |events| unsafe { SendInput(events, std::mem::size_of::<INPUT>() as i32) },
        )?;
        *accepted = insertion.accepted;
        if !insertion.complete {
            if !insertion.cleanup_complete {
                GATE.fail_cleanup();
            }
            // A short keyboard prefix has no complete read marker. Even paired
            // global releases cannot prove the target consumed that prefix.
            if keyboard && insertion.accepted > 0 {
                fail_delivery();
            }
            return Err(failed(if insertion.cleanup_complete {
                "Input batch only partially inserted; effects unknown"
            } else {
                "Input insertion and paired cleanup incomplete"
            }));
        }
    }
    // The complete key/marker pairs were inserted under one mutation, but the
    // read-only wait owns no mutation. Store::action still owns its single lease.
    let queue_result = if let Some(queue) = queue.as_mut() {
        let drained = queue.await_read(hwnd, until, terminal_keypress.then_some(batch));
        let detached = queue.finish();
        Some((drained, detached))
    } else {
        None
    };
    complete_segment(
        terminal_keypress,
        queue_result,
        || unsafe { GetForegroundWindow() } == hwnd,
    )
}
fn complete_segment(
    terminal_keypress: bool,
    queue_result: Option<(Result<(), Error>, Result<(), Error>)>,
    exact_foreground: impl FnOnce() -> bool,
) -> Result<SegmentCompletion, Error> {
    if let Some((drained, detached)) = queue_result {
        drained?;
        detached?;
    } else if terminal_keypress {
        return Err(Error::InvalidObservation);
    }
    if !terminal_keypress && !exact_foreground() {
        return Err(failed("Foreground changed after dispatch; effects unknown"));
    }
    // Only a complete named-key batch whose marker was read and whose queue
    // detached reaches this token. Closing a window or opening a dialog is a
    // legitimate consequence; this remains dispatch evidence, not effect proof.
    Ok(SegmentCompletion { terminal_keypress })
}
fn complete_named_keypress(batch: &[INPUT]) -> bool {
    !batch.is_empty()
        && batch.iter().all(|event| unsafe {
            event.r#type == INPUT_KEYBOARD
                && event.Anonymous.ki.wVk.0 != 0
                && !event.Anonymous.ki.dwFlags.contains(KEYEVENTF_UNICODE)
        })
        && pending_releases(batch).is_empty()
}
pub fn terminal_keypress_boundary(until: Instant) -> Result<(), Error> {
    keyboard_preflight(until, Instant::now(), closing(), cleanup_failed())?;
    no_held_keys()
}

// Same unassigned mask key used by pinned CUA's queue-drain implementation.
// It produces no character. A changed toggle bit plus key-up proves only that
// the attached target queue read the tail, never that Save/another action worked.
const DRAIN_KEY: u16 = 0xe8;
fn keyboard_drain_batch(batch: &[INPUT]) -> Vec<INPUT> {
    batch
        .iter()
        .copied()
        .chain([
            key(DRAIN_KEY, 0, KEYBD_EVENT_FLAGS(0)),
            key(DRAIN_KEY, 0, KEYEVENTF_KEYUP),
        ])
        .collect()
}
struct Insertion {
    accepted: usize,
    complete: bool,
    cleanup_complete: bool,
}
fn keyboard_preflight(
    until: Instant,
    now: Instant,
    stopping: bool,
    cleanup_failed: bool,
) -> Result<(), Error> {
    if now >= until {
        return Err(Error::TimedOut);
    }
    if stopping {
        return Err(Error::Closed);
    }
    if cleanup_failed {
        return Err(Error::NativeFailure {
            code: "input_cleanup_unconfirmed".into(),
            message:
                "Input cleanup changed during keyboard preparation; no keyboard input was issued"
                    .into(),
        });
    }
    Ok(())
}
fn checked_insert(
    events: &[INPUT],
    payload: usize,
    preflight: impl FnOnce() -> Result<(), Error>,
    send: impl FnMut(&[INPUT]) -> u32,
) -> Result<Insertion, Error> {
    preflight()?;
    Ok(insert_once(events, payload, send))
}
fn insert_once(
    events: &[INPUT],
    payload: usize,
    mut send: impl FnMut(&[INPUT]) -> u32,
) -> Insertion {
    let inserted = send(events) as usize;
    let complete = inserted == events.len();
    let releases = if complete {
        Vec::new()
    } else {
        pending_releases(&events[..inserted.min(events.len())])
    };
    let cleanup_complete = releases.is_empty() || send(&releases) as usize == releases.len();
    Insertion {
        accepted: inserted.min(payload),
        complete,
        cleanup_complete,
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct QueueTarget {
    top_thread: u32,
    focus_thread: u32,
    receiver_thread: u32,
}
impl QueueTarget {
    unsafe fn read(window: HWND) -> Result<Self, Error> {
        let top_thread = GetWindowThreadProcessId(window, None);
        if top_thread == 0 {
            return Err(Error::TargetChanged);
        }
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        GetGUIThreadInfo(top_thread, &mut info).map_err(failed)?;
        let focus_thread = GetWindowThreadProcessId(info.hwndFocus, None);
        let receiver_thread = if focus_thread != 0 && GetAncestor(info.hwndFocus, GA_ROOT) == window
        {
            focus_thread
        } else {
            // A menu/modal can report a focus outside this root. Attach only
            // the known target thread, never an unproven foreign focus thread.
            top_thread
        };
        Ok(Self {
            top_thread,
            focus_thread,
            receiver_thread,
        })
    }
}
struct KeyboardQueue {
    target: QueueTarget,
    attachment: ForegroundAttachment,
    before: i16,
}
fn after_attach<T>(
    attach: impl FnOnce() -> Result<T, Error>,
    baseline: impl FnOnce() -> i16,
) -> Result<(T, i16), Error> {
    let attached = attach()?;
    Ok((attached, baseline()))
}
impl KeyboardQueue {
    unsafe fn read_state() -> i16 {
        GetKeyState(DRAIN_KEY as i32)
    }
    fn attach(window: HWND) -> Result<Self, Error> {
        unsafe {
            let target = QueueTarget::read(window)?;
            // AttachThreadInput resets shared key state. Read the baseline only
            // AFTER attachment, or its reset could masquerade as our marker ACK.
            let (attachment, before) = after_attach(
                || {
                    let mut attachment = ForegroundAttachment::acquire_threads(&[
                        target.top_thread,
                        target.receiver_thread,
                    ]);
                    if !attachment.refused.is_empty() || attachment.attached.is_empty() {
                        let error = Error::NativeFailure {
                        code:"keyboard_queue_unavailable".into(),
                        message:format!("Cannot attach the selected window's keyboard queue ({target:?}, refused={:?}); no keyboard input was issued",attachment.refused),
                        };
                        if !attachment.detach() {
                            fail_delivery();
                        }
                        return Err(error);
                    }
                    Ok(attachment)
                },
                || Self::read_state(),
            )?;
            Ok(Self {
                target,
                attachment,
                before,
            })
        }
    }
    fn validate(&self, window: HWND) -> Result<(), Error> {
        if unsafe { GetForegroundWindow() } != window
            || unsafe { QueueTarget::read(window)? } != self.target
        {
            return Err(Error::NativeFailure {
                code:"keyboard_queue_changed".into(),
                message:"The exact foreground window or keyboard receiver changed before dispatch; no keyboard input was issued".into(),
            });
        }
        Ok(())
    }
    fn finish(&mut self) -> Result<(), Error> {
        if !self.attachment.detach() {
            fail_delivery();
            return Err(Error::NativeFailure {
                code:"keyboard_queue_detach_unconfirmed".into(),
                message:"The keyboard queue detach is unconfirmed; effects are unknown and further UIA delivery reads are prohibited".into(),
            });
        }
        Ok(())
    }
    fn await_read(
        &self,
        window: HWND,
        until: Instant,
        named: Option<&[INPUT]>,
    ) -> Result<(), Error> {
        let started = Instant::now();
        let keys = named.map(|batch| {
            let mut keys = Vec::new();
            for event in batch {
                let vk = unsafe { event.Anonymous.ki.wVk.0 };
                if !keys.contains(&vk) {
                    keys.push(vk);
                }
            }
            keys
        });
        let drain = wait_for_marker(
            self.before,
            until,
            || unsafe { Self::read_state() },
            || {
                keys.as_ref().map(|keys| unsafe {
                    NamedReleaseEvidence {
                        marker_async: GetAsyncKeyState(DRAIN_KEY as i32),
                        keys: keys
                            .iter()
                            .map(|vk| KeyReleaseEvidence {
                                vk: *vk,
                                synchronous: GetKeyState(*vk as i32),
                                asynchronous: GetAsyncKeyState(*vk as i32),
                            })
                            .collect(),
                    }
                })
            },
            closing,
            Instant::now,
            || std::thread::sleep(Duration::from_millis(1)),
        );
        self.trace(
            if drain.read { "read" } else { "unconfirmed" },
            window,
            &drain,
            started.elapsed(),
        );
        if drain.read {
            return Ok(());
        }
        fail_delivery();
        Err(Error::NativeFailure {
            code:"keyboard_delivery_unconfirmed".into(),
            message:format!("The inserted keyboard batch's target queue did not confirm its payload tail and release boundary ({:?}, marker_before={}, marker_now={}, payload_tail_seen={}, sentinel_release_read={}, release={:?}, stopping={}); effects are unknown and no input was replayed",self.target,self.before,drain.state,drain.payload_tail_seen,drain.sentinel_release_read,drain.named_release,drain.stopping),
        })
    }
    fn trace(&self, stage: &str, window: HWND, drain: &Drain, elapsed: Duration) {
        if std::env::var("DTW_AX_READINESS_TRACE").ok().as_deref() == Some("1") {
            eprintln!(
                "DTW Windows keyboard queue: {}",
                json!({"stage":stage,"pid":std::process::id(),
                "top_thread":self.target.top_thread,"focus_thread":self.target.focus_thread,
                "receiver_thread":self.target.receiver_thread,"attached_threads":self.attachment.attached,
                "marker_before":self.before,"marker_now":drain.state,"elapsed_us":elapsed.as_micros(),
                "payload_tail_seen":drain.payload_tail_seen,"sentinel_release_read":drain.sentinel_release_read,
                "named_release":drain.named_release.as_ref().map(NamedReleaseEvidence::json),
                "target":window.0 as usize,"foreground":unsafe {GetForegroundWindow()}.0 as usize})
            );
        }
    }
}
impl Drop for KeyboardQueue {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
fn marker_read(before: i16, state: i16) -> bool {
    before & 1 != state & 1 && state >= 0
}
struct Drain {
    state: i16,
    read: bool,
    stopping: bool,
    payload_tail_seen: bool,
    sentinel_release_read: bool,
    named_release: Option<NamedReleaseEvidence>,
}
#[derive(Debug)]
struct KeyReleaseEvidence {
    vk: u16,
    synchronous: i16,
    asynchronous: i16,
}
#[derive(Debug)]
struct NamedReleaseEvidence {
    marker_async: i16,
    keys: Vec<KeyReleaseEvidence>,
}
impl NamedReleaseEvidence {
    fn json(&self) -> serde_json::Value {
        json!({"marker_async":self.marker_async,"keys":self.keys.iter().map(|key|
            json!({"vk":key.vk,"synchronous":key.synchronous,"asynchronous":key.asynchronous})
        ).collect::<Vec<_>>()})
    }
    fn payload_released(&self) -> bool {
        self.marker_async >= 0
            && !self.keys.is_empty()
            && self
                .keys
                .iter()
                .all(|key| key.synchronous >= 0 && key.asynchronous >= 0)
    }
}
fn wait_for_marker(
    before: i16,
    until: Instant,
    mut state: impl FnMut() -> i16,
    mut named_release: impl FnMut() -> Option<NamedReleaseEvidence>,
    mut stopping: impl FnMut() -> bool,
    mut now: impl FnMut() -> Instant,
    mut pause: impl FnMut(),
) -> Drain {
    let mut deadline = until.min(now() + Duration::from_secs(2));
    let mut closing = false;
    let mut payload_tail_seen = false;
    loop {
        let state = state();
        payload_tail_seen |= before & 1 != state & 1;
        let sentinel_release_read = marker_read(before, state);
        let named_release = named_release();
        // A complete named chord's payload precedes the marker down. Once that
        // down was read in this original attachment, separately prove every
        // business key released in both synchronous and global state, and the
        // unassigned marker released globally. Unicode retains strict marker-up.
        let read = match named_release.as_ref() {
            Some(release) => payload_tail_seen && release.payload_released(),
            None => sentinel_release_read,
        };
        if read {
            return Drain {
                state,
                read: true,
                stopping: closing,
                payload_tail_seen,
                sentinel_release_read,
                named_release,
            };
        }
        let current = now();
        if stopping() && !closing {
            closing = true;
            deadline = deadline.min(current + Duration::from_millis(250));
        }
        if current >= deadline {
            return Drain {
                state,
                read: false,
                stopping: closing,
                payload_tail_seen,
                sentinel_release_read,
                named_release,
            };
        }
        pause();
    }
}

// SendInput reports the inserted prefix. Release only keys/buttons which that
// prefix actually left down; never synthesize ups for events not submitted.
fn pending_releases(prefix: &[INPUT]) -> Vec<INPUT> {
    let mut held: Vec<(u8, u16, u16, INPUT)> = Vec::new();
    for event in prefix {
        unsafe {
            if event.r#type == INPUT_KEYBOARD {
                let k = event.Anonymous.ki;
                if k.dwFlags.contains(KEYEVENTF_KEYUP) {
                    if let Some(index) = held.iter().rposition(|(kind, vk, scan, _)| {
                        *kind == 0 && *vk == k.wVk.0 && *scan == k.wScan
                    }) {
                        held.remove(index);
                    }
                } else {
                    held.push((
                        0,
                        k.wVk.0,
                        k.wScan,
                        key(k.wVk.0, k.wScan, k.dwFlags | KEYEVENTF_KEYUP),
                    ));
                }
            } else if event.r#type == INPUT_MOUSE {
                let flags = event.Anonymous.mi.dwFlags;
                for (kind, down, up) in [
                    (1, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
                    (2, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
                ] {
                    if flags.contains(up) {
                        if let Some(index) = held.iter().rposition(|(k, _, _, _)| *k == kind) {
                            held.remove(index);
                        }
                    }
                    if flags.contains(down) {
                        held.push((kind, 0, 0, mouse(up, 0, 0, 0)));
                    }
                }
            }
        }
    }
    held.into_iter()
        .rev()
        .map(|(_, _, _, release)| release)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fields(batch: &[INPUT]) -> Vec<(u16, u16, u32)> {
        batch
            .iter()
            .map(|event| unsafe {
                assert_eq!(event.r#type, INPUT_KEYBOARD);
                let k = event.Anonymous.ki;
                (k.wVk.0, k.wScan, k.dwFlags.0)
            })
            .collect()
    }
    #[test]
    fn terminal_keypress_requires_complete_named_batch_and_successful_read_and_detach() {
        let chord = keyboard(None, Some(&["ALT".into(), "F4".into()])).unwrap();
        assert!(complete_named_keypress(&chord));
        assert!(!complete_named_keypress(&chord[..chord.len() - 1]));
        assert!(!complete_named_keypress(&[]));
        assert!(!complete_named_keypress(
            &keyboard(Some("x"), None).unwrap()
        ));
        assert!(!complete_named_keypress(&[
            mouse(MOUSEEVENTF_LEFTDOWN, 0, 0, 0),
            mouse(MOUSEEVENTF_LEFTUP, 0, 0, 0)
        ]));
        for queue in [
            None,
            Some((Err(Error::TimedOut), Ok(()))),
            Some((Ok(()), Err(Error::Closed))),
        ] {
            assert!(complete_segment(true, queue, || panic!(
                "no foreground read before valid queue completion"
            ))
            .is_err());
        }
        let completion = complete_segment(true, Some((Ok(()), Ok(()))), || {
            panic!("completed chord may close its target")
        })
        .unwrap();
        assert!(completion.terminal_keypress());
    }
    #[test]
    fn nonterminal_keyboard_and_mouse_keep_exact_foreground_tail_check() {
        for queue in [None, Some((Ok(()), Ok(())))] {
            assert!(complete_segment(false, queue, || false).is_err());
        }
        assert!(!complete_segment(false, None, || true)
            .unwrap()
            .terminal_keypress());
        assert!(!complete_segment(false, Some((Ok(()), Ok(()))), || true)
            .unwrap()
            .terminal_keypress());
    }
    #[test]
    fn complete_chord_and_marker_are_one_batch_but_acceptance_counts_only_payload() {
        let payload = keyboard(None, Some(&["CTRL".into(), "S".into()])).unwrap();
        let events = keyboard_drain_batch(&payload);
        assert_eq!(fields(&events[..payload.len()]), fields(&payload));
        assert_eq!(
            fields(&events[payload.len()..]),
            [(DRAIN_KEY, 0, 0), (DRAIN_KEY, 0, KEYEVENTF_KEYUP.0)]
        );
        let mut calls = 0;
        let inserted = insert_once(&events, payload.len(), |batch| {
            calls += 1;
            assert_eq!(fields(batch), fields(&events));
            batch.len() as u32
        });
        assert_eq!(calls, 1);
        assert!(inserted.complete && inserted.cleanup_complete);
        assert_eq!(inserted.accepted, 4);
    }
    #[test]
    fn partial_marker_cleans_only_its_known_down_and_never_replays_business_keys() {
        let payload = keyboard(None, Some(&["CTRL".into(), "S".into()])).unwrap();
        let events = keyboard_drain_batch(&payload);
        for cleanup_success in [true, false] {
            let mut calls = 0;
            let inserted = insert_once(&events, payload.len(), |batch| {
                calls += 1;
                if calls == 1 {
                    return 5;
                }
                assert_eq!(fields(batch), [(DRAIN_KEY, 0, KEYEVENTF_KEYUP.0)]);
                u32::from(cleanup_success)
            });
            assert_eq!(calls, 2);
            assert_eq!(inserted.accepted, 4);
            assert!(!inserted.complete);
            assert_eq!(inserted.cleanup_complete, cleanup_success);
        }
    }
    #[test]
    fn partial_payload_cleans_inserted_prefix_and_zero_insertion_has_no_cleanup_events() {
        let payload = keyboard(None, Some(&["CTRL".into(), "S".into()])).unwrap();
        let events = keyboard_drain_batch(&payload);
        let mut calls = 0;
        let inserted = insert_once(&events, payload.len(), |batch| {
            calls += 1;
            if calls == 1 {
                return 2;
            }
            assert_eq!(
                fields(batch),
                [(83, 0, KEYEVENTF_KEYUP.0), (17, 0, KEYEVENTF_KEYUP.0)]
            );
            2
        });
        assert_eq!(inserted.accepted, 2);
        assert!(!inserted.complete && inserted.cleanup_complete);
        let mut calls = 0;
        let inserted = insert_once(&events, payload.len(), |_| {
            calls += 1;
            0
        });
        assert_eq!(calls, 1);
        assert_eq!(inserted.accepted, 0);
        assert!(inserted.cleanup_complete && !inserted.complete);
    }
    #[test]
    fn baseline_is_read_after_attach_reset_and_failed_attachment_never_reads_it() {
        let state = std::cell::Cell::new(1i16);
        let (_, baseline) = after_attach(
            || {
                state.set(0);
                Ok(())
            },
            || state.get(),
        )
        .unwrap();
        assert_eq!(baseline, 0);
        assert!(!marker_read(baseline, state.get()));
        assert_eq!(
            after_attach::<()>(
                || Err(Error::TargetChanged),
                || panic!("no baseline after failed attach")
            ),
            Err(Error::TargetChanged)
        );
    }
    #[test]
    fn marker_requires_changed_state_and_key_up_in_both_toggle_directions() {
        assert!(!marker_read(0, 0));
        assert!(!marker_read(0, 0x8001u16 as i16));
        assert!(marker_read(0, 1));
        assert!(marker_read(1, 0));
        assert!(!marker_read(1, 1));
        assert!(!marker_read(1, 0x8000u16 as i16));
    }
    #[test]
    fn cancellation_keeps_a_bounded_read_window_and_original_deadline_is_not_extended() {
        let start = Instant::now();
        for (original_ms, expected_ms) in [(1000u64, 250u64), (10, 10)] {
            let ticks = std::cell::Cell::new(0u64);
            let result = wait_for_marker(
                0,
                start + Duration::from_millis(original_ms),
                || 0,
                || None,
                || true,
                || start + Duration::from_millis(ticks.get()),
                || ticks.set(ticks.get() + 1),
            );
            assert!(!result.read && result.stopping);
            assert_eq!(ticks.get(), expected_ms);
        }
        let ticks = std::cell::Cell::new(0u64);
        let result = wait_for_marker(
            0,
            start + Duration::from_secs(2),
            || if ticks.get() < 3 { 0x8001u16 as i16 } else { 1 },
            || None,
            || true,
            || start + Duration::from_millis(ticks.get()),
            || ticks.set(ticks.get() + 1),
        );
        assert!(result.read && result.stopping);
        assert_eq!(ticks.get(), 3);
    }
    fn released_named_keys() -> NamedReleaseEvidence {
        NamedReleaseEvidence {
            marker_async: 0,
            keys: vec![
                KeyReleaseEvidence {
                    vk: 17,
                    synchronous: 0,
                    asynchronous: 0,
                },
                KeyReleaseEvidence {
                    vk: 83,
                    synchronous: 0,
                    asynchronous: 0,
                },
            ],
        }
    }
    fn sample_named_tail(
        marker: i16,
        release: impl FnMut() -> Option<NamedReleaseEvidence>,
    ) -> Drain {
        let start = Instant::now();
        let ticks = std::cell::Cell::new(0u64);
        wait_for_marker(
            0,
            start + Duration::from_millis(4),
            || marker,
            release,
            || false,
            || start + Duration::from_millis(ticks.get()),
            || ticks.set(ticks.get() + 1),
        )
    }
    #[test]
    fn named_tail_can_prove_payload_released_without_claiming_sentinel_up_read() {
        let down = 0xff81u16 as i16;
        let named = sample_named_tail(down, || Some(released_named_keys()));
        assert!(named.read && named.payload_tail_seen);
        assert!(!named.sentinel_release_read);
        let unicode = sample_named_tail(down, || None);
        assert!(!unicode.read && unicode.payload_tail_seen && !unicode.sentinel_release_read);
        let no_flip = sample_named_tail(0, || Some(released_named_keys()));
        assert!(!no_flip.read && !no_flip.payload_tail_seen);
    }
    #[test]
    fn named_tail_refuses_synchronous_or_global_business_keys_and_global_marker_down() {
        for which in 0..3 {
            let result = sample_named_tail(0xff81u16 as i16, || {
                let mut evidence = released_named_keys();
                match which {
                    0 => evidence.keys[0].synchronous = -128,
                    1 => evidence.keys[0].asynchronous = -128,
                    _ => evidence.marker_async = -128,
                }
                Some(evidence)
            });
            assert!(!result.read && result.payload_tail_seen);
        }
        let empty = sample_named_tail(1, || {
            Some(NamedReleaseEvidence {
                marker_async: 0,
                keys: vec![],
            })
        });
        assert!(!empty.read);
    }
    #[test]
    fn tail_flip_is_latched_only_inside_original_wait_while_business_release_settles() {
        let ticks = std::cell::Cell::new(0u64);
        let start = Instant::now();
        let result = wait_for_marker(
            0,
            start + Duration::from_millis(10),
            || {
                if ticks.get() == 0 {
                    0xff81u16 as i16
                } else {
                    0
                }
            },
            || {
                let mut release = released_named_keys();
                if ticks.get() == 0 {
                    release.keys[0].synchronous = -128;
                }
                Some(release)
            },
            || false,
            || start + Duration::from_millis(ticks.get()),
            || ticks.set(ticks.get() + 1),
        );
        assert!(result.read && result.payload_tail_seen);
        assert!(!result.sentinel_release_read);
        assert_eq!(ticks.get(), 1);
    }
    #[test]
    fn unconfirmed_unicode_pair_cancellation_never_dispatches_the_next_segment() {
        let start = Instant::now();
        let ticks = std::cell::Cell::new(0u64);
        let mut dispatched = Vec::new();
        let result = crate::backends::input_sequence::run(&[0, 1], |step| {
            if let crate::backends::input_sequence::Step::Dispatch(segment) = step {
                dispatched.push(*segment);
                let tail = wait_for_marker(
                    0,
                    start + Duration::from_secs(1),
                    || 0xff81u16 as i16,
                    || None,
                    || true,
                    || start + Duration::from_millis(ticks.get()),
                    || ticks.set(ticks.get() + 1),
                );
                if !tail.read {
                    return Err(Error::Closed);
                }
            }
            Ok(())
        });
        assert_eq!(result, Err(Error::Closed));
        assert_eq!(dispatched, [0]);
        assert_eq!(ticks.get(), 250);
    }
    #[test]
    fn detach_failure_does_not_skip_other_owned_attachments() {
        let mut detached = Vec::new();
        let mut owned = vec![10, 20];
        assert!(!detach_owned(&mut owned, |thread| {
            detached.push(thread);
            thread != 20
        }));
        assert_eq!(detached, [20, 10]);
        assert!(owned.is_empty());
        assert!(detach_owned(&mut owned, |_| panic!(
            "Drop must not retry an uncertain detach"
        )));
    }
    #[test]
    fn preparation_expiry_stop_or_cleanup_failure_never_calls_the_native_sender() {
        let start = Instant::now();
        let until = start + Duration::from_millis(10);
        let events =
            keyboard_drain_batch(&keyboard(None, Some(&["CTRL".into(), "S".into()])).unwrap());
        for (now, stopping, failed) in [
            (until, false, false),
            (start, true, false),
            (start, false, true),
        ] {
            let calls = std::cell::Cell::new(0);
            let result = checked_insert(
                &events,
                4,
                || keyboard_preflight(until, now, stopping, failed),
                |_| {
                    calls.set(calls.get() + 1);
                    6
                },
            );
            assert!(result.is_err());
            assert_eq!(calls.get(), 0);
        }
        let result = checked_insert(
            &events,
            4,
            || keyboard_preflight(until, start, false, false),
            |batch| batch.len() as u32,
        )
        .unwrap();
        assert!(result.complete && result.cleanup_complete);
        assert_eq!(result.accepted, 4);
    }
    #[test]
    fn stop_waits_for_transaction_release_after_the_last_input_pair() {
        let gate = std::sync::Arc::new(MutationGate::new());
        let lease = std::sync::Arc::new(AtomicU8::new(1));
        let (other_gate, other_lease) = (gate.clone(), lease.clone());
        let release = std::thread::spawn(move || {
            while !other_gate.closing() {
                std::thread::yield_now();
            }
            std::thread::sleep(Duration::from_millis(10));
            other_lease.fetch_and(!1, Ordering::SeqCst);
        });
        assert!(quiesce_transaction(&gate, &lease, Duration::from_secs(1)));
        assert_eq!(lease.load(Ordering::SeqCst), 0);
        release.join().unwrap();
        let gate = MutationGate::new();
        let held = AtomicU8::new(1);
        assert!(!quiesce_transaction(&gate, &held, Duration::ZERO));
        assert!(gate.cleanup_failed());
        assert_eq!(held.load(Ordering::SeqCst), 5);
        assert!(quiesce_transaction(
            &MutationGate::new(),
            &AtomicU8::new(0),
            Duration::ZERO
        ));
    }
    #[test]
    fn unicode_surrogates_and_newlines_have_balanced_releases() {
        let batch = keyboard(Some("中😀\r\n"), None).unwrap();
        assert_eq!(batch.len(), 8);
        for pair in batch.chunks_exact(2) {
            unsafe {
                let down = pair[0].Anonymous.ki;
                let up = pair[1].Anonymous.ki;
                assert_eq!((down.wVk, down.wScan), (up.wVk, up.wScan));
                assert!(!down.dwFlags.contains(KEYEVENTF_KEYUP));
                assert!(up.dwFlags.contains(KEYEVENTF_KEYUP));
            }
        }
        assert_eq!(unsafe { batch[6].Anonymous.ki.wVk }, VIRTUAL_KEY(13));
    }
    #[test]
    fn text_segments_keep_one_scalar_and_complete_chord_together() {
        let segments = keyboard_segments(Some("中😀\r\n"), None).unwrap();
        assert_eq!(
            segments.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![2, 4, 2]
        );
        assert_eq!(
            keyboard_segments(Some(&"a".repeat(16384)), None)
                .unwrap()
                .len(),
            16384
        );
        assert!(keyboard_segments(Some(&"a".repeat(16385)), None).is_err());
        let chord = keyboard_segments(None, Some(&["CTRL".into(), "A".into()])).unwrap();
        assert_eq!(chord.len(), 1);
        assert_eq!(chord[0].len(), 4);
    }
    #[test]
    fn partial_batch_releases_only_its_inserted_held_prefix() {
        let chord = keyboard(None, Some(&["CTRL".into(), "A".into()])).unwrap();
        assert!(pending_releases(&chord[..0]).is_empty());
        assert_eq!(pending_releases(&chord[..1]).len(), 1);
        assert_eq!(pending_releases(&chord[..2]).len(), 2);
        let remaining = pending_releases(&chord[..3]);
        assert_eq!(remaining.len(), 1);
        assert_eq!(unsafe { remaining[0].Anonymous.ki.wVk }, VIRTUAL_KEY(17));
        assert!(pending_releases(&chord).is_empty());
        let text = keyboard(Some("ab"), None).unwrap();
        assert!(pending_releases(&text[..2]).is_empty());
        let partial = pending_releases(&text[..3]);
        assert_eq!(partial.len(), 1);
        assert_eq!(unsafe { partial[0].Anonymous.ki.wScan }, 'b' as u16);
    }
    #[test]
    fn chord_is_validated_before_any_input_is_constructed() {
        assert!(keyboard(None, Some(&["CTRL".into(), "Control".into()])).is_err());
        assert!(keyboard(None, Some(&["CTRL".into(), "UNKNOWN".into()])).is_err());
        let batch = keyboard(None, Some(&["CTRL".into(), "SHIFT".into(), "A".into()])).unwrap();
        let codes: Vec<_> = batch
            .iter()
            .map(|input| unsafe { input.Anonymous.ki.wVk.0 })
            .collect();
        assert_eq!(codes, vec![17, 16, 65, 65, 16, 17]);
    }
}
