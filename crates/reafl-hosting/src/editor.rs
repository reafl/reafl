//! Native child window used to embed a plug-in editor inside the DAW window.
//!
//! egui paints into one OpenGL surface, so an editor drawn by the plug-in needs
//! its own OS window. We create a plain child window parented to the DAW window
//! and move it to whatever rectangle the egui wrapper reserves; the plug-in's
//! `IPlugView`/`clap_plugin_gui` attaches to that child.

/// A child window owned by the editor wrapper. The handle is kept as `isize`
/// so the type stays `Send`, matching how eframe hands out window handles.
#[derive(Debug)]
pub struct EditorWindow {
    handle: isize,
}

impl EditorWindow {
    /// Creates a visible child window at `(x, y)` in physical client pixels.
    #[cfg(windows)]
    pub fn new(parent: isize, x: i32, y: i32, width: i32, height: i32) -> Option<Self> {
        let class = wide("STATIC");
        let title = wide("");
        // SAFETY: the predefined STATIC class always exists, and all pointers
        // are valid for the duration of the call.
        let handle = unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                windows_sys::Win32::UI::WindowsAndMessaging::WS_CHILD
                    | windows_sys::Win32::UI::WindowsAndMessaging::WS_VISIBLE
                    | windows_sys::Win32::UI::WindowsAndMessaging::WS_CLIPCHILDREN
                    | windows_sys::Win32::UI::WindowsAndMessaging::WS_CLIPSIBLINGS,
                x,
                y,
                width.max(1),
                height.max(1),
                parent as _,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if handle.is_null() {
            None
        } else {
            Some(Self {
                handle: handle as isize,
            })
        }
    }

    /// Non-Windows builds have no embedding backend yet.
    #[cfg(not(windows))]
    pub fn new(_parent: isize, _x: i32, _y: i32, _width: i32, _height: i32) -> Option<Self> {
        None
    }

    pub fn handle(&self) -> isize {
        self.handle
    }

    #[cfg(windows)]
    pub fn set_rect(&self, x: i32, y: i32, width: i32, height: i32) {
        // SAFETY: the handle is owned by this type and still alive.
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::SetWindowPos(
                self.handle as _,
                std::ptr::null_mut(),
                x,
                y,
                width.max(1),
                height.max(1),
                windows_sys::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                    | windows_sys::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE,
            );
        }
    }

    #[cfg(not(windows))]
    pub fn set_rect(&self, _x: i32, _y: i32, _width: i32, _height: i32) {}

    #[cfg(windows)]
    pub fn set_visible(&self, visible: bool) {
        // SAFETY: the handle is owned by this type and still alive.
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(
                self.handle as _,
                if visible {
                    windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNA
                } else {
                    windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE
                },
            );
        }
    }

    #[cfg(not(windows))]
    pub fn set_visible(&self, _visible: bool) {}

    #[cfg(windows)]
    pub fn destroy(&mut self) {
        if self.handle != 0 {
            // SAFETY: the handle is owned by this type and destroyed once.
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(self.handle as _);
            }
            self.handle = 0;
        }
    }

    #[cfg(not(windows))]
    pub fn destroy(&mut self) {
        self.handle = 0;
    }
}

impl Drop for EditorWindow {
    fn drop(&mut self) {
        self.destroy();
    }
}

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
