//! Native X11 provider with bounded window, capture and foreground input support.
use super::native_client::{self, Platform as PlatformContract};
use crate::computer::{Capability, Error};
mod client_pid;
mod native;
mod worker;
pub use native_client::AppInfo;
pub use worker::worker_entry;
pub struct Platform;
pub type NativeBackend = native_client::NativeBackend<Platform>;
impl PlatformContract for Platform {
    const ID: &'static str = "dtw-linux-x11-experimental";
    const COVERAGE: &'static str = "running local X11/EWMH applications with XRes 1.2 PID proof; window metadata, visible client pixels and XTest foreground input";
    const CONDITIONS: &'static [&'static str] = &[
        "requires a local X11 DISPLAY, EWMH window manager, XRes 1.2, XInput 2 and XTest",
        "Wayland and XWayland are unsupported",
        "capture requires an unobstructed client window; AT-SPI, app launch and app quit are unsupported",
        "text input is limited to characters in keyboard group zero with no active group switch or text-altering modifiers",
        "input establishes foreground focus and uses the shared pointer and keyboard",
        "pointer gestures stay within one visible client window",
    ];
    const CAPABILITIES: &'static [Capability] = &[
        Capability::Windows,
        Capability::Capture,
        Capability::Click,
        Capability::Move,
        Capability::Drag,
        Capability::TypeText,
        Capability::Keypress,
        Capability::Scroll,
        Capability::Focus,
    ];
    fn worker_args() -> Result<Vec<String>, Error> {
        Ok(vec![
            "--dtw-native-worker".into(),
            std::process::id().to_string(),
        ])
    }
}
