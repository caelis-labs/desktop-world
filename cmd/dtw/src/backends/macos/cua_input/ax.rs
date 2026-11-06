// Adapted from trycua/cua b38360443960a6540685a9a5eb5407f10fe3d03b.
// Source: libs/cua-driver/rust/crates/platform-macos/src/ax/bindings.rs
// Copyright (c) 2025 Cua AI, Inc. MIT. See docs/background-input-port.md.

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
use core_foundation::{
    array::CFArray,
    base::{CFRelease, CFRetain, CFType, CFTypeID, CFTypeRef, TCFType},
    boolean::CFBoolean,
    number::CFNumber,
    string::{CFString as CFStr, CFStringRef},
};
use std::{
    os::raw::{c_int, c_void},
    time::Duration,
};
#[repr(C)]
pub struct __AXUIElement(c_void);
pub type AXUIElementRef = *mut __AXUIElement;
pub type AXError = c_int;
pub const kAXErrorSuccess: AXError = 0;
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    #[link_name = "AXUIElementCreateApplication"]
    fn create_application(pid: i32) -> AXUIElementRef;
    pub fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    pub fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AXError;
    fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, seconds: f32) -> AXError;
    fn AXUIElementGetTypeID() -> CFTypeID;
    fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut i32) -> AXError;
    fn AXUIElementCopyActionNames(element: AXUIElementRef, names: *mut CFTypeRef) -> AXError;
    fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> AXError;
    fn _AXUIElementGetWindow(element: AXUIElementRef, wid: *mut u32) -> AXError;
}

// The menu port uses the same CUA bindings with retained ownership and explicit
// read failures. An AX timeout must not become a proven absence of a shortcut.
pub(super) struct Element(AXUIElementRef);
impl Drop for Element {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0 as _) }
    }
}
impl Element {
    pub fn application(pid: i32) -> anyhow::Result<Self> {
        let raw = unsafe { AXUIElementCreateApplication(pid) };
        anyhow::ensure!(!raw.is_null(), "AX application is unavailable");
        Ok(Self(raw))
    }
    fn attribute(&self, name: &str) -> anyhow::Result<Option<CFType>> {
        super::control::check_local()?;
        let name = CFStr::new(name);
        let mut raw = std::ptr::null();
        let status =
            unsafe { AXUIElementCopyAttributeValue(self.0, name.as_concrete_TypeRef(), &mut raw) };
        let value = (!raw.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(raw) });
        anyhow::ensure!(
            matches!(status, 0 | -25205 | -25212),
            "AX menu attribute read failed ({status})"
        );
        super::control::check_local()?;
        Ok(value)
    }
    pub fn string(&self, name: &str) -> anyhow::Result<Option<String>> {
        let Some(value) = self.attribute(name)? else {
            return Ok(None);
        };
        anyhow::ensure!(
            value.type_of() == CFStr::type_id(),
            "AX menu attribute is not a string"
        );
        Ok(Some(
            unsafe { CFStr::wrap_under_get_rule(value.as_CFTypeRef() as _) }.to_string(),
        ))
    }
    pub fn number(&self, name: &str) -> anyhow::Result<Option<i64>> {
        let Some(value) = self.attribute(name)? else {
            return Ok(None);
        };
        anyhow::ensure!(
            value.type_of() == CFNumber::type_id(),
            "AX menu attribute is not a number"
        );
        let number = unsafe { CFNumber::wrap_under_get_rule(value.as_CFTypeRef() as _) };
        Ok(number.to_i64())
    }
    pub fn boolean(&self, name: &str) -> anyhow::Result<Option<bool>> {
        let Some(value) = self.attribute(name)? else {
            return Ok(None);
        };
        if value.type_of() == CFBoolean::type_id() {
            return Ok(Some(
                unsafe { CFBoolean::wrap_under_get_rule(value.as_CFTypeRef() as _) }.into(),
            ));
        }
        anyhow::ensure!(
            value.type_of() == CFNumber::type_id(),
            "AX menu attribute is not a boolean"
        );
        Ok(
            unsafe { CFNumber::wrap_under_get_rule(value.as_CFTypeRef() as _) }
                .to_i64()
                .map(|n| n != 0),
        )
    }
    pub fn element(&self, name: &str) -> anyhow::Result<Option<Self>> {
        let Some(value) = self.attribute(name)? else {
            return Ok(None);
        };
        anyhow::ensure!(
            value.type_of() == unsafe { AXUIElementGetTypeID() },
            "AX menu attribute is not an element"
        );
        Ok(Some(unsafe { Self::retain(value.as_CFTypeRef()) }))
    }
    unsafe fn retain(raw: CFTypeRef) -> Self {
        CFRetain(raw);
        AXUIElementSetMessagingTimeout(raw as _, 0.15);
        Self(raw as _)
    }
    pub fn children(&self) -> anyhow::Result<Vec<Self>> {
        let Some(value) = self.attribute("AXChildren")? else {
            return Ok(vec![]);
        };
        anyhow::ensure!(
            value.type_of() == CFArray::<CFTypeRef>::type_id(),
            "AX menu children are not an array"
        );
        let array = unsafe { CFArray::<CFTypeRef>::wrap_under_get_rule(value.as_CFTypeRef() as _) };
        anyhow::ensure!(array.len() <= 2000, "AX menu child limit exceeded");
        let mut out = Vec::new();
        for item in array.iter() {
            anyhow::ensure!(
                unsafe { core_foundation::base::CFGetTypeID(*item) }
                    == unsafe { AXUIElementGetTypeID() },
                "AX menu child is not an element"
            );
            out.push(unsafe { Self::retain(*item) });
        }
        Ok(out)
    }
    pub fn actions(&self) -> anyhow::Result<Vec<String>> {
        super::control::check_local()?;
        let mut raw = std::ptr::null();
        let status = unsafe { AXUIElementCopyActionNames(self.0, &mut raw) };
        let value = (!raw.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(raw) });
        anyhow::ensure!(status == 0, "AX menu actions unavailable ({status})");
        let value = value.ok_or_else(|| anyhow::anyhow!("AX menu actions absent"))?;
        anyhow::ensure!(
            value.type_of() == CFArray::<CFTypeRef>::type_id(),
            "AX menu actions are not an array"
        );
        let array = unsafe { CFArray::<CFTypeRef>::wrap_under_get_rule(value.as_CFTypeRef() as _) };
        let mut out = Vec::new();
        for item in array.iter() {
            anyhow::ensure!(
                unsafe { core_foundation::base::CFGetTypeID(*item) } == CFStr::type_id(),
                "AX menu action is not a string"
            );
            out.push(unsafe { CFStr::wrap_under_get_rule(*item as _) }.to_string());
        }
        Ok(out)
    }
    pub fn window_id(&self) -> Option<u32> {
        unsafe { ax_get_window_id(self.0) }
    }
    pub fn perform(&self, action: &str) -> AXError {
        let action = CFStr::new(action);
        action_with_reply_budget(
            super::control::remaining(),
            |seconds| unsafe { AXUIElementSetMessagingTimeout(self.0, seconds) },
            || unsafe { AXUIElementPerformAction(self.0, action.as_concrete_TypeRef()) },
        )
    }
}

// Modal menu callbacks share the same caller budget as semantic AX presses.
// A shorter private cap can turn an already delivered action into an unknown
// result. Configure one bounded reply; never replay or hide a native error.
fn action_with_reply_budget(
    remaining: Duration,
    configure: impl FnOnce(f32) -> AXError,
    dispatch: impl FnOnce() -> AXError,
) -> AXError {
    let seconds = remaining.as_secs_f32();
    if seconds <= 0.001 {
        return -25204;
    }
    let configured = configure(seconds);
    if configured != kAXErrorSuccess {
        return configured;
    }
    dispatch()
}

#[cfg(test)]
mod action_reply_tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn modal_initialization_can_exceed_two_seconds_without_replay() {
        let budget = Cell::new(0.0);
        let calls = Cell::new(0);
        let result = action_with_reply_budget(
            Duration::from_secs(6),
            |seconds| {
                budget.set(seconds);
                0
            },
            || {
                calls.set(calls.get() + 1);
                if budget.get() > 3.2 {
                    0
                } else {
                    -25204
                }
            },
        );
        assert_eq!(result, 0);
        assert_eq!(calls.get(), 1);
        assert_eq!(budget.get(), 6.0);
    }

    #[test]
    fn short_deadline_and_native_failure_are_preserved() {
        for status in [-25204, -25202] {
            let calls = Cell::new(0);
            let result = action_with_reply_budget(
                Duration::from_millis(75),
                |seconds| {
                    assert!((seconds - 0.075).abs() < 0.00001);
                    0
                },
                || {
                    calls.set(calls.get() + 1);
                    status
                },
            );
            assert_eq!(result, status);
            assert_eq!(calls.get(), 1);
        }
    }

    #[test]
    fn expired_budget_or_configuration_failure_does_not_dispatch() {
        assert_eq!(
            action_with_reply_budget(
                Duration::ZERO,
                |_| panic!("expired budget configured"),
                || panic!("expired action dispatched")
            ),
            -25204
        );
        assert_eq!(
            action_with_reply_budget(
                Duration::from_secs(6),
                |_| -25202,
                || panic!("action dispatched after configuration failed")
            ),
            -25202
        );
    }
}
pub unsafe fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef {
    let app = create_application(pid);
    if !app.is_null() {
        AXUIElementSetMessagingTimeout(app, 0.15);
    }
    app
}
/// Only AXNoValue proves absence. Unsupported attributes, timeouts and invalid
/// elements remain unknown and never authorize foreground borrowing.
pub fn focused_window_absent(pid: i32) -> bool {
    unsafe {
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return false;
        }
        let attr = CFStr::new("AXFocusedWindow");
        let mut value: CFTypeRef = std::ptr::null();
        let status = AXUIElementCopyAttributeValue(app, attr.as_concrete_TypeRef(), &mut value);
        CFRelease(app as CFTypeRef);
        let absent = status == -25212 && value.is_null();
        if !value.is_null() {
            CFRelease(value);
        }
        absent
    }
}

pub fn focused_window_id_of_pid(pid: i32) -> Option<u32> {
    unsafe {
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return None;
        }
        let window = copy_element_attr(app, "AXFocusedWindow");
        CFRelease(app as CFTypeRef);
        let Some(window) = window else {
            return focused_top_level_text_control_id(pid);
        };
        let window_id = ax_get_window_id(window);
        CFRelease(window as CFTypeRef);
        window_id
    }
}

pub fn focused_element_window_id_of_pid(pid: i32) -> Option<u32> {
    unsafe {
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return None;
        }
        let element = copy_element_attr(app, "AXFocusedUIElement");
        CFRelease(app as CFTypeRef);
        let element = element?;
        let window = copy_element_attr(element, "AXWindow");
        CFRelease(element as CFTypeRef);
        let Some(window) = window else {
            return focused_top_level_text_control_id(pid);
        };
        let window_id = ax_get_window_id(window);
        CFRelease(window as CFTypeRef);
        window_id
    }
}

fn top_level_text_control_matches(
    pid: i32,
    owner: Option<i32>,
    parent_owner: Option<i32>,
    role: &str,
    parent_role: &str,
    window: Option<u32>,
) -> bool {
    pid > 0
        && owner == Some(pid)
        && parent_owner == Some(pid)
        && role == "AXTextField"
        && parent_role == "AXApplication"
        && window.is_some_and(|id| id > 0)
}

// A standalone inline editor can be the actual key receiver without publishing
// AXFocusedWindow (Finder). Never override a real focused window: VCL can keep
// stale field focus after its document has become key (CUA-1767).
fn focused_top_level_text_control(pid: i32) -> Option<Element> {
    if !focused_window_absent(pid) {
        return None;
    }
    let app = Element::application(pid).ok()?;
    let control = app.element("AXFocusedUIElement").ok()??;
    let parent = control.element("AXParent").ok()??;
    let owner = |element: &Element| {
        let mut owner = 0;
        (unsafe { AXUIElementGetPid(element.0, &mut owner) } == 0).then_some(owner)
    };
    top_level_text_control_matches(
        pid,
        owner(&control),
        owner(&parent),
        &control.string("AXRole").ok()??,
        &parent.string("AXRole").ok()??,
        control.window_id(),
    )
    .then_some(control)
}

pub fn focused_top_level_text_control_id(pid: i32) -> Option<u32> {
    focused_top_level_text_control(pid)?.window_id()
}

pub unsafe fn copy_element_attr(
    element: AXUIElementRef,
    attr_name: &str,
) -> Option<AXUIElementRef> {
    let attr = CFStr::new(attr_name);
    let mut value: CFTypeRef = std::ptr::null();
    let err = AXUIElementCopyAttributeValue(element, attr.as_concrete_TypeRef(), &mut value);
    if err != kAXErrorSuccess || value.is_null() {
        return None;
    }
    if core_foundation::base::CFGetTypeID(value) != AXUIElementGetTypeID() {
        CFRelease(value);
        return None;
    }
    AXUIElementSetMessagingTimeout(value as AXUIElementRef, 0.15);
    Some(value as AXUIElementRef)
}

pub unsafe fn ax_get_window_id(element: AXUIElementRef) -> Option<u32> {
    let mut wid: u32 = 0;
    let err = _AXUIElementGetWindow(element, &mut wid);
    if err == kAXErrorSuccess && wid != 0 {
        Some(wid)
    } else {
        None
    }
}

pub unsafe fn copy_ax_windows(element: AXUIElementRef) -> Vec<AXUIElementRef> {
    let attr = CFStr::new("AXWindows");
    let mut value: CFTypeRef = std::ptr::null();
    let err = AXUIElementCopyAttributeValue(element, attr.as_concrete_TypeRef(), &mut value);
    if err != kAXErrorSuccess || value.is_null() {
        return vec![];
    }
    let cf_array_type_id = CFArray::<CFTypeRef>::type_id();
    if core_foundation::base::CFGetTypeID(value) != cf_array_type_id {
        CFRelease(value);
        return vec![];
    }
    let arr = CFArray::<CFTypeRef>::wrap_under_create_rule(value as _);
    let ax_type_id = AXUIElementGetTypeID();
    (0..arr.len())
        .filter_map(|i| {
            let item = *arr.get(i)?;
            if core_foundation::base::CFGetTypeID(item) == ax_type_id {
                CFRetain(item);
                AXUIElementSetMessagingTimeout(item as AXUIElementRef, 0.15);
                Some(item as AXUIElementRef)
            } else {
                None
            }
        })
        .collect()
}

pub struct WindowEvidence {
    element: AXUIElementRef,
    id: u32,
}
impl WindowEvidence {
    pub fn retain(pid: i32, id: u32) -> Option<Self> {
        unsafe {
            let app = AXUIElementCreateApplication(pid);
            if app.is_null() {
                return None;
            }
            // AXFocusedWindow can be a real sheet absent from AXWindows.
            // Retain that exact CUA focus source before the ordinary inventory
            // fallback; enumerating only top-level windows rejected valid input.
            let mut found = copy_element_attr(app, "AXFocusedWindow").and_then(|window| {
                if ax_get_window_id(window) == Some(id) {
                    Some(Self {
                        element: window,
                        id,
                    })
                } else {
                    CFRelease(window as _);
                    None
                }
            });
            if found.is_some() {
                CFRelease(app as _);
                return found;
            }
            if let Some(control) = focused_top_level_text_control(pid)
                .filter(|control| control.window_id() == Some(id))
            {
                CFRetain(control.0 as _);
                CFRelease(app as _);
                return Some(Self {
                    element: control.0,
                    id,
                });
            }
            for window in copy_ax_windows(app) {
                if found.is_none() && ax_get_window_id(window) == Some(id) {
                    found = Some(Self {
                        element: window,
                        id,
                    });
                } else {
                    CFRelease(window as _);
                }
            }
            CFRelease(app as _);
            found
        }
    }
    pub fn still_matches(&self) -> bool {
        unsafe {
            let attribute = CFStr::new("AXRole");
            let mut value: CFTypeRef = std::ptr::null();
            let status = AXUIElementCopyAttributeValue(
                self.element,
                attribute.as_concrete_TypeRef(),
                &mut value,
            );
            let live = status == 0 && !value.is_null();
            if !value.is_null() {
                CFRelease(value);
            }
            live && ax_get_window_id(self.element) == Some(self.id)
        }
    }
}
impl Drop for WindowEvidence {
    fn drop(&mut self) {
        unsafe { CFRelease(self.element as _) }
    }
}

#[cfg(test)]
mod standalone_text_control_tests {
    use super::top_level_text_control_matches as accepts;
    #[test]
    fn standalone_editor_requires_exact_local_application_and_native_surface() {
        assert!(accepts(
            10,
            Some(10),
            Some(10),
            "AXTextField",
            "AXApplication",
            Some(100)
        ));
        for (pid, owner, parent, role, parent_role, window) in [
            (
                0,
                Some(0),
                Some(0),
                "AXTextField",
                "AXApplication",
                Some(100),
            ),
            (
                10,
                None,
                Some(10),
                "AXTextField",
                "AXApplication",
                Some(100),
            ),
            (
                10,
                Some(20),
                Some(10),
                "AXTextField",
                "AXApplication",
                Some(100),
            ),
            (
                10,
                Some(10),
                Some(20),
                "AXTextField",
                "AXApplication",
                Some(100),
            ),
            (
                10,
                Some(10),
                Some(10),
                "AXButton",
                "AXApplication",
                Some(100),
            ),
            (10, Some(10), Some(10), "AXTextField", "AXWindow", Some(100)),
            (10, Some(10), Some(10), "AXTextField", "AXApplication", None),
            (
                10,
                Some(10),
                Some(10),
                "AXTextField",
                "AXApplication",
                Some(0),
            ),
        ] {
            assert!(!accepts(pid, owner, parent, role, parent_role, window));
        }
    }
}
