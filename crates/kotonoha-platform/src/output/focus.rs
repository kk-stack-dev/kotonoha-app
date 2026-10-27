//! Whether the focused UI element is a password field. Unknown counts as "not a password field",
//! since the text is on the clipboard either way.

#[cfg(target_os = "macos")]
pub fn focused_field_is_secure() -> bool {
    use std::ffi::c_void;

    use core_foundation::base::{CFType, CFTypeRef, TCFType};
    use core_foundation::string::{CFString, CFStringRef};

    type AxElement = *const c_void;
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateSystemWide() -> AxElement;
        fn AXUIElementSetMessagingTimeout(element: AxElement, seconds: f32) -> i32;
        fn AXUIElementCopyAttributeValue(element: AxElement, attribute: CFStringRef, value: *mut CFTypeRef) -> i32;
    }

    fn copy_attribute(element: AxElement, name: &'static str) -> Option<CFType> {
        let attribute = CFString::from_static_string(name);
        let mut value: CFTypeRef = std::ptr::null();
        // SAFETY: `element` is a live AXUIElement and `value` receives a +1 reference on success
        let status = unsafe { AXUIElementCopyAttributeValue(element, attribute.as_concrete_TypeRef(), &mut value) };
        // SAFETY: on success the copied value is owned by us (create rule)
        (status == 0 && !value.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(value) })
    }

    // SAFETY: returns a new system-wide element owned by us
    let system = unsafe { CFType::wrap_under_create_rule(AXUIElementCreateSystemWide()) };
    // A hung app must not stall us for the default six seconds
    // SAFETY: valid element
    unsafe { AXUIElementSetMessagingTimeout(system.as_CFTypeRef(), 1.0) };
    let Some(focused) = copy_attribute(system.as_CFTypeRef(), "AXFocusedUIElement") else {
        return false;
    };
    ["AXSubrole", "AXRole"].iter().any(|name| {
        copy_attribute(focused.as_CFTypeRef(), name)
            .and_then(|v| v.downcast::<CFString>())
            .is_some_and(|s| s == "AXSecureTextField")
    })
}

#[cfg(windows)]
pub fn focused_field_is_secure() -> bool {
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};

    // SAFETY: COM is initialised for this (blocking pool) thread before use; failures are treated as "unknown"
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(automation) = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) else {
            return false;
        };
        automation.GetFocusedElement().and_then(|e| e.CurrentIsPassword()).is_ok_and(|b| b.as_bool())
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn focused_field_is_secure() -> bool {
    false
}
