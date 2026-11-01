//! Windows desktop provider. Its UIA apartment and all blocking APIs are owned
//! by a supervised DTW process, never an Agent thread or a CUA runtime.
use super::native_client::{self, Platform as PlatformContract};
use crate::computer::{Capability, Error, ObserveScopeKind};
pub(crate) mod cursor;
mod edit_delivery;
mod input;
mod native;
mod posted_mouse;
mod text_delivery;
pub(crate) mod window_state;
mod worker;
pub use cursor::CursorSession;
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
        "window waits prove same-process owned native modals with Win32 owner/disabled state and UIA IsModal; same-window web modals and cross-process dialogs are unsupported",
        "scroll anchors bind retained capture pixels or a live UIA element clipped through its ancestor rectangles; missing native geometry fails explicitly",
        "client-area left clicks on supported HWND controls (Chromium content: single click only) and native-control wheel messages use background delivery without moving the shared pointer; other mouse gestures, keyboard input and focus require foreground; background-only rejects unsupported routes before dispatch",
        "supplementary-plane keyboard text requires a bounded readable focused TextPattern selection or same-process Unicode Edit; protected/unreadable controls are denied before typing; unconfirmed accepted text prevents clean input handoff, without replay",
        "one optional session cursor presents actual pointer posts without focus or input; exact authenticated peer cursor windows alone are exempt from capture occlusion",
        "Windows GUI coverage is recorded per candidate; compilation alone is not GUI evidence",
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
