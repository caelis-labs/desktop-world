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
    KeyboardFocusAbsent,
}
impl PhysicalReason {
    pub(super) fn code(self) -> &'static str {
        match self {
            Self::NativeHoverTracking => "native_hover_tracking",
            Self::MissingAddressedApi => "addressed_event_api_unavailable",
            Self::KeyboardSemantics => "native_keyboard_semantics",
            Self::KeyboardFocusAbsent => "background_key_window_absent",
        }
    }
}

/// Foreground ownership is already proven by the execution scope; only keyboard
/// transport changes. Pointer operations keep their addressed route where supported.
pub(super) fn owned_foreground_keyboard(operation: &str, owned: bool) -> bool {
    owned && matches!(operation, "type_text" | "keypress")
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

/// Only established capability limits can start a script's foreground borrow.
/// Pointer transport remains independent: later supported input stays addressed.
pub(super) fn allows_activation(reason: &str) -> bool {
    matches!(
        reason,
        "explicit_focus"
            | "native_keyboard_semantics"
            | "native_hover_tracking"
            | "addressed_event_api_unavailable"
            | "associated_process_keyboard"
            | "native_modal_keyboard"
            | "background_key_window_absent"
            | "windowless_application"
            | "native_open_panel_click"
            | "semantic_text_focus"
            | "native_modal_press"
            | "native_menu_press"
    )
}

pub(super) fn foreground_plan<'a>(
    reply: &'a serde_json::Value,
    operation: &str,
) -> Option<&'a str> {
    let plan = reply.get("foreground_plan")?;
    if reply["dispatch"] != "not_dispatched"
        || reply["evidence"] != "unverified"
        || reply.get("error").is_some()
        || reply.get("cleanup_error").is_some()
        || reply.get("focus_preparation").is_some()
        || reply.get("interruption").is_some()
        || plan["visual"].as_bool()? != visual_focus(operation)
    {
        return None;
    }
    let reason = plan["reason"].as_str()?;
    let keyboard = matches!(operation, "keypress" | "type_text");
    let valid = match reason {
        "explicit_focus" => matches!(operation, "focus" | "focus_app"),
        "windowless_application" => operation == "keypress_app",
        "background_key_window_absent" => keyboard,
        "native_hover_tracking" => operation == "move",
        "native_open_panel_click" => operation == "click",
        "semantic_text_focus" | "native_modal_press" | "native_menu_press" => {
            operation == "press" && plan["visual"] == false
        }
        "addressed_event_api_unavailable" => visual_focus(operation) || keyboard,
        "native_modal_keyboard" | "associated_process_keyboard" => keyboard,
        "native_keyboard_semantics" => {
            (operation == "keypress"
                && matches!(
                    plan["capability_detail"].as_str(),
                    Some(
                        "chromium_command_chord"
                            | "native_modal_command_chord"
                            | "native_menu_disabled"
                            | "native_menu_window_unbound"
                            | "native_menu_unavailable"
                    )
                ))
                || operation == "type_text"
                    && matches!(
                        plan["capability_detail"].as_str(),
                        Some("chromium_supplementary_unicode" | "appkit_multiline_text")
                    )
        }
        _ => false,
    };
    valid.then_some(reason)
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
        "background_key_window_absent" => Some(PhysicalReason::KeyboardFocusAbsent),
        "addressed_event_api_unavailable" => Some(PhysicalReason::MissingAddressedApi),
        "native_keyboard_semantics"
            if matches!(
                result["capability_detail"].as_str(),
                Some(
                    "chromium_command_chord"
                        | "native_modal_command_chord"
                        | "native_menu_disabled"
                        | "native_menu_window_unbound"
                        | "native_menu_unavailable"
                        | "chromium_supplementary_unicode"
                        | "appkit_multiline_text"
                )
            ) =>
        {
            Some(PhysicalReason::KeyboardSemantics)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn owned_scope_uses_foreground_keyboard_without_broadening_other_actions() {
        for op in [
            "keypress",
            "type_text",
            "press",
            "click",
            "move",
            "drag",
            "scroll",
            "focus",
        ] {
            assert_eq!(
                super::owned_foreground_keyboard(op, true),
                matches!(op, "keypress" | "type_text")
            );
            assert!(!super::owned_foreground_keyboard(op, false));
        }
        assert!(!super::allows_activation("owned_foreground_keyboard"));
    }

    use super::*;
    use serde_json::json;
    #[test]
    fn absent_background_key_window_and_windowless_shortcuts_have_exact_plans() {
        for (operation, reason) in [
            ("keypress", "background_key_window_absent"),
            ("type_text", "background_key_window_absent"),
            ("keypress_app", "windowless_application"),
            ("focus_app", "explicit_focus"),
        ] {
            let mut reply = json!({"dispatch":"not_dispatched","evidence":"unverified",
                "foreground_plan":{"reason":reason,"visual":false}});
            assert_eq!(foreground_plan(&reply, operation), Some(reason));
            assert!(allows_activation(reason));
            reply["dispatch"] = json!("unknown");
            assert_eq!(foreground_plan(&reply, operation), None);
        }
        let absent = json!({"dispatch":"not_dispatched","fallback_allowed":true,
            "error":{"code":"background_unavailable"},"capability_evidence":"background_key_window_absent"});
        assert_eq!(
            fallback_reason(&absent),
            Some(PhysicalReason::KeyboardFocusAbsent)
        );
    }

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
    fn only_established_capability_limits_authorize_activation() {
        for reason in [
            "unknown",
            "ax_unavailable",
            "modal_state_unproven",
            "keyboard_focus_requires_activation",
            "associated_surface_requires_foreground",
        ] {
            assert!(!allows_activation(reason), "{reason}");
        }
        for reason in [
            "explicit_focus",
            "native_keyboard_semantics",
            "native_hover_tracking",
            "addressed_event_api_unavailable",
            "associated_process_keyboard",
            "native_modal_keyboard",
        ] {
            assert!(allows_activation(reason), "{reason}");
        }
    }

    #[test]
    fn app_menu_preparation_only_authorizes_one_undispatched_semantic_press() {
        let mut reply = json!({"dispatch":"not_dispatched","evidence":"unverified",
            "foreground_plan":{"reason":"native_menu_press","visual":false}});
        assert!(allows_activation("native_menu_press"));
        assert_eq!(foreground_plan(&reply, "press"), Some("native_menu_press"));
        for operation in ["click", "move", "keypress", "type_text", "focus"] {
            assert_eq!(foreground_plan(&reply, operation), None, "{operation}");
        }
        for dispatch in ["dispatched", "unknown"] {
            reply["dispatch"] = json!(dispatch);
            assert_eq!(foreground_plan(&reply, "press"), None);
        }
    }

    #[test]
    fn foreground_plans_require_clean_exact_operation_capability() {
        let base = json!({"dispatch":"not_dispatched","evidence":"unverified",
            "foreground_plan":{"reason":"native_hover_tracking","visual":true}});
        assert_eq!(
            foreground_plan(&base, "move"),
            Some("native_hover_tracking")
        );
        assert_eq!(foreground_plan(&base, "click"), None);
        for key in [
            "error",
            "cleanup_error",
            "focus_preparation",
            "interruption",
        ] {
            let mut changed = base.clone();
            changed[key] = json!({});
            assert_eq!(foreground_plan(&changed, "move"), None, "{key}");
        }
        for dispatch in ["unknown", "dispatched"] {
            let mut changed = base.clone();
            changed["dispatch"] = json!(dispatch);
            assert_eq!(foreground_plan(&changed, "move"), None);
        }
        let mut keyboard = json!({"dispatch":"not_dispatched","evidence":"unverified",
            "foreground_plan":{"reason":"native_keyboard_semantics","visual":false}});
        assert_eq!(foreground_plan(&keyboard, "keypress"), None);
        keyboard["foreground_plan"]["capability_detail"] = json!("chromium_supplementary_unicode");
        assert_eq!(
            foreground_plan(&keyboard, "type_text"),
            Some("native_keyboard_semantics")
        );
        assert_eq!(foreground_plan(&keyboard, "move"), None);
        keyboard["foreground_plan"]["reason"] = json!("addressed_event_api_unavailable");
        assert_eq!(
            foreground_plan(&keyboard, "keypress"),
            Some("addressed_event_api_unavailable")
        );
        assert_eq!(foreground_plan(&keyboard, "press"), None);
        keyboard["foreground_plan"]["reason"] = json!("modal_state_unproven");
        assert_eq!(foreground_plan(&keyboard, "keypress"), None);
    }

    #[test]
    fn keyboard_fallback_requires_a_known_bounded_capability_detail() {
        let mut result = json!({"dispatch":"not_dispatched", "fallback_allowed":true,
            "error":{"code":"background_unavailable"}, "capability_evidence":"native_keyboard_semantics"});
        for detail in [
            serde_json::Value::Null,
            json!("generic_command"),
            json!(true),
        ] {
            result["capability_detail"] = detail;
            assert_eq!(fallback_reason(&result), None);
        }
        for detail in [
            "chromium_command_chord",
            "native_modal_command_chord",
            "native_menu_disabled",
            "native_menu_window_unbound",
            "native_menu_unavailable",
            "chromium_supplementary_unicode",
            "appkit_multiline_text",
        ] {
            result["capability_detail"] = json!(detail);
            assert_eq!(
                fallback_reason(&result),
                Some(PhysicalReason::KeyboardSemantics)
            );
        }
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
