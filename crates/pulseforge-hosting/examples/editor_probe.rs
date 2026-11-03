//! Verifies that a plug-in editor really attaches to a host child window.
//!
//! Creates a throwaway top-level window, embeds the editor through
//! [`pulseforge_hosting::editor::EditorWindow`], pumps the Win32 message loop
//! for a moment, then reports whether the plug-in created any child window of
//! its own (which is what a working `IPlugView`/`clap_plugin_gui` attach does).
//!
//! `cargo run -p pulseforge-hosting --example editor_probe -- <bundle>`

#[cfg(not(windows))]
fn main() {
    println!("editor_probe only runs on Windows");
}

#[cfg(windows)]
fn main() {
    use pulseforge_hosting::editor::EditorWindow;
    use pulseforge_hosting::{ParentWindow, load};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, DispatchMessageW, EnumChildWindows, GetMessageW, MSG,
        SW_HIDE, TranslateMessage, WM_QUIT, WS_OVERLAPPEDWINDOW,
    };

    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: editor_probe <bundle>");
        std::process::exit(2);
    };
    let (mut ui, audio) = match load(&path, true, 48_000.0, 64) {
        Ok(pair) => pair,
        Err(error) => {
            eprintln!("load failed: {error}");
            std::process::exit(1);
        }
    };
    println!("plug-in     : {}", ui.info().name);
    println!("editor      : {}", ui.info().supports_editor);
    println!("preferred   : {:?}", ui.editor_size());
    drop(audio);

    if !ui.info().supports_editor {
        println!("no editor to embed");
        return;
    }

    let wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain([0]).collect() };
    // SAFETY: plain Win32 window creation on this thread; every pointer is valid.
    let parent = unsafe {
        CreateWindowExW(
            0,
            wide("STATIC").as_ptr(),
            wide("probe").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            800,
            600,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if parent.is_null() {
        eprintln!("could not create the host window");
        std::process::exit(1);
    }

    let Some(child) = EditorWindow::new(parent as isize, 0, 0, 700, 500) else {
        eprintln!("could not create the editor window");
        unsafe { DestroyWindow(parent) };
        std::process::exit(1);
    };

    match ui.open_editor(ParentWindow(child.handle()), 700, 500) {
        Ok((width, height)) => println!("open_editor : ok ({width}x{height})"),
        Err(error) => {
            println!("open_editor : {error}");
            ui.close_editor();
            drop(child);
            unsafe { DestroyWindow(parent) };
            return;
        }
    }

    // SAFETY: standard message pump for this thread's windows.
    unsafe {
        let mut message: MSG = std::mem::zeroed();
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(700);
        while std::time::Instant::now() < deadline {
            while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
                if message.message == WM_QUIT {
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    let mut count = 0usize;
    // SAFETY: the enumeration callback only increments a local counter.
    unsafe {
        EnumChildWindows(
            child.handle() as _,
            Some(enum_count),
            &mut count as *mut usize as isize,
        );
    }
    println!("child windows: {count}");
    if count == 0 {
        println!("WARNING: the plug-in did not attach a child window");
    }

    ui.close_editor();
    drop(child);
    // SAFETY: hides then destroys the window created above.
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(parent, SW_HIDE);
        DestroyWindow(parent);
    }
}

#[cfg(windows)]
unsafe extern "system" fn enum_count(_window: *mut core::ffi::c_void, param: isize) -> i32 {
    // SAFETY: the caller passes a valid `*mut usize` as the user parameter.
    unsafe {
        let counter = &mut *(param as *mut usize);
        *counter += 1;
    }
    1
}
