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
    const FOREGROUND_CAPTURE: bool = true;
    const COVERAGE: &'static str = "installed Win32/packaged apps and running user windows; retained UIA queries; physical-pixel foreground window capture";
    const CONDITIONS: &'static [&'static str] = &[
        "Windows interactive user desktop; elevated and secure desktops are unsupported",
        "app IDs are canonical executable paths or proven win:<AUMID> identities; window refs route across matching process instances",
        "capture crops the composed desktop to the foreground window, including visible popups within its bounds; pixels behind other applications and outside the displays are hidden",
        "semantic scopes: window, retained subtree, capture-pixel region and native menu within the selected window; popup menu keyboard ownership requires its native menu-owner proof",
        "readText pages native UIA TextPattern, ValuePattern or static text; passwords and unsupported controls are not synthesized",
        "window waits prove same-process owned native modals with Win32 owner/disabled state and UIA IsModal; same-window web modals and cross-process dialogs are unsupported",
        "scroll anchors bind retained capture pixels or a live UIA element clipped through its ancestor rectangles; missing native geometry fails explicitly",
        "interaction prepares the exact foreground receiver; UIA drives semantic controls and physical pointer input requires an unobstructed hit; a proven native caption can activate the receiver when ordinary foreground coordination is refused",
        "input operates in the foreground; visible external handoffs stop further input for the script, including away-and-return; focus and physical pointer position stay where the operation leaves them",
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
