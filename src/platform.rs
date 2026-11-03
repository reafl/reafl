//! Small platform shims the UI needs but egui does not expose.
//!
//! Only the OS cursor is handled here: egui can hide the pointer, but returning
//! it to where a relative drag started needs the native API.

/// Reads the OS cursor position in screen pixels.
#[cfg(windows)]
pub fn cursor_position() -> Option<(i32, i32)> {
    // SAFETY: plain Win32 query with no pointer aliasing.
    unsafe {
        let mut point = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
        if windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point) == 0 {
            None
        } else {
            Some((point.x, point.y))
        }
    }
}

#[cfg(not(windows))]
pub fn cursor_position() -> Option<(i32, i32)> {
    None
}

/// Moves the OS cursor to a screen position.
#[cfg(windows)]
pub fn set_cursor_position(x: i32, y: i32) {
    // SAFETY: plain Win32 call with scalar arguments.
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos(x, y);
    }
}

#[cfg(not(windows))]
pub fn set_cursor_position(_x: i32, _y: i32) {}
