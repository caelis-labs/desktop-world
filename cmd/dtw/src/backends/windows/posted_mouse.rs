//! Targeted client-area messages, following CUA's Windows post_click route.
//! No foreign thread suspension, foreign styles, activation or shared cursor
//! changes. Queue acceptance is dispatch evidence, never proof of app effect.
use super::input;
use crate::{backends::native_client::failed, computer::Error};
use std::time::Instant;
use windows::Win32::{
    Foundation::{HWND, LPARAM, POINT, WPARAM},
    Graphics::Gdi::ScreenToClient,
    UI::{
        HiDpi::{
            GetWindowDpiAwarenessContext, PhysicalToLogicalPointForPerMonitorDPI,
            SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT,
        },
        Input::KeyboardAndMouse::IsWindowEnabled,
        WindowsAndMessaging::*,
    },
};

pub(super) struct Target {
    window: HWND,
    root: HWND,
    pid: u32,
    thread: u32,
    screen: POINT,
    client: LPARAM,
    screen_param: LPARAM,
    double_clicks: bool,
}

fn packed(point: POINT) -> Option<LPARAM> {
    let x = i16::try_from(point.x).ok()?;
    let y = i16::try_from(point.y).ok()?;
    Some(LPARAM(
        ((x as u16 as u32) | ((y as u16 as u32) << 16)) as isize,
    ))
}
fn supported(class: &str, scroll: bool, count: u64) -> bool {
    if class == "Chrome_RenderWidgetHostHWND" {
        return !scroll && count == 1;
    }
    class.starts_with("WindowsForms10.")
        || matches!(
            class.to_ascii_lowercase().as_str(),
            "button"
                | "edit"
                | "richedit20w"
                | "richedit50w"
                | "syslistview32"
                | "systreeview32"
                | "listbox"
                | "combobox"
                | "scrollbar"
        )
}
struct DpiScope(DPI_AWARENESS_CONTEXT);
impl Drop for DpiScope {
    fn drop(&mut self) {
        unsafe {
            SetThreadDpiAwarenessContext(self.0);
        }
    }
}
impl Target {
    pub(super) fn prepare(
        root: HWND,
        pid: u32,
        point: POINT,
        scroll: bool,
        count: u64,
    ) -> Result<Option<Self>, Error> {
        unsafe {
            let window = WindowFromPoint(point);
            let mut hit_pid = 0;
            let thread = GetWindowThreadProcessId(window, Some(&mut hit_pid));
            if hit_pid != pid
                || GetAncestor(window, GA_ROOT) != root
                || !IsWindowEnabled(window).as_bool()
            {
                return Err(failed(
                    "Posted mouse target is foreign, disabled or occluded",
                ));
            }
            let mut class = [0u16; 256];
            let len = GetClassNameW(window, &mut class);
            if len == 0 {
                return Err(failed("Cannot identify mouse receiver class"));
            }
            if !supported(
                &String::from_utf16_lossy(&class[..len as usize]),
                scroll,
                count,
            ) {
                return Ok(None);
            }
            // LPARAM uses the receiver's logical client coordinates, including
            // DPI-unaware receivers. Restore only our own worker thread context.
            let previous = SetThreadDpiAwarenessContext(GetWindowDpiAwarenessContext(window));
            if previous.0.is_null() {
                return Err(failed("Cannot enter receiver DPI context"));
            }
            let _scope = DpiScope(previous);
            let mut logical = point;
            PhysicalToLogicalPointForPerMonitorDPI(window, &mut logical)
                .ok()
                .map_err(failed)?;
            let screen_param = packed(logical);
            ScreenToClient(window, &mut logical).ok().map_err(failed)?;
            let mut rect = Default::default();
            GetClientRect(window, &mut rect).map_err(failed)?;
            if logical.x < rect.left
                || logical.x >= rect.right
                || logical.y < rect.top
                || logical.y >= rect.bottom
            {
                return Ok(None);
            }
            let Some(client) = packed(logical) else {
                return Ok(None);
            };
            let Some(screen_param) = screen_param else {
                return Ok(None);
            };
            Ok(Some(Self {
                window,
                root,
                pid,
                thread,
                screen: point,
                client,
                screen_param,
                double_clicks: GetClassLongW(window, GCL_STYLE) & CS_DBLCLKS.0 != 0,
            }))
        }
    }
    pub(super) fn validate(&self) -> Result<(), Error> {
        unsafe {
            let mut pid = 0;
            let thread = GetWindowThreadProcessId(self.window, Some(&mut pid));
            if pid != self.pid
                || thread != self.thread
                || GetAncestor(self.window, GA_ROOT) != self.root
                || WindowFromPoint(self.screen) != self.window
                || !IsWindowEnabled(self.window).as_bool()
            {
                return Err(failed("Posted mouse receiver changed"));
            }
        }
        Ok(())
    }
    fn post(&self, message: u32, wparam: usize, position: LPARAM) -> Result<(), Error> {
        unsafe { PostMessageW(self.window, message, WPARAM(wparam), position) }.map_err(failed)
    }
    pub(super) fn click(
        &self,
        second: bool,
        until: Instant,
        dispatched: &mut bool,
    ) -> Result<(), Error> {
        self.validate()?;
        let _mutation = input::mutation(until)?;
        self.post(WM_MOUSEMOVE, 0, self.client)?;
        *dispatched = true;
        let down = if second && self.double_clicks {
            WM_LBUTTONDBLCLK
        } else {
            WM_LBUTTONDOWN
        };
        self.post(down, 1, self.client)?;
        // The admitted segment always pairs its accepted down, even if close or
        // deadline occurs between these posts. Never replay a possibly sent click.
        if let Err(error) = self.post(WM_LBUTTONUP, 0, self.client) {
            if self.post(WM_LBUTTONUP, 0, self.client).is_err() {
                input::fail_delivery();
            }
            return Err(error);
        }
        Ok(())
    }
    pub(super) fn wheel(
        &self,
        horizontal: bool,
        delta: i32,
        until: Instant,
        dispatched: &mut bool,
    ) -> Result<(), Error> {
        self.validate()?;
        let _mutation = input::mutation(until)?;
        self.post(
            if horizontal {
                WM_MOUSEHWHEEL
            } else {
                WM_MOUSEWHEEL
            },
            (delta as u16 as usize) << 16,
            self.screen_param,
        )?;
        *dispatched = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn class_and_gesture_admission_is_narrow() {
        assert!(supported("Chrome_RenderWidgetHostHWND", false, 1));
        assert!(!supported("Chrome_RenderWidgetHostHWND", true, 1));
        assert!(!supported("Chrome_RenderWidgetHostHWND", false, 2));
        assert!(!supported("Chrome_WidgetWin_1", false, 1));
        assert!(!supported("unrecognized", false, 1));
        assert!(supported("WindowsForms10.Window.8.app.0.abc", true, 1));
    }
    #[test]
    fn signed_message_coordinates_do_not_clamp_or_wrap() {
        assert_eq!(packed(POINT { x: -4, y: 7 }).unwrap().0 as u32, 0x0007fffc);
        assert!(packed(POINT { x: 32768, y: 0 }).is_none());
        assert!(packed(POINT { x: 0, y: -32769 }).is_none());
    }
}
