//! The paste keystroke, sent as a physical key code so the active IME or keyboard layout cannot turn it
//! into another character.

use std::time::{Duration, Instant};

use enigo::{Direction, Enigo, Key, Keyboard, Settings};

pub fn send_paste() -> Result<(), String> {
    let settings = Settings { open_prompt_to_get_permissions: false, ..Settings::default() };
    let mut enigo = Enigo::new(&settings).map_err(|e| e.to_string())?;
    let modifier = if cfg!(target_os = "macos") { Key::Meta } else { Key::Control };
    enigo.key(modifier, Direction::Press).map_err(|e| e.to_string())?;
    let pressed = press_v(&mut enigo);
    // Always release the modifier, even if V failed
    let released = enigo.key(modifier, Direction::Release).map_err(|e| e.to_string());
    pressed.and(released)
}

#[cfg(target_os = "macos")]
fn press_v(enigo: &mut Enigo) -> Result<(), String> {
    const KVK_ANSI_V: u16 = 9;
    enigo.raw(KVK_ANSI_V, Direction::Click).map_err(|e| e.to_string())
}

#[cfg(not(target_os = "macos"))]
fn press_v(enigo: &mut Enigo) -> Result<(), String> {
    const VK_V: u32 = 0x56;
    enigo.key(Key::Other(VK_V), Direction::Click).map_err(|e| e.to_string())
}

pub async fn wait_for_modifiers_released(timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while modifiers_held() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
}

#[cfg(target_os = "macos")]
fn modifiers_held() -> bool {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceFlagsState(state_id: i32) -> u64;
    }
    const HID_SYSTEM_STATE: i32 = 1;
    const SHIFT: u64 = 0x0002_0000;
    const CONTROL: u64 = 0x0004_0000;
    const OPTION: u64 = 0x0008_0000;
    const COMMAND: u64 = 0x0010_0000;
    // SAFETY: plain query of the current modifier state; no pointers involved
    let flags = unsafe { CGEventSourceFlagsState(HID_SYSTEM_STATE) };
    flags & (SHIFT | CONTROL | OPTION | COMMAND) != 0
}

#[cfg(windows)]
fn modifiers_held() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN]
        .iter()
        // SAFETY: GetAsyncKeyState only reads key state
        .any(|vk| unsafe { GetAsyncKeyState(i32::from(vk.0)) } as u16 & 0x8000 != 0)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn modifiers_held() -> bool {
    false
}
