//! Native input capability rules. No Agent-facing delivery mode or retry policy.
//!
//! Evidence and bounded GUI reproductions are in docs/macos-input-routing.md.
//! Unknown frameworks keep the addressed event route. Missing AX, stale state,
//! timeouts and arbitrary provider errors never establish a physical fallback.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PhysicalReason {
    NativeHoverTracking,
    MissingAddressedApi,
    KeyboardSemantics,
}
impl PhysicalReason {
    pub(super) fn code(self) -> &'static str {
        match self {
            Self::NativeHoverTracking => "native_hover_tracking",
            Self::MissingAddressedApi => "addressed_event_api_unavailable",
            Self::KeyboardSemantics => "native_keyboard_semantics",
        }
    }
}

/// Coordinate input can prove its surface without AX. Keyboard fallback still
/// needs the exact AX window and editing destination before every key down.
pub(super) fn visual_focus(operation: &str) -> bool {
    matches!(operation, "click" | "move" | "drag" | "scroll")
}

/// The all-addressed acceptance has effects for every other mouse operation,
/// but no hover callbacks in AppKit/Chromium. Do not generalize this exception
/// to their clicks, modal input, scroll or drag. Unknown frameworks stay addressed.
pub(super) fn pointer_reason(operation: &str, framework: &str) -> Option<PhysicalReason> {
    (operation == "move" && matches!(framework, "appkit" | "chromium"))
        .then_some(PhysicalReason::NativeHoverTracking)
}

/// An explicit unsupported capability must also be cleanly not dispatched.
/// A backend flag/message alone cannot authorize a second delivery path.
pub(super) fn fallback_reason(result: &serde_json::Value) -> Option<PhysicalReason> {
    if result["dispatch"] != "not_dispatched"
        || result["fallback_allowed"] != true
        || result["error"]["code"] != "background_unavailable"
        || result.get("cleanup_error").is_some()
    {
        return None;
    }
    match result["capability_evidence"].as_str()? {
        "addressed_event_api_unavailable" => Some(PhysicalReason::MissingAddressedApi),
        "native_keyboard_semantics" => Some(PhysicalReason::KeyboardSemantics),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn visual_pointer_admission_never_bypasses_keyboard_focus_checks() {
        for operation in ["click", "move", "drag", "scroll"] {
            assert!(visual_focus(operation), "{operation}");
        }
        for operation in ["type_text", "keypress", "focus", "keypress_app", "unknown"] {
            assert!(!visual_focus(operation), "{operation}");
        }
    }

    #[test]
    fn only_the_failed_hover_shape_requires_tracking_fallback() {
        for framework in ["appkit", "chromium", "qt", "unknown"] {
            for operation in ["click", "drag", "scroll"] {
                assert_eq!(pointer_reason(operation, framework), None);
            }
        }
        assert_eq!(pointer_reason("move", "unknown"), None);
        assert_eq!(pointer_reason("move", "qt"), None);
        assert_eq!(
            pointer_reason("move", "appkit"),
            Some(PhysicalReason::NativeHoverTracking)
        );
        assert_eq!(
            pointer_reason("move", "chromium"),
            Some(PhysicalReason::NativeHoverTracking)
        );
    }
    #[test]
    fn errors_or_uncertain_preparation_never_authorize_fallback() {
        for dispatch in ["dispatched", "unknown", "not_dispatched"] {
            for error in [
                "children_unavailable",
                "permission_required",
                "stale_observation",
                "cancelled",
                "background_unavailable",
            ] {
                assert_eq!(
                    fallback_reason(
                        &json!({"dispatch":dispatch,"fallback_allowed":true,"error":{"code":error}})
                    ),
                    None
                );
            }
        }
        let mut proven = json!({"dispatch":"not_dispatched","fallback_allowed":true,"capability_evidence":"addressed_event_api_unavailable", "error":{"code":"background_unavailable"}});
        assert_eq!(
            fallback_reason(&proven),
            Some(PhysicalReason::MissingAddressedApi)
        );
        proven["dispatch"] = json!("unknown");
        assert_eq!(fallback_reason(&proven), None);
    }
}
