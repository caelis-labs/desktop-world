//! Actual pointer-event feedback for the current native action; never input.
use super::overlay::{OverlayEmitter, OverlayEndpoint, OverlayTarget, PointerKind, PointerRoute};
use serde_json::Value;
use std::cell::{Cell, RefCell};

struct Presentation {
    emitter: OverlayEmitter,
    target: OverlayTarget,
    route: PointerRoute,
}
thread_local! {
    static ACTIVE: RefCell<Option<Presentation>> = const { RefCell::new(None) };
    static SUPPRESSED: Cell<bool> = const { Cell::new(false) };
}

pub(super) struct Scope(Option<Presentation>);
impl Scope {
    pub(super) fn new(request: &Value, bounds: &Value, route: PointerRoute) -> Self {
        let presentation = (|| {
            let endpoint: OverlayEndpoint =
                serde_json::from_value(request["_cursor_endpoint"].clone()).ok()?;
            let target = OverlayTarget {
                pid: u32::try_from(request["target"]["app"]["pid"].as_u64()?).ok()?,
                birth: request["target"]["app"]["generation"].as_u64()?,
                window: u32::try_from(request["target"]["native_id"].as_u64()?).ok()?,
                bounds: serde_json::from_value(bounds.clone()).ok()?,
            };
            Some(Presentation {
                emitter: OverlayEmitter::connect(endpoint).ok()?,
                target,
                route,
            })
        })();
        Self(ACTIVE.with(|active| active.replace(presentation)))
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.with(|active| {
            active.replace(self.0.take());
        });
    }
}

/// Donor priming and emergency release events are not model pointer actions.
/// Suppress by event purpose, never by a coordinate that might be a real display.
pub(super) fn without_feedback(action: impl FnOnce()) {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            SUPPRESSED.with(|value| value.set(self.0));
        }
    }
    let _restore = Restore(SUPPRESSED.with(|value| value.replace(true)));
    action();
}

pub(super) fn posted(point: [f64; 2], kind: PointerKind) {
    if SUPPRESSED.with(Cell::get) {
        return;
    }
    ACTIVE.with(|active| {
        if let Some(presentation) = active.borrow_mut().as_mut() {
            // Presentation failure must not alter, fail or replay posted input.
            let _ = presentation.emitter.posted(
                presentation.target.clone(),
                point,
                presentation.route,
                kind,
            );
        }
    });
}
pub(super) extern "C" fn native_posted(x: f64, y: f64, kind: i32) {
    let kind = match kind {
        0 => PointerKind::Move,
        1 => PointerKind::Press,
        2 => PointerKind::Release,
        3 => PointerKind::Scroll,
        _ => return,
    };
    posted([x, y], kind);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{os::unix::net::UnixDatagram, time::Duration};

    #[test]
    fn feedback_reports_only_actual_in_scope_posts_and_never_primer_coordinates() {
        let path = format!(
            "/private/tmp/dtw-pointer-test-{}",
            uuid::Uuid::new_v4().simple()
        );
        let receiver = UnixDatagram::bind(&path).unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        let request = json!({"_cursor_endpoint":{"path":path,"token":"owned","operation":9},
            "target":{"app":{"pid":7,"generation":11},"native_id":13}});
        let bounds = json!([-100.0, -100.0, 200.0, 200.0]);
        {
            let _scope = Scope::new(&request, &bounds, PointerRoute::Background);
            without_feedback(|| posted([-1.0, -1.0], PointerKind::Press));
            posted([1000.0, 1000.0], PointerKind::Press); // Invalid target point.
            posted([-1.0, -1.0], PointerKind::Press); // Valid negative display position.
        }
        posted([2.0, 3.0], PointerKind::Release); // No remaining action owner.
        let mut bytes = [0; 4096];
        let size = receiver.recv(&mut bytes).unwrap();
        let event: Value = serde_json::from_slice(&bytes[..size]).unwrap();
        assert_eq!(event["point"], json!([-1.0, -1.0]));
        assert_eq!(event["route"], "background");
        assert_eq!(event["kind"], "press");
        assert_eq!(event["sequence"], 1);
        assert_eq!(event["target"]["window"], 13);
        assert!(receiver.recv(&mut bytes).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
