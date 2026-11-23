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
    const COVERAGE: &'static str = "local X11/EWMH window inventory with XRes PID proof, visible client pixels, XTest foreground input, bounded single-window AT-SPI semantics and restricted XDG application startup";
    const CONDITIONS: &'static [&'static str] = &[
        "requires a local X11 DISPLAY, EWMH window manager, XRes 1.2, XInput 2 and XTest",
        "Wayland and XWayland are unsupported",
        "capture requires an unobstructed client window; app quit is unsupported",
        "launch is limited to argument-free absolute ELF executables registered in supported XDG desktop entries",
        "text input is limited to characters in keyboard group zero with no active group switch or text-altering modifiers",
        "input establishes foreground focus and uses the shared pointer and keyboard",
        "AT-SPI semantics require a matching single X11 window, local accessibility bus, and toolkit support",
        "pointer gestures stay within one visible client window",
    ];
    const CAPABILITIES: &'static [Capability] = &[
        Capability::Windows,
        Capability::Launch,
        Capability::Observe,
        Capability::ReadText,
        Capability::Press,
        Capability::SetValue,
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
