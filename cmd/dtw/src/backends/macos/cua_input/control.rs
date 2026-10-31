//! DTW ownership around the port: one admitted call, validated delivery and paired cleanup.
use core_graphics::event::{CGEvent, CGEventFlags, CGEventType};
use foreign_types::ForeignType;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    ffi::c_void,
    sync::{Condvar, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub type Validate = extern "C" fn(*mut c_void, i32) -> i32;
#[derive(Default)]
struct Lifecycle {
    active: bool,
    closed: bool,
    cleanup_failed: bool,
}
static LIFECYCLE: Mutex<Lifecycle> = Mutex::new(Lifecycle {
    active: false,
    closed: false,
    cleanup_failed: false,
});
static FINISHED: Condvar = Condvar::new();
thread_local! { static CALL: RefCell<Option<State>> = const { RefCell::new(None) }; }
struct Pending {
    id: u32,
    event: CGEvent,
    auth: bool,
    public: bool,
    keyboard: bool,
}
struct State {
    context: *mut c_void,
    validate: Validate,
    deadline: u64,
    pid: i32,
    key_window: Option<u32>,
    pointer_public: bool,
    prepared: bool,
    dispatched: bool,
    error: Option<String>,
    cleanup_error: Option<String>,
    cleanup: bool,
    cleanup_deadline: Option<Instant>,
    pending: Vec<Pending>,
}
pub struct Session {
    finished: bool,
}
impl Session {
    pub fn start(
        pid: i32,
        deadline: u64,
        context: *mut c_void,
        validate: Validate,
        pointer_public: bool,
    ) -> Result<Self, Value> {
        let mut lifecycle = LIFECYCLE.lock().unwrap_or_else(|p| p.into_inner());
        if lifecycle.closed || lifecycle.cleanup_failed {
            return Err(rejected(
                "closed",
                "input owner is closing or has an unresolved cleanup failure",
                false,
            ));
        }
        if lifecycle.active {
            return Err(rejected(
                "busy",
                "input owner already has an active call",
                false,
            ));
        }
        lifecycle.active = true;
        CALL.with(|c| {
            *c.borrow_mut() = Some(State {
                context,
                validate,
                deadline,
                pid,
                key_window: None,
                pointer_public,
                prepared: false,
                dispatched: false,
                error: None,
                cleanup_error: None,
                cleanup: false,
                cleanup_deadline: None,
                pending: Vec::new(),
            })
        });
        Ok(Self { finished: false })
    }
    pub fn finish(mut self, result: anyhow::Result<()>, background_unavailable: bool) -> Value {
        if let Err(error) = result {
            fail(error.to_string());
        }
        cleanup_pending();
        let state = CALL
            .with(|c| c.borrow_mut().take())
            .expect("admitted input call");
        let response = outcome(
            state.prepared,
            state.dispatched,
            state.error,
            state.cleanup_error.clone(),
            background_unavailable,
        );
        self.finished = true;
        self.end(state.cleanup_error.is_some());
        response
    }
    fn end(&self, cleanup_failed: bool) {
        let mut state = LIFECYCLE.lock().unwrap_or_else(|p| p.into_inner());
        state.cleanup_failed |= cleanup_failed;
        state.active = false;
        FINISHED.notify_all();
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        cleanup_pending();
        let failed = CALL.with(|c| {
            c.borrow_mut()
                .take()
                .is_some_and(|s| s.cleanup_error.is_some() || s.prepared || s.dispatched)
        });
        self.end(failed);
    }
}
pub fn rejected(code: &str, message: &str, fallback: bool) -> Value {
    json!({"ok":false,"dispatch":"not_dispatched","fallback_allowed":fallback,"error":{"code":code,"message":message}})
}
fn outcome(
    prepared: bool,
    dispatched: bool,
    error: Option<String>,
    cleanup_error: Option<String>,
    unavailable: bool,
) -> Value {
    let failed = error.is_some() || cleanup_error.is_some();
    let dispatch = if failed && (prepared || dispatched) {
        "unknown"
    } else if dispatched {
        "dispatched"
    } else {
        "not_dispatched"
    };
    let mut output = json!({"ok":!failed,"dispatch":dispatch,"fallback_allowed":failed&&unavailable&&!prepared&&!dispatched&&cleanup_error.is_none()});
    if let Some(message) = error {
        output["error"] = json!({"code":if unavailable{"background_unavailable"}else{"background_input_failed"},"message":message});
    }
    if let Some(message) = cleanup_error {
        if output.get("error").is_none() {
            output["error"] = json!({"code":"cleanup_failed","message":message});
        }
        output["cleanup_error"] = json!({"code":"cleanup_failed","message":message});
    }
    output
}
pub fn fail(message: String) {
    CALL.with(|c| {
        if let Some(s) = c.borrow_mut().as_mut() {
            s.error.get_or_insert(message);
        }
    });
}
pub fn cleanup_failure(message: String) {
    CALL.with(|c| {
        if let Some(s) = c.borrow_mut().as_mut() {
            s.cleanup_error.get_or_insert(message);
        }
    });
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
enum Validation {
    Target = 0,
    CleanupTarget = 1,
    InputOwner = 2,
}
#[derive(Clone, Copy)]
struct Gate {
    validation: Validation,
    cleanup_deadline: Option<Instant>,
}
impl Gate {
    fn record_error(&self, message: String) {
        if self.validation == Validation::Target {
            fail(message);
        } else {
            cleanup_failure(message);
        }
    }
    /// No AX, bridge callback, or native event construction. Recheck after any
    /// blocking validation and immediately before the admitted post.
    fn local(&self) -> anyhow::Result<()> {
        let (deadline, previous) = CALL
            .with(|call| {
                call.borrow().as_ref().map(|state| {
                    (
                        state.deadline,
                        if self.validation == Validation::Target {
                            // A failed paired release also closes admission for
                            // new input; only remaining owned cleanup may run.
                            state.error.clone().or_else(|| state.cleanup_error.clone())
                        } else {
                            state.cleanup_error.clone()
                        },
                    )
                })
            })
            .ok_or_else(|| anyhow::anyhow!("input call has no owner"))?;
        let result = (|| {
            anyhow::ensure!(previous.is_none(), "{}", previous.unwrap_or_default());
            if self.validation == Validation::Target {
                anyhow::ensure!(
                    !LIFECYCLE.lock().unwrap_or_else(|p| p.into_inner()).closed,
                    "input owner is stopping"
                );
                anyhow::ensure!(now_ms() < deadline, "input deadline elapsed");
            } else {
                anyhow::ensure!(
                    self.cleanup_deadline
                        .is_some_and(|until| Instant::now() < until),
                    "input cleanup timed out"
                );
            }
            Ok(())
        })();
        if let Err(error) = &result {
            self.record_error(error.to_string());
        }
        result
    }
    fn validate(&self) -> anyhow::Result<()> {
        self.local()?;
        let (context, validate) = CALL
            .with(|call| {
                call.borrow()
                    .as_ref()
                    .map(|state| (state.context, state.validate))
            })
            .ok_or_else(|| anyhow::anyhow!("input call has no owner"))?;
        let result = match validate(context, self.validation as i32) {
            1 => Ok(()),
            0 => Err(anyhow::anyhow!(
                "input target changed; refresh app/window state"
            )),
            _ => Err(anyhow::anyhow!("input cancelled or validation unavailable")),
        };
        if let Err(error) = &result {
            self.record_error(error.to_string());
        }
        result?;
        // The callback can consume the remaining deadline or race shutdown.
        self.local()
    }
}
fn target_gate() -> anyhow::Result<Gate> {
    CALL.with(|call| {
        call.borrow().as_ref().map(|state| Gate {
            validation: if state.cleanup {
                Validation::CleanupTarget
            } else {
                Validation::Target
            },
            cleanup_deadline: state.cleanup_deadline,
        })
    })
    .ok_or_else(|| anyhow::anyhow!("input call has no owner"))
}
fn owner_gate() -> anyhow::Result<Gate> {
    CALL.with(|call| {
        call.borrow().as_ref().map(|state| Gate {
            validation: Validation::InputOwner,
            // Normal paired KeyUp gets its own bounded release allowance. It must
            // not start the terminal cleanup timer on the first character of a
            // long typeText. Once terminal cleanup starts, all releases/restoration
            // share its already-established deadline.
            cleanup_deadline: Some(
                state
                    .cleanup_deadline
                    .unwrap_or_else(|| Instant::now() + Duration::from_secs(2)),
            ),
        })
    })
    .ok_or_else(|| anyhow::anyhow!("input call has no owner"))
}
pub fn check() -> anyhow::Result<()> {
    target_gate()?.validate()
}
/// A completed post may legitimately open a modal or close its window. Keep
/// cancellation, expiry and the original error, without admitting another input
/// or requiring the pre-input surface to remain operable after its effect.
pub fn check_completed() -> anyhow::Result<()> {
    target_gate()?.local()
}
pub fn check_input_owner() -> anyhow::Result<()> {
    owner_gate()?.validate()
}
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
pub fn sleep(duration: Duration) {
    let until = Instant::now() + duration;
    while Instant::now() < until {
        // Waiting has no native side effect. Revalidate the target immediately
        // before the next post, not by repeatedly traversing AX during pacing.
        let (cleanup, stop) = CALL.with(|call| {
            call.borrow().as_ref().map_or((false, true), |state| {
                let stop = if state.cleanup {
                    state.cleanup_error.is_some()
                        || state
                            .cleanup_deadline
                            .is_some_and(|deadline| Instant::now() >= deadline)
                } else {
                    state.error.is_some()
                        || now_ms() >= state.deadline
                        || LIFECYCLE.lock().unwrap_or_else(|p| p.into_inner()).closed
                };
                (state.cleanup, stop)
            })
        });
        if stop {
            if cleanup {
                cleanup_failure("input cleanup stopped during pacing".into());
            } else {
                fail("input stopped during pacing".into());
            }
            return;
        }
        std::thread::sleep(
            until
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(10)),
        );
    }
}
pub fn before_preparation() -> bool {
    if check().is_err() {
        return false;
    }
    CALL.with(|c| {
        if let Some(s) = c.borrow_mut().as_mut() {
            s.prepared = true;
        }
    });
    true
}
pub fn prepared() -> bool {
    CALL.with(|c| c.borrow().as_ref().is_some_and(|s| s.prepared))
}
pub fn with_cleanup<T>(body: impl FnOnce() -> T) -> T {
    let previous = CALL.with(|c| {
        let mut c = c.borrow_mut();
        let s = c.as_mut().expect("input owner");
        let prev = s.cleanup;
        s.cleanup = true;
        s.cleanup_deadline
            .get_or_insert(Instant::now() + Duration::from_secs(2));
        prev
    });
    struct Reset(bool);
    impl Drop for Reset {
        fn drop(&mut self) {
            CALL.with(|c| {
                if let Some(s) = c.borrow_mut().as_mut() {
                    s.cleanup = self.0;
                }
            })
        }
    }
    let _reset = Reset(previous);
    body()
}
fn pair(event: &CGEvent) -> Option<(u32, CGEventType, bool)> {
    match event.get_type() {
        CGEventType::KeyDown => Some((
            event.get_integer_value_field(9) as u32,
            CGEventType::KeyUp,
            true,
        )),
        CGEventType::KeyUp => Some((
            event.get_integer_value_field(9) as u32,
            CGEventType::KeyUp,
            false,
        )),
        CGEventType::LeftMouseDown => Some((0x10000, CGEventType::LeftMouseUp, true)),
        CGEventType::LeftMouseUp => Some((0x10000, CGEventType::LeftMouseUp, false)),
        CGEventType::RightMouseDown => Some((0x10001, CGEventType::RightMouseUp, true)),
        CGEventType::RightMouseUp => Some((0x10001, CGEventType::RightMouseUp, false)),
        _ => None,
    }
}
fn key_window_allows(
    event: CGEventType,
    target: Option<u32>,
    matches: impl FnOnce(u32) -> bool,
) -> bool {
    !matches!(event, CGEventType::KeyDown) || target.is_none_or(matches)
}

extern "C" {
    fn CGEventCreateCopy(event: *mut c_void) -> *mut c_void;
}
pub fn post_event(pid: i32, event: &CGEvent, auth: bool) {
    post_event_via(pid, event, auth, false)
}
fn is_release(event: CGEventType) -> bool {
    matches!(
        event,
        CGEventType::KeyUp | CGEventType::LeftMouseUp | CGEventType::RightMouseUp
    )
}
fn input_event_gate(event: CGEventType, paired_release: bool) -> anyhow::Result<Gate> {
    if !is_release(event) {
        return target_gate();
    }
    if !paired_release {
        let message = "input release has no admitted down pair";
        fail(message.into());
        anyhow::bail!(message);
    }
    // The down's pending entry, delivery transport and original application
    // identity authorize only this release, even if that down opened a modal.
    owner_gate()
}
pub fn post_event_via(pid: i32, event: &CGEvent, auth: bool, public: bool) {
    let paired = pair(event);
    let release_transport = if is_release(event.get_type()) {
        paired.and_then(|(id, _, _)| {
            CALL.with(|call| {
                call.borrow().as_ref().and_then(|state| {
                    state
                        .pending
                        .iter()
                        .rev()
                        .find(|pending| {
                            pending.id == id
                                && pending.keyboard
                                    == matches!(event.get_type(), CGEventType::KeyUp)
                        })
                        .map(|pending| (pending.auth, pending.public))
                })
            })
        })
    } else {
        None
    };
    let gate = input_event_gate(event.get_type(), release_transport.is_some());
    let Ok(gate) = gate else { return };
    if gate.validate().is_err() {
        return;
    }
    let (auth, public) = release_transport.unwrap_or((auth, public));
    // The same app can change key windows without changing target identity or bounds.
    // Arm only after priming; paired releases deliberately ignore current focus.
    let target = CALL.with(|c| c.borrow().as_ref().and_then(|s| s.key_window));
    if !key_window_allows(event.get_type(), target, |wid| {
        super::skylight::key_window_matches(pid, wid)
    }) {
        fail("app key window changed during input; refresh app/window state".into());
        return;
    }
    let release = if let Some((id, up, true)) = paired {
        let raw = unsafe { CGEventCreateCopy(event.as_ptr().cast()) };
        if raw.is_null() {
            fail("could not reserve paired input release".into());
            return;
        }
        let release = unsafe { CGEvent::from_ptr(raw.cast()) };
        release.set_type(up);
        release.set_flags(CGEventFlags::CGEventFlagNull);
        if id >= 0x10000 {
            let phase = if event.get_integer_value_field(0) == 1 {
                2
            } else {
                3
            };
            super::skylight::set_integer_field(release.as_ptr().cast(), 0, phase);
        }
        Some(Pending {
            id,
            event: release,
            auth,
            public,
            keyboard: matches!(event.get_type(), CGEventType::KeyDown),
        })
    } else {
        None
    };
    // The exact-window AX read and event allocation above may have blocked.
    if matches!(event.get_type(), CGEventType::ScrollWheel) {
        if let Err(error) = super::super::scroll_guard::check() {
            gate.record_error(
                error["message"]
                    .as_str()
                    .unwrap_or("scroll anchor proof failed")
                    .to_owned(),
            );
            return;
        }
        // The resident AX proof was read-only and outside input admission.
        // Recheck the normal window/bounds/cancel contract after that IPC wait.
        if gate.validate().is_err() {
            return;
        }
    }
    // Do not register a release or send a new down after local admission closes.
    if gate.local().is_err() {
        return;
    }
    if let Some(release) = release {
        CALL.with(|call| call.borrow_mut().as_mut().unwrap().pending.push(release));
    }
    CALL.with(|c| c.borrow_mut().as_mut().unwrap().dispatched = true);
    if !send_event(pid, event, auth, public) {
        gate.record_error("targeted event dispatch could not be confirmed".into());
        return;
    }
    if matches!(event.get_type(), CGEventType::ScrollWheel) {
        super::super::scroll_guard::posted();
    }
    use super::super::overlay::PointerKind;
    let kind = match event.get_type() {
        CGEventType::LeftMouseDown | CGEventType::RightMouseDown | CGEventType::OtherMouseDown => {
            Some(PointerKind::Press)
        }
        CGEventType::LeftMouseUp | CGEventType::RightMouseUp | CGEventType::OtherMouseUp => {
            Some(PointerKind::Release)
        }
        CGEventType::MouseMoved => Some(PointerKind::Move),
        CGEventType::ScrollWheel => Some(PointerKind::Scroll),
        _ => None,
    };
    if let Some(kind) = kind {
        let point = event.location();
        super::super::pointer_presentation::posted([point.x, point.y], kind);
    }
    if let Some((id, _, false)) = paired {
        CALL.with(|c| {
            let mut c = c.borrow_mut();
            let pending = &mut c.as_mut().unwrap().pending;
            if let Some(index) = pending.iter().rposition(|p| p.id == id) {
                pending.remove(index);
            }
        });
    }
}
pub fn cleanup_pending() {
    let Some((pid, pending)) = CALL.with(|c| {
        c.borrow_mut()
            .as_mut()
            .map(|s| (s.pid, std::mem::take(&mut s.pending)))
    }) else {
        return;
    };
    if pending.is_empty() {
        return;
    }
    with_cleanup(|| {
        for pending in pending.into_iter().rev() {
            // Every entry was reserved before its admitted down. Releasing it
            // cannot require the old window to survive the down's effect.
            let gate = owner_gate();
            let Ok(gate) = gate else { break };
            if gate.validate().is_err() {
                cleanup_failure(
                    "paired release skipped because its original owner could not be validated"
                        .into(),
                );
                break;
            }
            if !send_event(pid, &pending.event, pending.auth, pending.public) {
                cleanup_failure("paired input release failed".into());
                break;
            }
        }
    });
}
pub fn quiesce(timeout_ms: u32) -> bool {
    let until = Instant::now() + Duration::from_millis(timeout_ms.into());
    let mut state = LIFECYCLE.lock().unwrap_or_else(|p| p.into_inner());
    state.closed = true;
    while state.active {
        let Some(remaining) = until.checked_duration_since(Instant::now()) else {
            return false;
        };
        let (next, result) = FINISHED
            .wait_timeout(state, remaining)
            .unwrap_or_else(|p| p.into_inner());
        state = next;
        if result.timed_out() && state.active {
            return false;
        }
    }
    !state.cleanup_failed
}
#[cfg(test)]
mod tests {
    use super::*;
    static OWNER_TEST: Mutex<()> = Mutex::new(());
    #[test]
    fn preparation_is_not_safe_to_replay() {
        let v = outcome(true, false, Some("failed".into()), None, true);
        assert_eq!(v["dispatch"], "unknown");
        assert_eq!(v["fallback_allowed"], false);
    }
    #[test]
    fn only_preflight_unavailable_allows_fallback() {
        let v = outcome(false, false, Some("unavailable".into()), None, true);
        assert_eq!(v["fallback_allowed"], true);
        let v = outcome(false, false, Some("cancelled".into()), None, false);
        assert_eq!(v["fallback_allowed"], false);
    }
    #[test]
    fn cleanup_failure_is_visible_and_never_replayed() {
        let v = outcome(true, true, None, Some("release failed".into()), true);
        assert_eq!(v["ok"], false);
        assert_eq!(v["dispatch"], "unknown");
        assert_eq!(v["fallback_allowed"], false);
        assert!(v["cleanup_error"].is_object());
        assert_eq!(v["error"]["code"], "cleanup_failed");
    }
    #[test]
    fn quiesce_waits_for_the_real_owner_to_finish() {
        let _owner = OWNER_TEST.lock().unwrap();
        extern "C" fn valid(_: *mut c_void, _: i32) -> i32 {
            1
        }
        let session =
            Session::start(7, now_ms() + 10_000, std::ptr::null_mut(), valid, false).unwrap();
        assert!(!std::thread::spawn(|| quiesce(1)).join().unwrap());
        assert!(check().is_err());
        let result = session.finish(Ok(()), false);
        assert_eq!(result["ok"], false);
        assert_eq!(result["fallback_allowed"], false);
        assert!(quiesce(0));
        // Only this pure lifecycle test admits a session. Restore isolated test state.
        *LIFECYCLE.lock().unwrap() = Lifecycle::default();
    }
    #[test]
    fn deadline_expiring_inside_validator_blocks_admission() {
        let _owner = OWNER_TEST.lock().unwrap();
        extern "C" fn expires(_: *mut c_void, phase: i32) -> i32 {
            assert_eq!(phase, 0);
            CALL.with(|call| call.borrow_mut().as_mut().unwrap().deadline = 0);
            1
        }
        let session =
            Session::start(7, now_ms() + 10_000, std::ptr::null_mut(), expires, false).unwrap();
        assert!(check().is_err());
        let result = session.finish(Ok(()), false);
        assert_eq!(result["dispatch"], "not_dispatched");
        assert_eq!(result["fallback_allowed"], false);
    }
    #[test]
    fn only_admitted_up_uses_the_app_release_gate() {
        let _owner = OWNER_TEST.lock().unwrap();
        extern "C" fn valid(_: *mut c_void, _: i32) -> i32 {
            1
        }
        for up in [
            CGEventType::KeyUp,
            CGEventType::LeftMouseUp,
            CGEventType::RightMouseUp,
        ] {
            let session =
                Session::start(7, now_ms() + 10_000, std::ptr::null_mut(), valid, false).unwrap();
            assert_eq!(
                input_event_gate(up, true).unwrap().validation,
                Validation::InputOwner
            );
            assert!(input_event_gate(up, false).is_err());
            let result = session.finish(Ok(()), false);
            assert_eq!(result["dispatch"], "not_dispatched");
            assert_eq!(result["fallback_allowed"], false);
        }
        let session =
            Session::start(7, now_ms() + 10_000, std::ptr::null_mut(), valid, false).unwrap();
        for new_input in [
            CGEventType::KeyDown,
            CGEventType::LeftMouseDown,
            CGEventType::RightMouseDown,
            CGEventType::MouseMoved,
            CGEventType::ScrollWheel,
        ] {
            assert_eq!(
                input_event_gate(new_input, true).unwrap().validation,
                Validation::Target
            );
        }
        assert_eq!(session.finish(Ok(()), false)["ok"], true);
    }
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum SurfaceState {
        Available,
        ModalOpened,
        Closed,
    }
    struct EffectContext {
        surface: std::cell::Cell<SurfaceState>,
        owner_valid: std::cell::Cell<bool>,
        phases: RefCell<Vec<i32>>,
    }
    extern "C" fn effect_validation(context: *mut c_void, phase: i32) -> i32 {
        let state = unsafe { &*context.cast::<EffectContext>() };
        state.phases.borrow_mut().push(phase);
        i32::from(
            state.owner_valid.get()
                && (phase == 2 || state.surface.get() == SurfaceState::Available),
        )
    }
    #[test]
    fn modal_or_closed_target_does_not_fail_completed_mouse_or_key_input() {
        let _owner = OWNER_TEST.lock().unwrap();
        for effect in [SurfaceState::ModalOpened, SurfaceState::Closed] {
            for (down, up) in [
                (CGEventType::LeftMouseDown, CGEventType::LeftMouseUp),
                (CGEventType::RightMouseDown, CGEventType::RightMouseUp),
                (CGEventType::KeyDown, CGEventType::KeyUp),
            ] {
                let mut state = EffectContext {
                    surface: std::cell::Cell::new(SurfaceState::Available),
                    owner_valid: std::cell::Cell::new(true),
                    phases: RefCell::default(),
                };
                let context = (&mut state as *mut EffectContext).cast();
                let session =
                    Session::start(7, now_ms() + 10_000, context, effect_validation, false)
                        .unwrap();
                input_event_gate(down, false).unwrap().validate().unwrap();
                // Model the admitted native post, then its legitimate app effect.
                CALL.with(|call| call.borrow_mut().as_mut().unwrap().dispatched = true);
                state.surface.set(effect);
                input_event_gate(up, true).unwrap().validate().unwrap();
                check_completed().unwrap();
                assert_eq!(*state.phases.borrow(), [0, 2]);
                let result = session.finish(Ok(()), false);
                assert_eq!(result["dispatch"], "dispatched");
                assert_eq!(result["ok"], true);
                assert!(result.get("cleanup_error").is_none());

                // Completion cannot authorize another down against the old surface.
                let next = Session::start(7, now_ms() + 10_000, context, effect_validation, false)
                    .unwrap();
                assert!(input_event_gate(down, false).unwrap().validate().is_err());
                assert_eq!(*state.phases.borrow(), [0, 2, 0]);
                let rejected = next.finish(Ok(()), false);
                assert_eq!(rejected["dispatch"], "not_dispatched");
                assert_eq!(rejected["fallback_allowed"], false);
            }
        }
    }
    #[test]
    fn paired_mouse_release_rejects_a_changed_owner_without_window_fallback() {
        let _owner = OWNER_TEST.lock().unwrap();
        let mut state = EffectContext {
            surface: std::cell::Cell::new(SurfaceState::Available),
            owner_valid: std::cell::Cell::new(true),
            phases: RefCell::default(),
        };
        let session = Session::start(
            7,
            now_ms() + 10_000,
            (&mut state as *mut EffectContext).cast(),
            effect_validation,
            false,
        )
        .unwrap();
        input_event_gate(CGEventType::LeftMouseDown, false)
            .unwrap()
            .validate()
            .unwrap();
        CALL.with(|call| call.borrow_mut().as_mut().unwrap().dispatched = true);
        state.surface.set(SurfaceState::Closed);
        state.owner_valid.set(false);
        assert!(input_event_gate(CGEventType::LeftMouseUp, true)
            .unwrap()
            .validate()
            .is_err());
        assert!(check_completed().is_err());
        assert!(input_event_gate(CGEventType::LeftMouseDown, false)
            .unwrap()
            .validate()
            .is_err());
        assert_eq!(*state.phases.borrow(), [0, 2]);
        let result = session.finish(Ok(()), false);
        assert_eq!(result["dispatch"], "unknown");
        assert_eq!(result["fallback_allowed"], false);
        assert!(result["cleanup_error"].is_object());
        *LIFECYCLE.lock().unwrap() = Lifecycle::default();
    }
    #[test]
    fn completion_keeps_original_error_expiry_and_cancellation() {
        let _owner = OWNER_TEST.lock().unwrap();
        extern "C" fn never_validate(_: *mut c_void, _: i32) -> i32 {
            panic!("Completion cannot traverse the old target")
        }
        for reason in ["original", "cleanup", "expired", "cancelled"] {
            let session = Session::start(
                7,
                now_ms() + 10_000,
                std::ptr::null_mut(),
                never_validate,
                false,
            )
            .unwrap();
            CALL.with(|call| call.borrow_mut().as_mut().unwrap().dispatched = true);
            match reason {
                "original" => fail("original dispatch failure".into()),
                "cleanup" => cleanup_failure("original paired release failure".into()),
                "expired" => CALL.with(|call| call.borrow_mut().as_mut().unwrap().deadline = 0),
                _ => LIFECYCLE.lock().unwrap().closed = true,
            }
            assert!(check_completed().is_err());
            let result = session.finish(Ok(()), false);
            assert_eq!(result["dispatch"], "unknown");
            assert_eq!(result["fallback_allowed"], false);
            if reason == "original" {
                assert_eq!(result["error"]["message"], "original dispatch failure");
            }
            if reason == "cleanup" {
                assert_eq!(
                    result["cleanup_error"]["message"],
                    "original paired release failure"
                );
            }
            *LIFECYCLE.lock().unwrap() = Lifecycle::default();
        }
    }
    #[test]
    fn window_loss_blocks_new_input_but_app_release_remains_bounded() {
        let _owner = OWNER_TEST.lock().unwrap();
        extern "C" fn closed_window(_: *mut c_void, phase: i32) -> i32 {
            i32::from(phase == 2)
        }
        let session = Session::start(
            7,
            now_ms() + 10_000,
            std::ptr::null_mut(),
            closed_window,
            false,
        )
        .unwrap();
        assert!(check().is_err());
        assert!(check_input_owner().is_ok());
        assert!(CALL.with(|call| call.borrow().as_ref().unwrap().cleanup_deadline.is_none()));
        let gate = Gate {
            validation: Validation::InputOwner,
            cleanup_deadline: Some(Instant::now()),
        };
        assert!(gate.validate().is_err());
        let result = session.finish(Ok(()), false);
        assert!(result["cleanup_error"].is_object());
        *LIFECYCLE.lock().unwrap() = Lifecycle::default();
    }
    #[test]
    fn sibling_switch_blocks_new_down_but_not_release() {
        let key_window = std::cell::Cell::new(7);
        assert!(key_window_allows(
            CGEventType::KeyDown,
            Some(7),
            |target| key_window.get() == target
        ));
        key_window.set(8);
        assert!(!key_window_allows(
            CGEventType::KeyDown,
            Some(7),
            |target| key_window.get() == target
        ));
        assert!(key_window_allows(CGEventType::KeyUp, Some(7), |_| panic!(
            "release must ignore current key window"
        )));
        assert!(key_window_allows(CGEventType::KeyDown, None, |_| panic!(
            "initial prime is not yet armed"
        )));
    }
}

pub fn preparation_failure(message: String) {
    let cleanup = CALL.with(|c| c.borrow().as_ref().is_some_and(|s| s.cleanup));
    if cleanup {
        cleanup_failure(message)
    } else {
        fail(message)
    }
}

pub fn arm_keyboard(window_id: u32) {
    CALL.with(|c| {
        if let Some(s) = c.borrow_mut().as_mut() {
            s.key_window = Some(window_id);
        }
    })
}

fn send_event(pid: i32, event: &CGEvent, auth: bool, public: bool) -> bool {
    if public {
        event.post_to_pid(pid);
        true
    } else {
        super::skylight::post_to_pid(pid, event.as_ptr().cast(), auth)
    }
}
pub fn pointer_uses_public() -> bool {
    CALL.with(|c| c.borrow().as_ref().is_some_and(|s| s.pointer_public))
}
