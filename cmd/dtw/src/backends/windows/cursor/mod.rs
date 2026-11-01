//! Optional session-scoped visual feedback. Never an input actuator.
//! Win32 window flags, target-relative ordering and layered DIB presentation
//! follow CUA b3836044 platform-windows/src/overlay.rs; see the port record.
mod identity;
mod owner;
mod raster;
mod renderer;
pub use owner::CursorSession;
pub(super) use owner::Emitter;
pub(super) use renderer::worker_entry;

use super::native;
use crate::backends::native_client::{parse_target, target_json};
use crate::computer::WindowTarget;
use serde_json::{json, Value};
use windows::Win32::{
    Foundation::HWND,
    UI::{
        HiDpi::GetDpiForWindow, Input::KeyboardAndMouse::IsWindowEnabled, WindowsAndMessaging::*,
    },
};

const CLASS: &str = "DesktopWorld.SessionCursor.v1";
const MAX_PACKET: usize = 4096;
const IDLE_HIDE_AFTER: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq)]
struct Target {
    window: WindowTarget,
    bounds: [i32; 4],
    dpi: u32,
    displays: Vec<[i32; 4]>,
}
impl Target {
    fn read(window: WindowTarget) -> Option<Self> {
        let hwnd = HWND(window.native_id as usize as *mut _);
        let bounds = native::rect(hwnd).ok()?;
        let target = Self {
            window,
            bounds: [bounds.left, bounds.top, bounds.right, bounds.bottom],
            dpi: unsafe { GetDpiForWindow(hwnd) },
            displays: renderer::displays()?,
        };
        target.valid().then_some(target)
    }
    fn hwnd(&self) -> HWND {
        HWND(self.window.native_id as usize as *mut _)
    }
    fn valid(&self) -> bool {
        let h = self.hwnd();
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        let owner = native::process_stamp(pid).ok();
        pid == self.window.app.pid
            && owner.as_ref() == Some(&self.window.app)
            && native::marker(h, false).ok() == Some(self.window.generation)
            && native::visible(h)
            && unsafe { IsWindowEnabled(h) }.as_bool()
            && unsafe { GetAncestor(h, GA_ROOT) } == h
            && unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) } & WS_EX_TOPMOST.0 as isize == 0
            && native::rect(h)
                .ok()
                .is_some_and(|r| [r.left, r.top, r.right, r.bottom] == self.bounds)
            && self.dpi > 0
            && unsafe { GetDpiForWindow(h) } == self.dpi
            && renderer::displays().as_ref() == Some(&self.displays)
    }
    fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.bounds[0] && y >= self.bounds[1] && x < self.bounds[2] && y < self.bounds[3]
    }
    fn json(&self) -> Value {
        json!({"window":target_json(&self.window),"bounds":self.bounds,
            "dpi":self.dpi,"displays":self.displays})
    }
    fn parse(v: &Value) -> Option<Self> {
        let bounds: Vec<i32> = v["bounds"]
            .as_array()?
            .iter()
            .map(|v| v.as_i64().and_then(|v| i32::try_from(v).ok()))
            .collect::<Option<_>>()?;
        let displays: Vec<[i32; 4]> = v["displays"]
            .as_array()?
            .iter()
            .map(|v| {
                let values = v.as_array()?;
                if values.len() != 4 {
                    return None;
                }
                Some([
                    i32::try_from(values[0].as_i64()?).ok()?,
                    i32::try_from(values[1].as_i64()?).ok()?,
                    i32::try_from(values[2].as_i64()?).ok()?,
                    i32::try_from(values[3].as_i64()?).ok()?,
                ])
            })
            .collect::<Option<_>>()?;
        if bounds.len() != 4 || displays.is_empty() || displays.len() > 64 {
            return None;
        }
        Some(Self {
            window: parse_target(&v["window"]).ok()?,
            bounds: bounds.try_into().ok()?,
            dpi: u32::try_from(v["dpi"].as_u64()?).ok()?,
            displays,
        })
    }
}

pub(super) fn prepare(window: WindowTarget) -> Option<Value> {
    Target::read(window).map(|target| target.json())
}

pub(super) fn proven_overlay(hwnd: HWND) -> bool {
    identity::proven(hwnd)
}
pub(super) fn begin_proof_cycle() {
    identity::begin_cycle();
}

#[derive(Default)]
struct Presentation {
    operation: u64,
    retired: u64,
    sequence: u64,
    press: u64,
    target: Option<Target>,
    point: Option<(i32, i32)>,
    pulse_until: Option<std::time::Instant>,
    last_activity: Option<std::time::Instant>,
}
impl Presentation {
    fn begin(&mut self, operation: u64, target: Target) -> bool {
        if operation <= self.operation || operation <= self.retired || !target.valid() {
            return false;
        }
        self.operation = operation;
        self.sequence = 0;
        self.press = 0;
        self.target = Some(target);
        self.point = None;
        self.pulse_until = None;
        self.last_activity = None;
        true
    }
    fn hide(&mut self, operation: u64) {
        self.retired = self.retired.max(operation);
        if operation >= self.operation {
            self.target = None;
            self.point = None;
            self.pulse_until = None;
            self.last_activity = None;
        }
    }
    fn idle_expired(&self, now: std::time::Instant) -> bool {
        self.point.is_some()
            && self
                .last_activity
                .is_some_and(|last| now.saturating_duration_since(last) >= IDLE_HIDE_AFTER)
    }
    fn expire_idle(&mut self, now: std::time::Instant) -> bool {
        if !self.idle_expired(now) {
            return false;
        }
        // Retire the operation as well as hiding it: late packets and the idle
        // z-order maintenance must not bring an old pointer presentation back.
        self.hide(self.operation);
        true
    }
    fn event(&mut self, value: &Value, now: std::time::Instant) -> bool {
        // A delayed packet cannot extend an already elapsed lifetime even if
        // the renderer has not reached its next idle tick yet.
        if self.idle_expired(now) {
            return false;
        }
        let (Some(operation), Some(sequence), Some(press), Some(x), Some(y)) = (
            value["operation"].as_u64(),
            value["sequence"].as_u64(),
            value["press"].as_u64(),
            value["x"].as_i64().and_then(|x| i32::try_from(x).ok()),
            value["y"].as_i64().and_then(|y| i32::try_from(y).ok()),
        ) else {
            return false;
        };
        if operation != self.operation || operation <= self.retired || sequence <= self.sequence {
            return false;
        }
        let Some(target) = self.target.as_ref() else {
            return false;
        };
        // Native identity/geometry is re-proven once before the coalesced paint
        // and on idle ticks, rather than once for every queued mouse packet.
        if !target.contains(x, y) {
            self.hide(operation);
            return false;
        }
        if press > sequence {
            return false;
        }
        self.sequence = sequence;
        self.point = Some((x, y));
        self.last_activity = Some(now);
        if press > self.press {
            self.press = press;
            self.pulse_until = Some(now + std::time::Duration::from_millis(200));
        }
        true
    }
}

fn diagnostic(context: &str, error: impl std::fmt::Display) {
    use std::io::Write;
    let _ = writeln!(
        std::io::stderr().lock(),
        "dtw Windows cursor {context}: {error}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computer::{AppIdentity, AppInstance};
    fn state() -> Presentation {
        Presentation {
            operation: 2,
            target: Some(Target {
                window: WindowTarget {
                    app: AppInstance {
                        identity: AppIdentity("not-a-live-test-target".into()),
                        pid: 1,
                        generation: 1,
                    },
                    native_id: 1,
                    generation: 1,
                },
                bounds: [-100, -100, 100, 100],
                dpi: 120,
                displays: vec![[-100, -100, 100, 100]],
            }),
            ..Default::default()
        }
    }
    fn event(op: u64, sequence: u64, press: u64, x: i32) -> Value {
        json!({"operation":op,"sequence":sequence,"press":press,"x":x,"y":-10})
    }
    #[test]
    fn delayed_old_packets_and_hides_cannot_revive_or_hide_new_operation() {
        let mut s = state();
        let now = std::time::Instant::now();
        assert!(s.event(&event(2, 1, 0, -20), now));
        s.hide(1);
        assert!(!s.event(&event(1, 99, 99, -20), now));
        assert_eq!(s.point, Some((-20, -10)));
        s.hide(2);
        assert!(!s.event(&event(2, 2, 2, -20), now));
        assert_eq!(s.point, None);
    }
    #[test]
    fn coalesced_release_keeps_one_bounded_pulse_and_bad_sequences_fail() {
        let mut s = state();
        let now = std::time::Instant::now();
        assert!(s.event(&event(2, 1, 1, -20), now));
        let expiry = s.pulse_until;
        assert!(s.event(
            &event(2, 3, 1, -10),
            now + std::time::Duration::from_millis(100)
        ));
        assert_eq!(s.pulse_until, expiry);
        assert!(!s.event(&event(2, 2, 2, -20), now));
        assert!(!s.event(&event(2, 4, 5, -20), now));
        assert_eq!(s.sequence, 3);
        assert!(!s.event(&event(2, 4, 1, 100), now));
        assert_eq!(s.point, None);
    }
    #[test]
    fn bootstrap_geometry_cannot_truncate_or_wrap_native_values() {
        let target = state().target.unwrap();
        assert_eq!(Target::parse(&target.json()), Some(target.clone()));
        let mut v = target.json();
        v["bounds"][0] = json!(i64::MAX);
        assert!(Target::parse(&v).is_none());
        let mut v = target.json();
        v["dpi"] = json!(u64::MAX);
        assert!(Target::parse(&v).is_none());
        assert!(identity::Identity::parse(&json!({"pid":0,"birth":1,"hwnd":1})).is_none());
    }
    #[test]
    fn idle_expiry_hides_and_retires_exactly_at_two_seconds() {
        let mut s = state();
        let now = std::time::Instant::now();
        assert!(!s.expire_idle(now + IDLE_HIDE_AFTER));
        assert!(s.event(&event(2, 1, 1, -20), now));
        assert!(!s.expire_idle(now + IDLE_HIDE_AFTER - std::time::Duration::from_nanos(1)));
        assert!(s.expire_idle(now + IDLE_HIDE_AFTER));
        assert_eq!(s.retired, 2);
        assert!(
            s.point.is_none()
                && s.target.is_none()
                && s.last_activity.is_none()
                && s.pulse_until.is_none()
        );
        assert!(!s.expire_idle(now + IDLE_HIDE_AFTER));
        assert!(!s.event(&event(2, 99, 99, -20), now + IDLE_HIDE_AFTER));
    }
    #[test]
    fn only_accepted_pointer_activity_extends_idle_lifetime() {
        let mut s = state();
        let now = std::time::Instant::now();
        assert!(s.event(&event(2, 1, 0, -20), now));
        assert!(!s.event(
            &event(1, 90, 0, -20),
            now + std::time::Duration::from_secs(1)
        ));
        assert!(!s.event(
            &event(2, 1, 0, -20),
            now + std::time::Duration::from_secs(1)
        ));
        assert!(!s.event(
            &event(2, 2, 3, -20),
            now + std::time::Duration::from_secs(1)
        ));
        assert_eq!(s.last_activity, Some(now));
        let refreshed = now + std::time::Duration::from_millis(1500);
        assert!(s.event(&event(2, 2, 0, -10), refreshed));
        assert!(!s.expire_idle(now + IDLE_HIDE_AFTER));
        assert!(s.expire_idle(refreshed + IDLE_HIDE_AFTER));
    }
    #[test]
    fn late_packet_before_renderer_tick_cannot_refresh_an_expired_presentation() {
        let mut s = state();
        let now = std::time::Instant::now();
        assert!(s.event(&event(2, 1, 0, -20), now));
        assert!(!s.event(&event(2, 2, 0, -10), now + IDLE_HIDE_AFTER));
        assert_eq!(s.last_activity, Some(now));
        assert!(s.expire_idle(now + IDLE_HIDE_AFTER));
    }
    #[test]
    fn idle_clocks_are_independent_and_retirement_does_not_reset_session_ordinals() {
        let (mut first, mut second) = (state(), state());
        let now = std::time::Instant::now();
        assert!(first.event(&event(2, 1, 0, -20), now));
        assert!(second.event(
            &event(2, 1, 0, -20),
            now + std::time::Duration::from_secs(1)
        ));
        assert!(first.expire_idle(now + IDLE_HIDE_AFTER));
        assert!(!second.expire_idle(now + IDLE_HIDE_AFTER));
        assert_eq!((first.operation, first.retired), (2, 2));
        assert_eq!((second.operation, second.retired), (2, 0));
        assert!(!second.expire_idle(now)); // A backwards test clock cannot expire activity.
    }
}
