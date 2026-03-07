//! Thin wrapper around macOS Accessibility API (AXUIElement).
//!
//! Provides safe Rust types for querying and interacting with UI elements
//! in any running application. Requires Accessibility permission in
//! System Settings > Privacy & Security > Accessibility.

#![allow(non_upper_case_globals, dead_code)]

use std::ffi::c_void;
use std::fmt;
use std::ptr;

// ---------------------------------------------------------------------------
// Core Foundation + Accessibility FFI bindings
// ---------------------------------------------------------------------------

// Core Foundation opaque types
type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFArrayRef = *const c_void;
type CFBooleanRef = *const c_void;
type CFIndex = isize;
type AXUIElementRef = CFTypeRef;
type AXError = i32;
type Boolean = u8;
type Pid = i32;

// AXError codes
const kAXErrorSuccess: AXError = 0;
#[allow(dead_code)]
const kAXErrorNoValue: AXError = -25212;

// CFBoolean constants
extern "C" {
    static kCFBooleanTrue: CFBooleanRef;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: Pid) -> AXUIElementRef;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AXError;
    fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> AXError;
    fn AXUIElementCopyElementAtPosition(
        application: AXUIElementRef,
        x: f32,
        y: f32,
        element: *mut AXUIElementRef,
    ) -> AXError;
    fn AXIsProcessTrusted() -> Boolean;
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> Boolean;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithCString(
        allocator: CFTypeRef,
        cstr: *const u8,
        encoding: u32,
    ) -> CFStringRef;
    fn CFStringGetCString(
        string: CFStringRef,
        buffer: *mut u8,
        buffer_size: CFIndex,
        encoding: u32,
    ) -> Boolean;
    fn CFStringGetLength(string: CFStringRef) -> CFIndex;
    fn CFArrayGetCount(array: CFArrayRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(array: CFArrayRef, index: CFIndex) -> CFTypeRef;
    fn CFGetTypeID(cf: CFTypeRef) -> u64;
    fn CFStringGetTypeID() -> u64;
    fn CFArrayGetTypeID() -> u64;
    fn CFBooleanGetTypeID() -> u64;
    fn CFBooleanGetValue(boolean: CFBooleanRef) -> Boolean;
    fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    fn CFRelease(cf: CFTypeRef);
    fn CFNumberGetTypeID() -> u64;
    fn CFNumberGetValue(number: CFTypeRef, the_type: i32, value_ptr: *mut c_void) -> Boolean;
    fn CFDictionaryCreate(
        allocator: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        count: CFIndex,
        key_callbacks: CFTypeRef,
        value_callbacks: CFTypeRef,
    ) -> CFTypeRef;
}

// AXValue functions (for position/size extraction)
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXValueGetTypeID() -> u64;
    fn AXValueGetValue(value: CFTypeRef, value_type: i32, value_ptr: *mut c_void) -> Boolean;
}

// AXValue types
const kAXValueCGPointType: i32 = 1;
const kAXValueCGSizeType: i32 = 2;

// CFNumber types
const kCFNumberFloat64Type: i32 = 13;

// CFString encoding
const kCFStringEncodingUTF8: u32 = 0x08000100;

// kAXTrustedCheckOptionPrompt key
extern "C" {
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Create a CFString from a Rust &str. Caller must CFRelease.
fn cfstring(s: &str) -> CFStringRef {
    let cstr = std::ffi::CString::new(s).unwrap();
    unsafe { CFStringCreateWithCString(ptr::null(), cstr.as_ptr() as *const u8, kCFStringEncodingUTF8) }
}

/// Extract a Rust String from a CFStringRef.
fn cfstring_to_string(cf: CFStringRef) -> Option<String> {
    if cf.is_null() {
        return None;
    }
    unsafe {
        let len = CFStringGetLength(cf);
        // UTF-8 can be up to 4x the UTF-16 length
        let buf_size = (len * 4 + 1) as usize;
        let mut buf = vec![0u8; buf_size];
        if CFStringGetCString(cf, buf.as_mut_ptr(), buf_size as CFIndex, kCFStringEncodingUTF8) != 0
        {
            let nul_pos = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            Some(String::from_utf8_lossy(&buf[..nul_pos]).to_string())
        } else {
            None
        }
    }
}

/// Get a string attribute from an AX element.
fn ax_get_string(element: AXUIElementRef, attr: &str) -> Option<String> {
    unsafe {
        let attr_cf = cfstring(attr);
        let mut value: CFTypeRef = ptr::null();
        let err = AXUIElementCopyAttributeValue(element, attr_cf, &mut value);
        CFRelease(attr_cf);
        if err != kAXErrorSuccess || value.is_null() {
            return None;
        }
        let result = if CFGetTypeID(value) == CFStringGetTypeID() {
            cfstring_to_string(value)
        } else {
            None
        };
        CFRelease(value);
        result
    }
}

/// Get a boolean attribute from an AX element.
fn ax_get_bool(element: AXUIElementRef, attr: &str) -> Option<bool> {
    unsafe {
        let attr_cf = cfstring(attr);
        let mut value: CFTypeRef = ptr::null();
        let err = AXUIElementCopyAttributeValue(element, attr_cf, &mut value);
        CFRelease(attr_cf);
        if err != kAXErrorSuccess || value.is_null() {
            return None;
        }
        let result = if CFGetTypeID(value) == CFBooleanGetTypeID() {
            Some(CFBooleanGetValue(value) != 0)
        } else {
            None
        };
        CFRelease(value);
        result
    }
}

/// Get a number attribute from an AX element (as f64).
fn ax_get_number(element: AXUIElementRef, attr: &str) -> Option<f64> {
    unsafe {
        let attr_cf = cfstring(attr);
        let mut value: CFTypeRef = ptr::null();
        let err = AXUIElementCopyAttributeValue(element, attr_cf, &mut value);
        CFRelease(attr_cf);
        if err != kAXErrorSuccess || value.is_null() {
            return None;
        }
        let result = if CFGetTypeID(value) == CFNumberGetTypeID() {
            let mut num: f64 = 0.0;
            if CFNumberGetValue(value, kCFNumberFloat64Type, &mut num as *mut f64 as *mut c_void)
                != 0
            {
                Some(num)
            } else {
                None
            }
        } else {
            None
        };
        CFRelease(value);
        result
    }
}

/// Get position (CGPoint) from an AX element.
fn ax_get_position(element: AXUIElementRef) -> Option<(f64, f64)> {
    unsafe {
        let attr_cf = cfstring("AXPosition");
        let mut value: CFTypeRef = ptr::null();
        let err = AXUIElementCopyAttributeValue(element, attr_cf, &mut value);
        CFRelease(attr_cf);
        if err != kAXErrorSuccess || value.is_null() {
            return None;
        }
        let result = if CFGetTypeID(value) == AXValueGetTypeID() {
            #[repr(C)]
            struct CGPoint {
                x: f64,
                y: f64,
            }
            let mut point = CGPoint { x: 0.0, y: 0.0 };
            if AXValueGetValue(
                value,
                kAXValueCGPointType,
                &mut point as *mut CGPoint as *mut c_void,
            ) != 0
            {
                Some((point.x, point.y))
            } else {
                None
            }
        } else {
            None
        };
        CFRelease(value);
        result
    }
}

/// Get size (CGSize) from an AX element.
fn ax_get_size(element: AXUIElementRef) -> Option<(f64, f64)> {
    unsafe {
        let attr_cf = cfstring("AXSize");
        let mut value: CFTypeRef = ptr::null();
        let err = AXUIElementCopyAttributeValue(element, attr_cf, &mut value);
        CFRelease(attr_cf);
        if err != kAXErrorSuccess || value.is_null() {
            return None;
        }
        let result = if CFGetTypeID(value) == AXValueGetTypeID() {
            #[repr(C)]
            struct CGSize {
                width: f64,
                height: f64,
            }
            let mut size = CGSize {
                width: 0.0,
                height: 0.0,
            };
            if AXValueGetValue(
                value,
                kAXValueCGSizeType,
                &mut size as *mut CGSize as *mut c_void,
            ) != 0
            {
                Some((size.width, size.height))
            } else {
                None
            }
        } else {
            None
        };
        CFRelease(value);
        result
    }
}

/// Get children array from an AX element. Returns retained refs — caller must CFRelease each.
fn ax_get_children(element: AXUIElementRef) -> Vec<AXUIElementRef> {
    unsafe {
        let attr_cf = cfstring("AXChildren");
        let mut value: CFTypeRef = ptr::null();
        let err = AXUIElementCopyAttributeValue(element, attr_cf, &mut value);
        CFRelease(attr_cf);
        if err != kAXErrorSuccess || value.is_null() {
            return vec![];
        }
        if CFGetTypeID(value) != CFArrayGetTypeID() {
            CFRelease(value);
            return vec![];
        }
        let count = CFArrayGetCount(value);
        let mut children = Vec::with_capacity(count as usize);
        for i in 0..count {
            let child = CFArrayGetValueAtIndex(value, i);
            if !child.is_null() {
                CFRetain(child);
                children.push(child);
            }
        }
        CFRelease(value);
        children
    }
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Describes a single UI element.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UIElement {
    pub role: String,
    pub title: Option<String>,
    pub value: Option<String>,
    pub description: Option<String>,
    pub position: Option<(f64, f64)>,
    pub size: Option<(f64, f64)>,
    pub focused: Option<bool>,
    pub enabled: Option<bool>,
    pub children_count: usize,
    /// Flat index in the tree (for click_element / set_value referencing)
    pub index: usize,
}

impl fmt::Display for UIElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.index, self.role)?;
        if let Some(ref t) = self.title {
            if !t.is_empty() {
                write!(f, " title=\"{t}\"")?;
            }
        }
        if let Some(ref v) = self.value {
            if !v.is_empty() {
                let preview = if v.len() > 40 {
                    format!("{}...", &v[..37])
                } else {
                    v.clone()
                };
                write!(f, " value=\"{preview}\"")?;
            }
        }
        if let Some(ref d) = self.description {
            if !d.is_empty() {
                write!(f, " desc=\"{d}\"")?;
            }
        }
        if let Some((x, y)) = self.position {
            write!(f, " @({x:.0},{y:.0})")?;
        }
        if let Some((w, h)) = self.size {
            write!(f, " {w:.0}x{h:.0}")?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Check if the current process has Accessibility permission.
pub fn is_trusted() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}

/// Check if trusted, and optionally prompt the user to grant permission.
pub fn is_trusted_with_prompt(prompt: bool) -> bool {
    unsafe {
        if prompt {
            let key = kAXTrustedCheckOptionPrompt;
            let value: CFTypeRef = if prompt {
                kCFBooleanTrue as CFTypeRef
            } else {
                ptr::null()
            };
            let options = CFDictionaryCreate(
                ptr::null(),
                &key as *const CFStringRef as *const CFTypeRef,
                &value,
                1,
                ptr::null(),
                ptr::null(),
            );
            let result = AXIsProcessTrustedWithOptions(options);
            CFRelease(options);
            result != 0
        } else {
            AXIsProcessTrusted() != 0
        }
    }
}

/// Get the PID of the frontmost application.
pub fn frontmost_app_pid() -> Option<Pid> {
    // Use NSWorkspace via osascript — simplest approach without AppKit binding
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(
            r#"tell application "System Events" to get unix id of first process whose frontmost is true"#,
        )
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    s.parse::<Pid>().ok()
}

/// Get PID of a named application.
pub fn app_pid(app_name: &str) -> Option<Pid> {
    let script = format!(
        r#"tell application "System Events" to get unix id of process "{}""#,
        app_name.replace('"', "\\\"")
    );
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    s.parse::<Pid>().ok()
}

/// Build a flat list of UI elements for an application (by PID), up to `max_depth` levels deep.
pub fn get_ui_tree(pid: Pid, max_depth: usize) -> Vec<UIElement> {
    let app = unsafe { AXUIElementCreateApplication(pid) };
    if app.is_null() {
        return vec![];
    }
    let mut elements = Vec::new();
    collect_elements(app, 0, max_depth, &mut elements);
    unsafe { CFRelease(app) };
    elements
}

/// Recursive element collector.
fn collect_elements(
    element: AXUIElementRef,
    depth: usize,
    max_depth: usize,
    out: &mut Vec<UIElement>,
) {
    if depth > max_depth {
        return;
    }
    // Cap total elements to prevent runaway traversal
    if out.len() >= 500 {
        return;
    }

    let role = ax_get_string(element, "AXRole").unwrap_or_default();
    let title = ax_get_string(element, "AXTitle");
    let value = ax_get_string(element, "AXValue");
    let description = ax_get_string(element, "AXDescription");
    let position = ax_get_position(element);
    let size = ax_get_size(element);
    let focused = ax_get_bool(element, "AXFocused");
    let enabled = ax_get_bool(element, "AXEnabled");

    let children = ax_get_children(element);
    let children_count = children.len();

    let index = out.len();
    out.push(UIElement {
        role,
        title,
        value,
        description,
        position,
        size,
        focused,
        enabled,
        children_count,
        index,
    });

    for child in &children {
        collect_elements(*child, depth + 1, max_depth, out);
    }

    // Release children
    for child in children {
        unsafe { CFRelease(child) };
    }
}

/// Find elements matching a query (role, title, or value contains the query string).
pub fn find_elements(pid: Pid, query: &str, max_depth: usize) -> Vec<UIElement> {
    let all = get_ui_tree(pid, max_depth);
    let q = query.to_ascii_lowercase();
    all.into_iter()
        .filter(|el| {
            el.role.to_ascii_lowercase().contains(&q)
                || el
                    .title
                    .as_ref()
                    .is_some_and(|t| t.to_ascii_lowercase().contains(&q))
                || el
                    .value
                    .as_ref()
                    .is_some_and(|v| v.to_ascii_lowercase().contains(&q))
                || el
                    .description
                    .as_ref()
                    .is_some_and(|d| d.to_ascii_lowercase().contains(&q))
        })
        .collect()
}

/// Perform the "AXPress" action on the element at the given tree index.
pub fn press_element(pid: Pid, index: usize) -> Result<(), String> {
    let app = unsafe { AXUIElementCreateApplication(pid) };
    if app.is_null() {
        return Err("Could not create AX element for PID".into());
    }

    let target = find_element_by_index(app, index);
    unsafe { CFRelease(app) };

    match target {
        Some(el) => {
            let action = cfstring("AXPress");
            let err = unsafe { AXUIElementPerformAction(el, action) };
            unsafe {
                CFRelease(action);
                CFRelease(el);
            }
            if err == kAXErrorSuccess {
                Ok(())
            } else {
                Err(format!("AXPress failed with error code {err}"))
            }
        }
        None => Err(format!("Element at index {index} not found")),
    }
}

/// Set the AXValue attribute on the element at the given tree index.
pub fn set_element_value(pid: Pid, index: usize, new_value: &str) -> Result<(), String> {
    let app = unsafe { AXUIElementCreateApplication(pid) };
    if app.is_null() {
        return Err("Could not create AX element for PID".into());
    }

    let target = find_element_by_index(app, index);
    unsafe { CFRelease(app) };

    match target {
        Some(el) => {
            // First focus the element
            let focused_attr = cfstring("AXFocused");
            unsafe {
                AXUIElementSetAttributeValue(el, focused_attr, kCFBooleanTrue as CFTypeRef);
                CFRelease(focused_attr);
            }

            let attr = cfstring("AXValue");
            let val = cfstring(new_value);
            let err = unsafe { AXUIElementSetAttributeValue(el, attr, val) };
            unsafe {
                CFRelease(attr);
                CFRelease(val);
                CFRelease(el);
            }
            if err == kAXErrorSuccess {
                Ok(())
            } else {
                Err(format!("SetAttributeValue failed with error code {err}"))
            }
        }
        None => Err(format!("Element at index {index} not found")),
    }
}

/// Get the element at a screen position for a given app.
pub fn element_at_position(pid: Pid, x: f32, y: f32) -> Option<UIElement> {
    let app = unsafe { AXUIElementCreateApplication(pid) };
    if app.is_null() {
        return None;
    }
    let mut element: AXUIElementRef = ptr::null();
    let err = unsafe { AXUIElementCopyElementAtPosition(app, x, y, &mut element) };
    unsafe { CFRelease(app) };

    if err != kAXErrorSuccess || element.is_null() {
        return None;
    }

    let role = ax_get_string(element, "AXRole").unwrap_or_default();
    let title = ax_get_string(element, "AXTitle");
    let value = ax_get_string(element, "AXValue");
    let description = ax_get_string(element, "AXDescription");
    let position = ax_get_position(element);
    let size = ax_get_size(element);
    let focused = ax_get_bool(element, "AXFocused");
    let enabled = ax_get_bool(element, "AXEnabled");
    let children = ax_get_children(element);
    let children_count = children.len();
    for c in children {
        unsafe { CFRelease(c) };
    }
    unsafe { CFRelease(element) };

    Some(UIElement {
        role,
        title,
        value,
        description,
        position,
        size,
        focused,
        enabled,
        children_count,
        index: 0,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Walk the tree to find the element at a specific flat index.
fn find_element_by_index(root: AXUIElementRef, target_index: usize) -> Option<AXUIElementRef> {
    let mut counter = 0usize;
    find_recursive(root, target_index, &mut counter)
}

fn find_recursive(
    element: AXUIElementRef,
    target: usize,
    counter: &mut usize,
) -> Option<AXUIElementRef> {
    if *counter == target {
        unsafe { CFRetain(element) };
        return Some(element);
    }
    *counter += 1;

    let children = ax_get_children(element);
    let mut result = None;
    for child in &children {
        if let Some(found) = find_recursive(*child, target, counter) {
            result = Some(found);
            break;
        }
    }
    for child in children {
        unsafe { CFRelease(child) };
    }
    result
}
