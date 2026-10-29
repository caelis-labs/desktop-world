// Adapted from trycua/cua b38360443960a6540685a9a5eb5407f10fe3d03b.
// Source: libs/cua-driver/rust/crates/platform-macos/src/input/mouse.rs
// Copyright (c) 2025 Cua AI, Inc. MIT. See docs/background-input-port.md.

use core_graphics::{
    event::{CGEvent, CGEventFlags, CGEventType, CGMouseButton},
    event_source::{CGEventSource, CGEventSourceStateID},
    geometry::CGPoint,
};
use foreign_types::ForeignType;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChromiumClickEventKind {
    Move,
    Down,
    Up,
}

#[derive(Clone, Copy, Debug)]
struct ChromiumClickRouteStep {
    kind: ChromiumClickEventKind,
    event_location: (f64, f64),
    skylight_window_location: (f64, f64),
    click_state: i64,
    phase: i64,
    button_number: i64,
    subtype: i64,
    delay_after_ms: u64,
}

fn chromium_click_route_plan(
    screen_x: f64,
    screen_y: f64,
    win_local_x: f64,
    win_local_y: f64,
    count: usize,
) -> Vec<ChromiumClickRouteStep> {
    let screen_target = (screen_x, screen_y);
    // The CGEvent keeps its desktop point; CGEventSetWindowLocation is the
    // point inside the addressed window. Chromium otherwise adds the window's
    // origin twice (verified by independent DOM pointer coordinates).
    let window_target = (win_local_x, win_local_y);
    let off_screen = (-1.0, -1.0);
    let mut steps = vec![
        ChromiumClickRouteStep {
            kind: ChromiumClickEventKind::Move,
            event_location: screen_target,
            skylight_window_location: window_target,
            click_state: 0,
            phase: 2,
            button_number: 0,
            subtype: 3,
            delay_after_ms: 15,
        },
        ChromiumClickRouteStep {
            kind: ChromiumClickEventKind::Down,
            event_location: off_screen,
            skylight_window_location: off_screen,
            click_state: 1,
            phase: 1,
            button_number: 0,
            subtype: 3,
            delay_after_ms: 1,
        },
        ChromiumClickRouteStep {
            kind: ChromiumClickEventKind::Up,
            event_location: off_screen,
            skylight_window_location: off_screen,
            click_state: 1,
            phase: 2,
            button_number: 0,
            subtype: 3,
            delay_after_ms: 100,
        },
    ];

    let click_pairs = count.clamp(1, 2);
    for pair_index in 1..=click_pairs {
        let click_state = pair_index as i64;
        steps.push(ChromiumClickRouteStep {
            kind: ChromiumClickEventKind::Down,
            event_location: screen_target,
            skylight_window_location: window_target,
            click_state,
            phase: 3,
            button_number: 0,
            subtype: 3,
            delay_after_ms: 1,
        });
        steps.push(ChromiumClickRouteStep {
            kind: ChromiumClickEventKind::Up,
            event_location: screen_target,
            skylight_window_location: window_target,
            click_state,
            phase: 3,
            button_number: 0,
            subtype: 3,
            delay_after_ms: if pair_index < click_pairs { 80 } else { 0 },
        });
    }
    steps
}

fn set_explicit_flags(event: &CGEvent, flags: CGEventFlags) {
    event.set_flags(flags);
}

pub fn click_at_xy_chromium(
    pid: i32,
    screen_x: f64,
    screen_y: f64,
    win_local_x: f64,
    win_local_y: f64,
    wid: u32,
    count: usize,
    modifiers: &[&str],
) -> anyhow::Result<()> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow::anyhow!("CGEventSource::new failed"))?;
    let route_plan = chromium_click_route_plan(screen_x, screen_y, win_local_x, win_local_y, count);
    let flags = parse_modifier_flags(modifiers);
    let window_id = wid as i64;

    // All 5 events share the same click-group ID so WindowServer / Chromium
    // treat the sequence as one gesture (Swift: field 58).
    let click_group_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as i64;

    // Stamp required fields onto a CGEvent.  All captured values are Copy so
    // this closure is Fn (callable multiple times).
    let stamp = |event: &CGEvent, step: &ChromiumClickRouteStep| {
        let ptr = event.as_ptr() as *mut std::ffi::c_void;
        let set = |f: u32, v: i64| {
            super::skylight::set_integer_field(ptr, f, v);
        };
        set(0, step.phase); // kCGMouseEventNumber (gesture phase)
        set(1, step.click_state); // kCGMouseEventClickState
        set(3, step.button_number); // kCGMouseEventButtonNumber (left)
        set(7, step.subtype); // kCGMouseEventSubtype (NSEventSubtypeTouch)
        set(40, pid as i64); // Chromium synthetic-event filter
        if window_id != 0 {
            set(51, window_id); // windowNumber (NSEvent bridge equivalent)
            set(91, window_id); // kCGMouseEventWindowUnderMousePointer
            set(92, window_id); // kCGMouseEventWindowUnderMousePointerThatCanHandleThisEvent
        }
        set(58, click_group_id); // click-group ID (gesture coalescing)
        super::skylight::set_window_location(
            ptr,
            step.skylight_window_location.0,
            step.skylight_window_location.1,
        );
        set_explicit_flags(event, flags);
    };

    let post = |event: &CGEvent, _step: &ChromiumClickRouteStep| {
        dispatch_mouse_event(
            post_route(pointer_post_mode(), skylight_available()),
            pid,
            event,
        );
    };

    for step in route_plan {
        super::control::check()?;
        let (event_type, error_label) = match step.kind {
            ChromiumClickEventKind::Move => (CGEventType::MouseMoved, "mouseMoved"),
            ChromiumClickEventKind::Down if step.phase == 1 => {
                (CGEventType::LeftMouseDown, "primer down")
            }
            ChromiumClickEventKind::Up if step.phase == 2 => {
                (CGEventType::LeftMouseUp, "primer up")
            }
            ChromiumClickEventKind::Down => (CGEventType::LeftMouseDown, "target down"),
            ChromiumClickEventKind::Up => (CGEventType::LeftMouseUp, "target up"),
        };
        let event = CGEvent::new_mouse_event(
            source.clone(),
            event_type,
            CGPoint::new(step.event_location.0, step.event_location.1),
            CGMouseButton::Left,
        )
        .map_err(|_| anyhow::anyhow!("{error_label} event creation failed"))?;
        stamp(&event, &step);
        post(&event, &step);
        if step.delay_after_ms > 0 {
            super::control::sleep(std::time::Duration::from_millis(step.delay_after_ms));
        }
    }

    Ok(())
}

pub fn right_click_at_xy_with_window_local(
    pid: i32,
    x: f64,
    y: f64,
    wx: f64,
    wy: f64,
    wid: u32,
    modifiers: &[&str],
) -> anyhow::Result<()> {
    right_click_at_xy_inner(
        pid,
        x,
        y,
        Some((wx, wy)),
        Some(wid),
        modifiers,
        CGMouseButton::Right,
    )
}

fn right_click_at_xy_inner(
    pid: i32,
    x: f64,
    y: f64,
    window_local: Option<(f64, f64)>,
    wid: Option<u32>,
    modifiers: &[&str],
    button: CGMouseButton,
) -> anyhow::Result<()> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let (down_type, up_type, button_number) = match button {
        CGMouseButton::Center => (CGEventType::OtherMouseDown, CGEventType::OtherMouseUp, 2),
        _ => (CGEventType::RightMouseDown, CGEventType::RightMouseUp, 1),
    };

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow::anyhow!("CGEventSource::new failed"))?;
    let point = CGPoint::new(x, y);
    let flags = parse_modifier_flags(modifiers);

    let click_group_id: Option<i64> = wid.map(|_| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as i64
    });

    post_mouse_moved_primer(
        pid,
        &source,
        point,
        window_local,
        wid,
        click_group_id,
        pointer_post_mode(),
    );
    super::control::sleep(std::time::Duration::from_millis(12));

    let down = CGEvent::new_mouse_event(source.clone(), down_type, point, button)
        .map_err(|_| anyhow::anyhow!("mouse down failed"))?;
    set_explicit_flags(&down, flags);
    post_mouse_event_with_mode(
        pid,
        &down,
        window_local,
        wid,
        click_group_id,
        1,
        button_number,
        3,
        pointer_post_mode(),
    );
    super::control::sleep(std::time::Duration::from_millis(28));

    let up = CGEvent::new_mouse_event(source, up_type, point, button)
        .map_err(|_| anyhow::anyhow!("mouse up failed"))?;
    set_explicit_flags(&up, flags);
    post_mouse_event_with_mode(
        pid,
        &up,
        window_local,
        wid,
        click_group_id,
        1,
        button_number,
        3,
        pointer_post_mode(),
    );

    Ok(())
}

fn post_mouse_event_with_mode(
    pid: i32,
    event: &CGEvent,
    window_local: Option<(f64, f64)>,
    wid: Option<u32>,
    click_group_id: Option<i64>,
    click_state: i64,
    button_number: i64,
    subtype: i64,
    mode: MousePostMode,
) {
    let event_ptr = event.as_ptr() as *mut std::ffi::c_void;

    // Stamp window-local point for backgrounded window targeting.
    if let Some((wx, wy)) = window_local {
        super::skylight::set_window_location(event_ptr, wx, wy);
    }

    // Chromium / AppKit window-routing fields — stamp when window_id is known.
    if let (Some(wid), Some(cgid)) = (wid, click_group_id) {
        let window_id = wid as i64;
        let set = |f: u32, v: i64| {
            super::skylight::set_integer_field(event_ptr, f, v);
        };
        set(1, click_state); // kCGMouseEventClickState
        set(3, button_number); // kCGMouseEventButtonNumber (0=left, 1=right, 2=middle)
        set(7, subtype); // kCGMouseEventSubtype (touch for clicks, normal for drags)
        set(51, window_id); // windowNumber
        set(58, cgid); // click-group ID (gesture coalescing)
        set(91, window_id); // kCGMouseEventWindowUnderMousePointer
        set(92, window_id); // kCGMouseEventWindowUnderMousePointerThatCanHandleThisEvent
    }

    // Always stamp f40 = target pid (Chromium synthetic-event filter).
    super::skylight::set_integer_field(event_ptr, 40, pid as i64);

    dispatch_mouse_event(post_route(mode, skylight_available()), pid, event);
}

fn post_mouse_moved_primer(
    pid: i32,
    source: &CGEventSource,
    point: CGPoint,
    window_local: Option<(f64, f64)>,
    wid: Option<u32>,
    click_group_id: Option<i64>,
    mode: MousePostMode,
) {
    if let Ok(mv) = CGEvent::new_mouse_event(
        source.clone(),
        CGEventType::MouseMoved,
        point,
        CGMouseButton::Left,
    ) {
        set_explicit_flags(&mv, CGEventFlags::CGEventFlagNull);
        post_mouse_event_with_mode(pid, &mv, window_local, wid, click_group_id, 0, 0, 3, mode);
    }
}

pub fn scroll_wheel_at_xy(
    pid: i32,
    screen_x: f64,
    screen_y: f64,
    window_local: Option<(f64, f64)>,
    wid: Option<u32>,
    delta_y_per_tick: i32,
    delta_x_per_tick: i32,
    ticks: usize,
) -> anyhow::Result<()> {
    use core_graphics::event::ScrollEventUnit;

    // Prime AppKit/WebKit's tracking state at the target before the wheel
    // gesture. Background windows can retain a stale hit-test location; a
    // nested overflow scroller then receives the wheel nowhere even though
    // the event is successfully posted to the target pid.
    let primer_source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow::anyhow!("CGEventSource::new failed"))?;
    post_mouse_moved_primer(
        pid,
        &primer_source,
        CGPoint::new(screen_x, screen_y),
        window_local,
        wid,
        Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos() as i64,
        ),
        pointer_post_mode(),
    );
    super::control::sleep(std::time::Duration::from_millis(12));

    for _ in 0..ticks.max(1) {
        super::control::check()?;
        // Fresh source per event, matching the click primitives.
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| anyhow::anyhow!("CGEventSource::new failed"))?;
        // wheel_count = 2 → both axes carried (vertical = wheel1/axis-1,
        // horizontal = wheel2/axis-2). Convert the driver's pixel-tuned step
        // into a bounded line delta for the event API.
        let wheel_y = delta_y_per_tick.clamp(-10, 10);
        let wheel_x = delta_x_per_tick.clamp(-10, 10);
        let event =
            CGEvent::new_scroll_event(source, ScrollEventUnit::LINE, 2, wheel_y, wheel_x, 0)
                .map_err(|_| anyhow::anyhow!("CGEvent::new_scroll_event failed"))?;
        // A user's modifiers in another app must not turn this into Shift-wheel
        // horizontal scrolling or a control/command-wheel gesture.
        set_explicit_flags(&event, CGEventFlags::CGEventFlagNull);

        let event_ptr = event.as_ptr() as *mut std::ffi::c_void;

        // Anchor the event at the target screen point so the renderer's wheel
        // hit-test routes the scroll to the element under the cursor.
        unsafe { CGEventSetLocation(event_ptr, screen_x, screen_y) };

        // Background-delivery stamps (mirror post_mouse_event).
        if let Some((wx, wy)) = window_local {
            super::skylight::set_window_location(event_ptr, wx, wy);
        }
        if let Some(wid) = wid {
            let window_id = wid as i64;
            let set = |f: u32, v: i64| {
                super::skylight::set_integer_field(event_ptr, f, v);
            };
            set(51, window_id); // windowNumber
            set(91, window_id); // kCGMouseEventWindowUnderMousePointer
            set(92, window_id); // ...ThatCanHandleThisEvent
        }
        // f40 = target pid (Chromium synthetic-event filter).
        super::skylight::set_integer_field(event_ptr, 40, pid as i64);

        // DTW posts once; no duplicate public/PID delivery.
        dispatch_mouse_event(pointer_post_mode(), pid, &event);

        super::control::sleep(std::time::Duration::from_millis(30));
    }
    Ok(())
}

fn parse_modifier_flags(modifiers: &[&str]) -> CGEventFlags {
    let mut flags = CGEventFlags::CGEventFlagNull;
    for m in modifiers {
        match m.to_lowercase().as_str() {
            "cmd" | "command" | "super" | "meta" | "win" => {
                flags |= CGEventFlags::CGEventFlagCommand
            }
            "shift" => flags |= CGEventFlags::CGEventFlagShift,
            "option" | "alt" => flags |= CGEventFlags::CGEventFlagAlternate,
            "ctrl" | "control" => flags |= CGEventFlags::CGEventFlagControl,
            _ => {}
        }
    }
    flags
}

#[derive(Clone, Copy)]
enum MousePostMode {
    SkyLightPreferred,
    PublicOnly,
}
fn post_route(mode: MousePostMode, _available: bool) -> MousePostMode {
    mode
}
fn skylight_available() -> bool {
    super::skylight::is_available()
}
fn dispatch_mouse_event(mode: MousePostMode, pid: i32, event: &CGEvent) {
    super::control::post_event_via(pid, event, false, matches!(mode, MousePostMode::PublicOnly));
}
extern "C" {
    fn CGEventSetLocation(event: *mut std::ffi::c_void, x: f64, y: f64);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chromium_recipe_keeps_distinct_screen_and_window_coordinates_and_pairs() {
        let plan = chromium_click_route_plan(700.0, 400.0, 10.0, 20.0, 2);
        assert_eq!(plan.len(), 7);
        assert_eq!(plan[0].event_location, (700.0, 400.0));
        assert_eq!(plan[0].skylight_window_location, (10.0, 20.0));
        assert_eq!(plan[1].event_location, (-1.0, -1.0));
        assert_eq!(plan[1].kind, ChromiumClickEventKind::Down);
        assert_eq!(plan[2].kind, ChromiumClickEventKind::Up);
        assert_eq!(plan[3].click_state, 1);
        assert_eq!(plan[5].click_state, 2);
        assert_eq!(plan[5].event_location, (700.0, 400.0));
        assert_eq!(plan[6].kind, ChromiumClickEventKind::Up);
        for step in &plan[1..3] {
            assert_eq!(step.event_location, (-1.0, -1.0));
            assert_eq!(step.skylight_window_location, (-1.0, -1.0));
        }
        for step in &plan[3..] {
            assert_eq!(step.event_location, (700.0, 400.0));
            assert_eq!(step.skylight_window_location, (10.0, 20.0));
        }
    }
    #[test]
    fn chromium_capture_point_retains_window_local_hit_at_positive_and_negative_origins() {
        // Candidate F: the 1600x1200 window image covers 1000x750 logical
        // points. Chrome's content starts 87 points below its window frame.
        let local = (527.0 * 1000.0 / 1600.0, 895.0 * 750.0 / 1200.0);
        let expected_client = (329.375, 472.375);
        for origin in [(230.0, 120.0), (-1450.0, -180.0)] {
            let screen = (origin.0 + local.0, origin.1 + local.1);
            let plan = chromium_click_route_plan(screen.0, screen.1, local.0, local.1, 1);
            for step in [&plan[0], &plan[3], &plan[4]] {
                assert_eq!(step.event_location, screen);
                assert_eq!(step.skylight_window_location, local);
                let client = (
                    step.skylight_window_location.0,
                    step.skylight_window_location.1 - 87.0,
                );
                assert_eq!(client, expected_client);
            }
        }
    }
}

pub fn click_at_xy_generic(
    pid: i32,
    x: f64,
    y: f64,
    wx: f64,
    wy: f64,
    wid: u32,
    count: usize,
    modifiers: &[&str],
) -> anyhow::Result<()> {
    click_at_xy_inner(
        pid,
        (x, y),
        Some((wx, wy)),
        Some(wid),
        count,
        modifiers,
        pointer_post_mode(),
    )
}
fn click_at_xy_inner(
    pid: i32,
    screen: (f64, f64),
    window_local: Option<(f64, f64)>,
    wid: Option<u32>,
    count: usize,
    modifiers: &[&str],
    post_mode: MousePostMode,
) -> anyhow::Result<()> {
    let (x, y) = screen;
    use std::time::{SystemTime, UNIX_EPOCH};

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow::anyhow!("CGEventSource::new failed"))?;
    let point = CGPoint::new(x, y);
    let flags = parse_modifier_flags(modifiers);

    // Shared click-group ID (f58) when window_id is known: keeps all pairs
    // in one gesture so WindowServer / Chromium coalesce them correctly.
    let click_group_id: Option<i64> = wid.map(|_| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as i64
    });

    super::keyboard::with_pid_modifier_keys(pid, modifiers, || {
        // Prime the target window's cursor-tracking state with a leading
        // mouseMoved so an AppKit NSButton / NSView hit-tests the down at the
        // right point. Without it the synthetic mouseDown on a backgrounded
        // AppKit control is silently ignored.
        post_mouse_moved_primer(
            pid,
            &source,
            point,
            window_local,
            wid,
            click_group_id,
            post_mode,
        );
        super::control::sleep(std::time::Duration::from_millis(12));

        for pair_index in 0..count {
            super::control::check()?;
            let click_state = (pair_index + 1) as i64;

            let down = CGEvent::new_mouse_event(
                source.clone(),
                CGEventType::LeftMouseDown,
                point,
                CGMouseButton::Left,
            )
            .map_err(|_| anyhow::anyhow!("CGEvent::new_mouse_event(down) failed"))?;
            set_explicit_flags(&down, flags);

            post_mouse_event_with_mode(
                pid,
                &down,
                window_local,
                wid,
                click_group_id,
                click_state,
                0,
                3,
                post_mode,
            );
            // 28 ms down→up gap: an NSButton's mouseDown enters a modal
            // tracking loop that polls for the matching mouseUp; too tight a
            // gap can race the loop's first poll and the click is dropped.
            super::control::sleep(std::time::Duration::from_millis(28));

            let up = CGEvent::new_mouse_event(
                source.clone(),
                CGEventType::LeftMouseUp,
                point,
                CGMouseButton::Left,
            )
            .map_err(|_| anyhow::anyhow!("CGEvent::new_mouse_event(up) failed"))?;
            set_explicit_flags(&up, flags);

            post_mouse_event_with_mode(
                pid,
                &up,
                window_local,
                wid,
                click_group_id,
                click_state,
                0,
                3,
                post_mode,
            );

            if count > 1 {
                super::control::sleep(std::time::Duration::from_millis(80));
            }
        }
        Ok(())
    })
}

fn pointer_post_mode() -> MousePostMode {
    if super::control::pointer_uses_public() {
        MousePostMode::PublicOnly
    } else {
        MousePostMode::SkyLightPreferred
    }
}
