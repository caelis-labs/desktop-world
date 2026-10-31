//! Win32 owner chains plus UIA's native modal flag, never a title/class guess.
use crate::{
    backends::native_client::{parse_target, string},
    computer::*,
};
use serde_json::Value;
pub(crate) fn parse(reply: &Value, app: &AppInstance) -> Result<WindowStateProbe, Error> {
    match string(reply, "status")? {
        "closed" => Ok(WindowStateProbe::Closed),
        "pending" => {
            let reason = string(reply, "reason")?;
            if ![
                "window_present",
                "window_not_visible",
                "blocked_by_modal",
                "dialog_not_present",
                "dialog_not_ready",
                "ambiguous_dialog",
            ]
            .contains(&reason)
            {
                return Err(Error::InvalidObservation);
            }
            Ok(WindowStateProbe::Pending {
                reason: reason.into(),
            })
        }
        "matched" => {
            let target = parse_target(&reply["window"]["target"])?;
            if &target.app != app || reply["window"]["visible"] != true {
                return Err(Error::InvalidObservation);
            }
            let (kind, relation) = match (string(reply, "kind")?, string(reply, "relation")?) {
                ("dialog", "child_modal") => (WindowKind::Dialog, WindowRelation::ChildModal),
                ("window", "same_window") => (WindowKind::Window, WindowRelation::SameWindow),
                _ => return Err(Error::InvalidObservation),
            };
            Ok(WindowStateProbe::Window(MatchedWindow {
                window: WindowInfo {
                    target,
                    title: string(&reply["window"], "title")?.into(),
                    visible: true,
                },
                kind,
                relation,
            }))
        }
        _ => Err(Error::InvalidObservation),
    }
}
