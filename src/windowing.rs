use dioxus::desktop::LogicalPosition;
use dioxus::desktop::tao::dpi::PhysicalPosition;
use dioxus::desktop::tao::window::Window;

pub const CIRCLE_SIZE: f64 = 48.0;
pub const COLLAPSED_W: f64 = 280.0;
pub const COLLAPSED_H: f64 = 48.0;
pub const EXPANDED_W: f64 = 400.0;
pub const EXPANDED_H: f64 = 480.0;
pub const ISLAND_BLEED: f64 = 18.0;

/// The OS window is created once at the panel footprint and never resized:
/// every expand/collapse is pure CSS inside it. Resizing a WebView2 window
/// always flashes (the old frame is shown top-left-anchored until the next
/// paint), so we simply never resize.
pub const WINDOW_W: f64 = EXPANDED_W + ISLAND_BLEED * 2.0;
pub const WINDOW_H: f64 = EXPANDED_H + ISLAND_BLEED * 2.0;

const TOP_MARGIN: f64 = 8.0;

/// Restore the position saved by the last drag. Returns false when there is
/// no saved position or it no longer overlaps any monitor (display layout
/// changed since), so the caller should fall back to `place_top_center`.
/// Everything is in physical pixels, matching `Window::outer_position`.
pub fn restore_position(window: &Window) -> bool {
    let Some((x, y)) = crate::storage::load_window_pos() else {
        return false;
    };
    let scale = window.scale_factor();
    let w = (WINDOW_W * scale) as i32;
    let h = (WINDOW_H * scale) as i32;
    // At least 40px of the window must be on-screen on some monitor, or it
    // would be stranded where the user can no longer grab it.
    let visible = window.available_monitors().any(|m| {
        let mp = m.position();
        let ms = m.size();
        x + w > mp.x + 40
            && x < mp.x + ms.width as i32 - 40
            && y + h > mp.y + 40
            && y < mp.y + ms.height as i32 - 40
    });
    if !visible {
        return false;
    }
    window.set_outer_position(PhysicalPosition::new(x, y));
    true
}

pub fn place_top_center(window: &Window, width: f64) {
    if let Some(monitor) = window
        .current_monitor()
        .or_else(|| window.primary_monitor())
    {
        let scale = monitor.scale_factor();
        let size = monitor.size().to_logical::<f64>(scale);
        let position = monitor.position().to_logical::<f64>(scale);
        let x = position.x + ((size.width - width) / 2.0).max(0.0);
        window.set_outer_position(LogicalPosition::new(x.round(), position.y + TOP_MARGIN));
    }
}

/// Hot regions (physical px, left/top/right/bottom) where the window must
/// stay interactive: the island itself, plus the panel while expanded.
/// Everywhere else the fixed-size window is transparent and click-through.
///
/// While expanded the whole window becomes interactive: clicks landing on the
/// transparent margin are used to collapse the panel (popover behavior), so
/// they must not fall through to apps underneath.
pub fn hot_rects(window: &Window, expanded: bool, island_wide: bool) -> Vec<(i32, i32, i32, i32)> {
    let scale = window.scale_factor();
    let Ok(pos) = window.outer_position() else {
        // Cannot locate the window: stay interactive (safe default).
        return vec![(i32::MIN / 2, i32::MIN / 2, i32::MAX / 2, i32::MAX / 2)];
    };
    let outer = window.outer_size();
    let inner = window.inner_size();
    // Client origin inside the outer frame (≈0 for this undecorated window).
    let ox = pos.x as f64 + (outer.width as f64 - inner.width as f64) / 2.0;
    let oy = pos.y as f64 + (outer.height as f64 - inner.height as f64) / 2.0;

    let to_rect = |x: f64, y: f64, w: f64, h: f64| {
        let l = ox + x * scale;
        let t = oy + y * scale;
        (
            l.round() as i32,
            t.round() as i32,
            (l + w * scale).round() as i32,
            (t + h * scale).round() as i32,
        )
    };

    if expanded {
        return vec![to_rect(0.0, 0.0, WINDOW_W, WINDOW_H)];
    }

    // The island is centered in the stage's content box (bleed + EXPANDED_W).
    let island_w = if island_wide { COLLAPSED_W } else { CIRCLE_SIZE };
    let island_x = ISLAND_BLEED + (EXPANDED_W - island_w) / 2.0;
    vec![to_rect(island_x, ISLAND_BLEED, island_w, COLLAPSED_H)]
}

#[cfg(target_os = "windows")]
pub fn cursor_inside(rects: &[(i32, i32, i32, i32)]) -> bool {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let mut pt = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut pt) } == 0 {
        return true; // Stay interactive if the query fails.
    }
    rects
        .iter()
        .any(|&(l, t, r, b)| pt.x >= l && pt.x < r && pt.y >= t && pt.y < b)
}

#[cfg(not(target_os = "windows"))]
pub fn cursor_inside(_rects: &[(i32, i32, i32, i32)]) -> bool {
    true
}

/// Toggle WS_EX_TRANSPARENT | WS_EX_LAYERED so mouse input falls through the
/// transparent parts of the window to apps underneath. The original extended
/// style is preserved exactly when interactivity is restored.
#[cfg(target_os = "windows")]
pub fn set_click_through(window: &Window, on: bool) {
    use dioxus::desktop::tao::platform::windows::WindowExtWindows;
    use std::sync::OnceLock;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, SetWindowLongPtrW, WS_EX_LAYERED, WS_EX_TRANSPARENT,
    };

    static ORIGINAL_EXSTYLE: OnceLock<isize> = OnceLock::new();

    let hwnd = window.hwnd() as _;
    let bits = (WS_EX_TRANSPARENT | WS_EX_LAYERED) as isize;
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let base = *ORIGINAL_EXSTYLE.get_or_init(|| ex);
        let new = if on { base | bits } else { base };
        if new != ex {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn set_click_through(_window: &Window, _on: bool) {}
