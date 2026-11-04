//! Native proofs and dispatch within the existing supervised Windows store.
use super::super::{cursor, pointer};
use super::*;

impl Store {
    pub(super) fn pointer_input(
        &mut self,
        target: &WindowTarget,
        window: HWND,
        r: &Value,
        until: Instant,
        dispatched: &mut bool,
    ) -> Result<(), Error> {
        if r["input_policy"] == "background_only" {
            return Err(Error::NativeFailure {
                code: "foreground_required".into(),
                message: "Pointer gestures require visible foreground input".into(),
            });
        }
        let pixels = self
            .pixels
            .remove(string(r, "binding")?)
            .ok_or(Error::StaleObservation)?;
        if pixels.target != *target
            || pixels.expires <= Instant::now()
            || pixels.dpi != unsafe { GetDpiForWindow(window) }
            || pixels.render_size != render_size(window)?
            || !equal_rect(pixels.bounds, unobstructed(window)?)
        {
            return Err(Error::StaleObservation);
        }
        let values = r["path"]
            .as_array()
            .filter(|path| !path.is_empty() && path.len() <= 256)
            .ok_or(Error::InvalidPoint)?;
        let mut points = Vec::new();
        for value in values {
            let x = value["x"].as_f64().ok_or(Error::InvalidPoint)?;
            let y = value["y"].as_f64().ok_or(Error::InvalidPoint)?;
            if !x.is_finite()
                || !y.is_finite()
                || x < 0.
                || y < 0.
                || x >= (pixels.bounds.right - pixels.bounds.left) as f64
                || y >= (pixels.bounds.bottom - pixels.bounds.top) as f64
            {
                return Err(Error::InvalidPoint);
            }
            points.push(POINT {
                x: pixels.bounds.left + x.floor() as i32,
                y: pixels.bounds.top + y.floor() as i32,
            });
        }
        let monitor = unsafe { MonitorFromRect(&pixels.bounds, MONITOR_DEFAULTTONULL) };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if monitor.0.is_null()
            || !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool()
            || !contains_rect(info.rcMonitor, pixels.bounds)
        {
            return Err(Error::NativeFailure {
                code: "unsupported_display_boundary".into(),
                message: "Pointer paths require a window fully on one display".into(),
            });
        }
        let drag = r["operation"] == "drag";
        let plan = pointer::plan(&points, drag, info.rcMonitor)?;
        for point in plan.points() {
            verify_point(window, target.app.pid, *point)?;
        }
        input::no_held_keys()?;
        self.consume(target);
        input::focus(window, until, dispatched)?;
        self.input_boundary(target, window, until)?;
        if !equal_rect(pixels.bounds, unobstructed(window)?)
            || pixels.dpi != unsafe { GetDpiForWindow(window) }
        {
            return Err(Error::StaleObservation);
        }
        let mut cursor = cursor::Emitter::new(&r["cursor_feedback"], target);
        if drag && native_caption(window, points[0], until)? {
            let end = points.last().unwrap();
            let (dx, dy) = (end.x - points[0].x, end.y - points[0].y);
            let destination = RECT {
                left: pixels.bounds.left + dx,
                top: pixels.bounds.top + dy,
                right: pixels.bounds.right + dx,
                bottom: pixels.bounds.bottom + dy,
            };
            if !contains_rect(info.rcMonitor, destination) {
                return Err(Error::InvalidPoint);
            }
            let mut raw = RECT::default();
            unsafe { GetWindowRect(window, &mut raw) }.map_err(failed)?;
            let _mutation = input::mutation(until)?;
            self.input_boundary(target, window, until)?;
            if !equal_rect(pixels.bounds, unobstructed(window)?) {
                return Err(Error::StaleObservation);
            }
            super::super::desktop::ensure_input_desktop()?;
            *dispatched = true;
            unsafe {
                SetWindowPos(
                    window,
                    HWND::default(),
                    raw.left + dx,
                    raw.top + dy,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                )
            }
            .map_err(failed)?;
            self.validate(target)?;
            if !equal_rect(rect(window)?, destination) {
                return Err(Error::TargetChanged);
            }
            if let Some(cursor) = cursor.as_mut() {
                cursor.posted(end.x, end.y, false);
            }
            return Ok(());
        }
        let mut accepted = 0;
        pointer::dispatch(&plan, window, until, dispatched, &mut accepted, |point| {
            self.input_boundary(target, window, until)?;
            if pixels.expires <= Instant::now()
                || pixels.dpi != unsafe { GetDpiForWindow(window) }
                || !equal_rect(pixels.bounds, unobstructed(window)?)
                || pixels.render_size != render_size(window)?
                || unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONULL) } != monitor
            {
                return Err(Error::StaleObservation);
            }
            verify_point(window, target.app.pid, point)?;
            if let Some(cursor) = cursor.as_mut() {
                cursor.posted(point.x, point.y, false);
            }
            Ok(())
        })
    }
}

fn contains_rect(outer: RECT, inner: RECT) -> bool {
    inner.left >= outer.left
        && inner.top >= outer.top
        && inner.right <= outer.right
        && inner.bottom <= outer.bottom
}
fn native_caption(window: HWND, point: POINT, until: Instant) -> Result<bool, Error> {
    check(until)?;
    if unsafe { GetWindowLongW(window, GWL_STYLE) } as u32 & WS_CAPTION.0 != WS_CAPTION.0
        || unsafe { IsZoomed(window) }.as_bool()
    {
        return Ok(false);
    }
    let mut hit = 0usize;
    let packed = ((point.y as u32 & 0xffff) << 16) | (point.x as u32 & 0xffff);
    let timeout = until
        .saturating_duration_since(Instant::now())
        .as_millis()
        .min(200) as u32;
    if unsafe {
        SendMessageTimeoutW(
            window,
            WM_NCHITTEST,
            WPARAM(0),
            LPARAM(packed as isize),
            SMTO_ABORTIFHUNG,
            timeout,
            Some(&mut hit),
        )
    }
    .0 == 0
    {
        return Err(failed("Native title-bar hit test did not complete"));
    }
    check(until)?;
    Ok(hit == HTCAPTION as usize)
}
