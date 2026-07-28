use dioxus::desktop::tao::window::Window;
use dioxus::desktop::{DesktopContext, LogicalPosition, LogicalSize};

pub const COLLAPSED_W: f64 = 280.0;
pub const COLLAPSED_H: f64 = 48.0;
pub const EXPANDED_W: f64 = 400.0;
pub const EXPANDED_H: f64 = 480.0;
pub const ISLAND_BLEED: f64 = 18.0;

pub fn set_window_size(desktop: &DesktopContext, expanded: bool) {
    let w = if expanded { EXPANDED_W } else { COLLAPSED_W } + ISLAND_BLEED * 2.0;
    let h = if expanded { EXPANDED_H } else { COLLAPSED_H } + ISLAND_BLEED * 2.0;
    desktop.set_inner_size(LogicalSize::new(w, h));
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
        window.set_outer_position(LogicalPosition::new(x.round(), position.y + 8.0));
    }
}
