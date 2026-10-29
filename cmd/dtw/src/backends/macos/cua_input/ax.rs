// Adapted from trycua/cua b38360443960a6540685a9a5eb5407f10fe3d03b.
// Source: libs/cua-driver/rust/crates/platform-macos/src/ax/bindings.rs
// Copyright (c) 2025 Cua AI, Inc. MIT. See docs/background-input-port.md.

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
use core_foundation::{
    array::CFArray,
    base::{CFRelease, CFRetain, CFTypeID, CFTypeRef, TCFType},
    string::{CFString as CFStr, CFStringRef},
};
use std::os::raw::{c_int, c_void};
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
    fn _AXUIElementGetWindow(element: AXUIElementRef, wid: *mut u32) -> AXError;
}
pub unsafe fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef {
    let app = create_application(pid);
    if !app.is_null() {
        AXUIElementSetMessagingTimeout(app, 0.15);
    }
    app
}
pub fn focused_window_id_of_pid(pid: i32) -> Option<u32> {
    unsafe {
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return None;
        }
        let window = copy_element_attr(app, "AXFocusedWindow");
        CFRelease(app as CFTypeRef);
        let window = window?;
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
        let window = window?;
        let window_id = ax_get_window_id(window);
        CFRelease(window as CFTypeRef);
        window_id
    }
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
            let mut found = None;
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
