//! One call owns the complete pointer sequence; no held button escapes it.
use super::input;
use crate::{
    backends::native_client::{check, failed},
    computer::Error,
};
use std::time::{Duration, Instant};
use windows::Win32::{
    Foundation::{HWND, POINT, RECT},
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

const MAX_MOVES: usize = 1024;
const STEP: i64 = 8;
pub struct Plan {
    points: Vec<POINT>,
    drag: bool,
}
impl Plan {
    pub fn points(&self) -> &[POINT] {
        &self.points
    }
}
/// `bounds` is the original window's admitted display bounds, not a union of displays.
pub fn plan(path: &[POINT], drag: bool, bounds: RECT) -> Result<Plan, Error> {
    if path.is_empty()
        || path.len() > MAX_MOVES
        || (!drag && path.len() != 1)
        || (drag && path.len() < 2)
        || bounds.right <= bounds.left
        || bounds.bottom <= bounds.top
        || path.iter().any(|p| {
            p.x < bounds.left || p.x >= bounds.right || p.y < bounds.top || p.y >= bounds.bottom
        })
    {
        return Err(Error::InvalidPoint);
    }
    let mut points = vec![path[0]];
    for pair in path.windows(2) {
        let dx = pair[1].x as i64 - pair[0].x as i64;
        let dy = pair[1].y as i64 - pair[0].y as i64;
        let steps = ((dx.abs().max(dy.abs()) + STEP - 1) / STEP).max(1);
        if points.len() as i64 + steps > MAX_MOVES as i64 {
            return Err(Error::InvalidPoint);
        }
        for step in 1..=steps {
            points.push(POINT {
                x: (pair[0].x as i64 + dx * step / steps) as i32,
                y: (pair[0].y as i64 + dy * step / steps) as i32,
            });
        }
    }
    if drag
        && points
            .iter()
            .all(|p| p.x == points[0].x && p.y == points[0].y)
    {
        return Err(Error::InvalidPoint);
    }
    Ok(Plan { points, drag })
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Event {
    Move(usize),
    Down,
    Up,
}
trait Delivery {
    fn validate(&mut self, index: usize) -> Result<(), Error>;
    fn insert(&mut self, event: Event) -> Result<(), Error>;
    fn failed_cleanup(&mut self);
}
fn execute(plan: &Plan, io: &mut impl Delivery) -> Result<(), Error> {
    let mut held = false;
    let result = (|| {
        io.validate(0)?;
        io.insert(Event::Move(0))?;
        if plan.drag {
            io.validate(0)?;
            io.insert(Event::Down)?;
            held = true;
            for index in 1..plan.points.len() {
                io.validate(index)?;
                io.insert(Event::Move(index))?;
            }
            io.validate(plan.points.len() - 1)?;
        }
        Ok(())
    })();
    // Release bypasses deadline, cancellation, geometry and foreground checks:
    // these may forbid new payload, never cleanup of our own admitted button.
    let cleanup = if held { io.insert(Event::Up) } else { Ok(()) };
    if cleanup.is_err() {
        io.failed_cleanup();
    }
    // Preserve the original cancellation/validation error even if release failed.
    result.and(cleanup)
}

/// Caller owns a single input Lease and must validate the same window, display,
/// geometry and visible receiver at EVERY supplied point (including after focus).
pub fn dispatch(
    plan: &Plan,
    hwnd: HWND,
    until: Instant,
    dispatched: &mut bool,
    accepted: &mut usize,
    validate: impl FnMut(POINT) -> Result<(), Error>,
) -> Result<(), Error> {
    let _mutation = input::mutation(until)?;
    input::no_held_keys()?;
    // Compute every absolute event before admitting any input.
    let (vx, vy, vw, vh) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    if vw <= 1 || vh <= 1 {
        return Err(Error::InvalidPoint);
    }
    let moves = plan
        .points
        .iter()
        .map(|p| {
            let x = p.x as i64 - vx as i64;
            let y = p.y as i64 - vy as i64;
            if x < 0 || x >= vw as i64 || y < 0 || y >= vh as i64 {
                return Err(Error::InvalidPoint);
            }
            Ok(input::mouse(
                MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                0,
                (x * 65535 / (vw - 1) as i64) as i32,
                (y * 65535 / (vh - 1) as i64) as i32,
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    struct Native<'a, F> {
        plan: &'a Plan,
        moves: Vec<INPUT>,
        hwnd: HWND,
        until: Instant,
        dispatched: &'a mut bool,
        accepted: &'a mut usize,
        validate: F,
    }
    impl<F: FnMut(POINT) -> Result<(), Error>> Delivery for Native<'_, F> {
        fn validate(&mut self, index: usize) -> Result<(), Error> {
            check(self.until)?;
            if input::closing() || input::cleanup_failed() {
                return Err(Error::Closed);
            }
            if unsafe { GetForegroundWindow() } != self.hwnd {
                return Err(failed("Pointer foreground changed"));
            }
            (self.validate)(self.plan.points[index])?;
            // Native validation can block. Recheck cancellation immediately before injection.
            check(self.until)?;
            if input::closing() || input::cleanup_failed() {
                return Err(Error::Closed);
            }
            if unsafe { GetForegroundWindow() } != self.hwnd {
                return Err(failed("Pointer foreground changed"));
            }
            Ok(())
        }
        fn insert(&mut self, event: Event) -> Result<(), Error> {
            let input = match event {
                Event::Move(index) => self.moves[index],
                Event::Down => input::mouse(MOUSEEVENTF_LEFTDOWN, 0, 0, 0),
                Event::Up => input::mouse(MOUSEEVENTF_LEFTUP, 0, 0, 0),
            };
            if !matches!(event, Event::Up) {
                super::desktop::ensure_input_desktop()?;
            }
            *self.dispatched = true;
            let count = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
            *self.accepted += count as usize;
            if count != 1 {
                return Err(failed("Pointer insertion incomplete; effects unknown"));
            }
            if matches!(event, Event::Move(_)) {
                std::thread::sleep(Duration::from_millis(2));
            }
            Ok(())
        }
        fn failed_cleanup(&mut self) {
            input::fail_delivery();
        }
    }
    *accepted = 0;
    execute(
        plan,
        &mut Native {
            plan,
            moves,
            hwnd,
            until,
            dispatched,
            accepted,
            validate,
        },
    )?;
    check(until)?;
    if input::closing() {
        return Err(Error::Closed);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake {
        events: Vec<Event>,
        validations: usize,
        stop: Option<usize>,
        fail: Option<Event>,
        dirty: bool,
    }
    impl Delivery for Fake {
        fn validate(&mut self, _: usize) -> Result<(), Error> {
            let n = self.validations;
            self.validations += 1;
            if self.stop == Some(n) {
                Err(Error::Cancelled)
            } else {
                Ok(())
            }
        }
        fn insert(&mut self, event: Event) -> Result<(), Error> {
            self.events.push(event);
            if self.fail == Some(event) {
                Err(Error::InvalidObservation)
            } else {
                Ok(())
            }
        }
        fn failed_cleanup(&mut self) {
            self.dirty = true;
        }
    }
    fn fake() -> Fake {
        Fake {
            events: vec![],
            validations: 0,
            stop: None,
            fail: None,
            dirty: false,
        }
    }
    fn path(drag: bool) -> Plan {
        let points = [POINT { x: 10, y: 10 }, POINT { x: 26, y: 10 }];
        plan(
            &points[..if drag { 2 } else { 1 }],
            drag,
            RECT {
                left: 0,
                top: 0,
                right: 100,
                bottom: 100,
            },
        )
        .unwrap()
    }
    #[test]
    fn hover_never_presses_and_drag_has_exactly_one_pair() {
        let mut io = fake();
        execute(&path(false), &mut io).unwrap();
        assert_eq!(io.events, [Event::Move(0)]);
        let mut io = fake();
        execute(&path(true), &mut io).unwrap();
        assert_eq!(
            io.events,
            [
                Event::Move(0),
                Event::Down,
                Event::Move(1),
                Event::Move(2),
                Event::Up
            ]
        );
    }
    #[test]
    fn every_cancellation_boundary_releases_only_admitted_down() {
        for stop in 0..5 {
            let mut io = fake();
            io.stop = Some(stop);
            assert!(execute(&path(true), &mut io).is_err());
            let held = io.events.contains(&Event::Down);
            assert_eq!(
                io.events.iter().filter(|e| **e == Event::Up).count(),
                usize::from(held)
            );
            if held {
                assert_eq!(io.events.last(), Some(&Event::Up));
            }
        }
    }
    #[test]
    fn failed_payload_is_not_replayed_and_cleanup_preserves_cancellation() {
        let mut io = fake();
        io.fail = Some(Event::Move(1));
        assert!(execute(&path(true), &mut io).is_err());
        assert_eq!(
            io.events,
            [Event::Move(0), Event::Down, Event::Move(1), Event::Up]
        );
        let mut io = fake();
        io.stop = Some(2);
        io.fail = Some(Event::Up);
        assert!(matches!(
            execute(&path(true), &mut io),
            Err(Error::Cancelled)
        ));
        assert!(io.dirty);
        assert_eq!(io.events, [Event::Move(0), Event::Down, Event::Up]);
        let mut io = fake();
        io.fail = Some(Event::Down);
        assert!(execute(&path(true), &mut io).is_err());
        assert_eq!(io.events, [Event::Move(0), Event::Down]);
    }
    #[test]
    fn planner_checks_entire_path_and_bounds_work_before_injection() {
        let bounds = RECT {
            left: 0,
            top: 0,
            right: 20000,
            bottom: 20000,
        };
        assert!(plan(
            &[POINT { x: 1, y: 1 }, POINT { x: 19999, y: 1 }],
            true,
            bounds
        )
        .is_err());
        assert!(plan(
            &[POINT { x: 1, y: 1 }, POINT { x: 20000, y: 1 }],
            true,
            bounds
        )
        .is_err());
        assert!(plan(&[POINT { x: 1, y: 1 }; 2], true, bounds).is_err());
        let p = path(true);
        assert_eq!(p.points.len(), 3);
        assert!(p
            .points
            .windows(2)
            .all(|p| (p[1].x - p[0].x).abs() <= STEP as i32));
    }
}
