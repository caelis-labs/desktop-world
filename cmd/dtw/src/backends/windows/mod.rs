//! Windows desktop provider. Its UIA apartment and all blocking APIs are owned
//! by a supervised DTW process, never an Agent thread or a CUA runtime.
use super::native_client::{self, Platform as PlatformContract};
use crate::computer::{Capability, Error, ObserveScopeKind};
mod input;
mod native;
mod worker;
pub use native_client::AppInfo;
pub use worker::{parent_alive_check, request_stop, worker_entry};
pub struct Platform;
pub type NativeBackend = native_client::NativeBackend<Platform>;
impl PlatformContract for Platform {
    const ID: &'static str = "dtw-windows-native";
    const COVERAGE: &'static str = "same-process Win32 top-level windows; retained UIA queries; visible unobstructed physical-pixel capture";
    const CONDITIONS: &'static [&'static str] = &[
        "Windows interactive user desktop; elevated and secure desktops are unsupported",
        "app IDs are canonical executable paths; window refs route across matching process instances",
        "capture requires an unminimized fully visible window and proven native GDI rendering; blank or unsupported renders fail explicitly; no desktop-copy fallback or occluded WGC capture",
        "semantic scopes: window, retained subtree, capture-pixel region; menu scope is unsupported",
        "readText pages native UIA TextPattern, ValuePattern or static text; passwords and unsupported controls are not synthesized",
        "input establishes foreground focus and moves the shared pointer for clicks",
        "native Windows GUI acceptance is pending; compilation is not GUI evidence",
    ];
    const CAPABILITIES: &'static [Capability] = &[
        Capability::Windows,
        Capability::Observe,
        Capability::Press,
        Capability::Capture,
        Capability::Click,
        Capability::TypeText,
        Capability::Keypress,
        Capability::Scroll,
        Capability::Focus,
        Capability::SetValue,
        Capability::ReadText,
    ];
    const DISCOVERY_SCOPES: &'static [ObserveScopeKind] = &[
        ObserveScopeKind::Window,
        ObserveScopeKind::Subtree,
        ObserveScopeKind::Region,
    ];
    fn worker_args() -> Result<Vec<String>, Error> {
        Ok(vec![
            "--dtw-native-worker".into(),
            std::process::id().to_string(),
            native::process_stamp(std::process::id())?
                .generation
                .to_string(),
        ])
    }
}
