//! Permission to send keystrokes. macOS requires Accessibility access; Windows needs nothing.

#[cfg(target_os = "macos")]
mod imp {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::string::{CFString, CFStringRef};

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
        static kAXTrustedCheckOptionPrompt: CFStringRef;
    }

    /// An ad-hoc signed build gets a new identity on every rebuild, and macOS then treats the old grant as
    /// belonging to another app: System Settings still shows it on, but this returns false.
    pub fn has_input_permission() -> bool {
        // SAFETY: no arguments; reads this process's trust state
        unsafe { AXIsProcessTrusted() }
    }

    /// Shows the system prompt that links to the Accessibility settings.
    pub fn request_input_permission() {
        // SAFETY: kAXTrustedCheckOptionPrompt is a constant CFString provided by the framework
        let key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
        let options = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::true_value())]);
        // SAFETY: `options` is a valid CFDictionary for the duration of the call
        unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) };
    }

    pub fn open_input_permission_settings() {
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn();
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn has_input_permission() -> bool {
        true
    }
    pub fn request_input_permission() {}
    pub fn open_input_permission_settings() {}
}

pub use imp::{has_input_permission, open_input_permission_settings, request_input_permission};
