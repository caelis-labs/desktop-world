//! Windows desktop provider. Its UIA apartment and all blocking APIs are owned
//! by a supervised DTW process, never an Agent thread or a CUA runtime.
use super::native_client::{self, Platform as PlatformContract};
use crate::computer::{Capability, Error, ObserveScopeKind};
mod applications;
pub(crate) mod cursor;
mod desktop;
mod discovery;
mod discovery_cache;
mod edit_delivery;
mod execution_scope;
mod input;
mod native;
mod pointer;
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
    const COVERAGE: &'static str = "installed Win32/packaged apps and running user windows; retained UIA queries; exact-HWND physical-pixel app composition";
    const CONDITIONS: &'static [&'static str] = &[
        "Windows interactive user desktop; elevated and secure desktops are unsupported",
        "app IDs are canonical executable paths or proven win:<AUMID> identities; window refs route across matching process instances",
        "capture uses exact-HWND rendering for visible unminimized opaque or proven global-alpha/color-key surfaces, including covered windows; per-pixel layered alpha and unsupported native renders fail explicitly; no desktop-copy fallback",
        "semantic scopes: window, retained subtree, capture-pixel region and native menu within the selected window; popup menu keyboard ownership requires its native menu-owner proof",
        "readText pages native UIA TextPattern, ValuePattern or static text; passwords and unsupported controls are not synthesized",
        "window waits prove same-process owned native modals with Win32 owner/disabled state and UIA IsModal; same-window web modals and cross-process dialogs are unsupported",
        "scroll anchors bind retained capture pixels or a live UIA element clipped through its ancestor rectangles; missing native geometry fails explicitly",
        "client-area left clicks on supported HWND controls (Chromium content: single click only) and native-control wheel messages use background delivery without moving the shared pointer; other mouse gestures, keyboard input and focus prepare the exact foreground receiver internally; a proven native caption can use CUA coordinate-routed pointer activation when ordinary foreground coordination is refused; background-only rejects unsupported routes before preparation",
        "foreground and physical-pointer borrowing lasts for one script; visible external handoffs are sticky, including away-and-return; settlement restores only still-owned original resources; an unreadable hidden original cannot be guessed, and unconfirmed return retains a visible active target",
        "supplementary-plane keyboard text requires a bounded readable focused TextPattern selection or same-process Unicode Edit; protected/unreadable controls are denied before typing; unconfirmed accepted text prevents clean input handoff, without replay",
        "installed discovery reads Start Menu links, App Paths and AppsFolder identities; skipped argument-bearing links and unreadable sources remain explicit; launch executes only a resolved executable or proven packaged identity once",
        "normal quit/close requests WM_CLOSE on the selected instance and preserves save prompts; no forced termination",
        "move/drag require the original visible receiver, geometry and one display; all admitted drag buttons are released, and standard title bars move through native window position",
        "one optional session cursor presents actual pointer posts without focus or input; exact authenticated peer cursor windows alone are exempt from input occlusion",
        "Windows GUI coverage is recorded per candidate; compilation alone is not GUI evidence",
    ];
    const CAPABILITIES: &'static [Capability] = &[
        Capability::Windows,
        Capability::Observe,
        Capability::Press,
        Capability::Capture,
        Capability::Click,
        Capability::Move,
        Capability::Drag,
        Capability::Launch,
        Capability::Quit,
        Capability::CloseWindow,
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
        ObserveScopeKind::Menu,
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
