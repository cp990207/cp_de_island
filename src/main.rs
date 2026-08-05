#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Entry point: single-instance guard, window/launch configuration, and the
//! Dioxus launch. All feature code lives in the modules below — see `ui` for
//! the view layer (state / hooks / components).

mod balance;
mod memo;
mod storage;
mod ui;
mod windowing;

use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
use windowing::{WINDOW_H, WINDOW_W};

#[cfg(target_os = "windows")]
use dioxus::desktop::tao::platform::windows::{WindowBuilderExtWindows, WindowExtWindows};

fn main() {
    // Refuse to run alongside another instance: two processes would keep
    // overwriting each other's data file (last writer wins).
    #[cfg(target_os = "windows")]
    let _instance_guard = match single_instance::acquire() {
        Some(guard) => guard,
        None => return,
    };

    dioxus::LaunchBuilder::desktop()
        .with_cfg(desktop_config())
        .launch(ui::App);
}

/// Single-instance guard backed by a named Win32 mutex. The returned handle
/// must stay alive for the whole process; the OS releases it on exit.
#[cfg(target_os = "windows")]
mod single_instance {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE,
    };
    use windows_sys::Win32::System::Threading::CreateMutexW;

    pub fn acquire() -> Option<HANDLE> {
        let name: Vec<u16> = "Local\\MemoPillSingleInstance\0"
            .encode_utf16()
            .collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            // Query failed — do not block the app over a locking error.
            return Some(handle);
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                CloseHandle(handle);
            }
            return None;
        }
        Some(handle)
    }
}

fn desktop_config() -> Config {
    let initial_w = WINDOW_W;
    let initial_h = WINDOW_H;

    let mut window = WindowBuilder::new()
        .with_title("Memo Pill")
        .with_inner_size(LogicalSize::new(initial_w, initial_h))
        .with_resizable(false)
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top(true)
        .with_visible(true);

    #[cfg(target_os = "windows")]
    {
        window = window
            .with_skip_taskbar(true)
            .with_undecorated_shadow(false);
    }

    Config::new()
        .with_window(window)
        .with_background_color((0, 0, 0, 0))
        .with_disable_context_menu(true)
        .with_on_window(move |handle, _| {
            handle.set_always_on_top(true);
            #[cfg(target_os = "windows")]
            {
                let _ = handle.set_skip_taskbar(true);
                handle.set_undecorated_shadow(false);
            }
            // Keep the position from the last drag; first run (or a stale
            // position off every monitor) falls back to top-center.
            if !windowing::restore_position(&handle) {
                windowing::place_top_center(&handle, initial_w);
            }
        })
}
