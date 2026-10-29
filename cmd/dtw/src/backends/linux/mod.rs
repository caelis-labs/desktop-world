//! Experimental native X11 provider; not a Linux desktop support commitment.
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
    const COVERAGE:&'static str="unverified local X11/EWMH windows with XRes 1.2 PID proof; visible client-pixel capture; XTest foreground input";
    const CONDITIONS:&'static [&'static str]=&[
        "experimental source baseline; Linux GUI acceptance and official support are not claimed",
        "requires a local X11 DISPLAY, EWMH window manager, XRes 1.2, XInput 2 and XTest",
        "native Wayland and XWayland sessions are explicitly unsupported in this provider",
        "capture requires an unobstructed client window; no AT-SPI semantic operations yet",
        "text input is limited to characters in keyboard group zero with no active group switch or text-altering modifiers; no clipboard fallback",
        "input establishes foreground focus and uses the shared pointer and keyboard",
    ];
    const CAPABILITIES: &'static [Capability] = &[
        Capability::Windows,
        Capability::Capture,
        Capability::Click,
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
