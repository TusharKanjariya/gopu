//! Where Ask Gopu waits on screen (ask-gopu map, "Staying on screen"). Two choices in Settings:
//! - **the tab** (default): Gopu's head in a small tab. Dropped near any edge of a screen it sticks to that edge, the
//!   corners flowing into it; dropped anywhere else it floats there as a capsule. Stuck to an edge, it follows the
//!   user to the screen they're working on.
//! - **taskbar**: Gopu's own tray icon by the clock. Windows lets no app inside the taskbar itself, and a window laid
//!   over it collides with the taskbar's widgets and gets covered, so the tray icon is the anchor.
//!
//! A click or the Gopu key opens the box from it; while an answer is on its way with the box folded away, it shows
//! Gopu is thinking, and the answer opens by itself.

use gopu_core::platform;
use gopu_core::settings::Settings;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Manager, Monitor, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// Logical sizes: the floating capsule, and the tab stuck to a left or right edge (turned for top and bottom).
const FLOAT: (f64, f64) = (34.0, 62.0);
const DOCK: (f64, f64) = (30.0, 74.0);
/// Dropped with its centre this close to an edge (logical px), the tab sticks to it.
const STICK: f64 = 60.0;
/// The tray icon is pulsing.
static PULSING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Edge {
    Right,
    Left,
    Top,
    Bottom,
    Float,
}

impl Edge {
    fn parse(s: Option<&str>) -> Edge {
        match s {
            Some("left") => Edge::Left,
            Some("top") => Edge::Top,
            Some("bottom") => Edge::Bottom,
            Some("float") => Edge::Float,
            _ => Edge::Right,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Edge::Right => "right",
            Edge::Left => "left",
            Edge::Top => "top",
            Edge::Bottom => "bottom",
            Edge::Float => "float",
        }
    }

    fn size(self) -> (f64, f64) {
        match self {
            Edge::Float => FLOAT,
            Edge::Right | Edge::Left => DOCK,
            Edge::Top | Edge::Bottom => (DOCK.1, DOCK.0),
        }
    }
}

/// A screen's work area (physical px) and its scale.
#[derive(Clone, Copy, Debug)]
struct Area {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    scale: f64,
}

fn area(m: &Monitor) -> Area {
    let a = m.work_area();
    Area { x: a.position.x, y: a.position.y, w: a.size.width as i32, h: a.size.height as i32, scale: m.scale_factor() }
}

fn area_at(app: &AppHandle, x: f64, y: f64) -> Option<Area> {
    app.monitor_from_point(x, y).ok().flatten().or_else(|| app.primary_monitor().ok().flatten()).map(|m| area(&m))
}

/// The tab's rectangle (x, y, w, h; physical px) for `edge` on `a`, centred as near (cx, cy) as the edge allows.
fn spot(edge: Edge, a: &Area, cx: i32, cy: i32) -> (i32, i32, i32, i32) {
    let (lw, lh) = edge.size();
    let (w, h) = ((lw * a.scale).round() as i32, (lh * a.scale).round() as i32);
    let fit_x = |x: i32| x.clamp(a.x, a.x + a.w - w);
    let fit_y = |y: i32| y.clamp(a.y, a.y + a.h - h);
    let (x, y) = match edge {
        Edge::Right => (a.x + a.w - w, fit_y(cy - h / 2)),
        Edge::Left => (a.x, fit_y(cy - h / 2)),
        Edge::Top => (fit_x(cx - w / 2), a.y),
        Edge::Bottom => (fit_x(cx - w / 2), a.y + a.h - h),
        Edge::Float => (fit_x(cx - w / 2), fit_y(cy - h / 2)),
    };
    (x, y, w, h)
}

/// The edge nearest to a centre at (cx, cy), and how far it is (logical px).
fn nearest(a: &Area, cx: i32, cy: i32) -> (Edge, f64) {
    [(Edge::Right, a.x + a.w - cx), (Edge::Left, cx - a.x), (Edge::Top, cy - a.y), (Edge::Bottom, a.y + a.h - cy)]
        .into_iter()
        .min_by_key(|&(_, d)| d)
        .map(|(e, d)| (e, d as f64 / a.scale))
        .unwrap()
}

/// Where a tab dropped with its centre at (cx, cy) ends up: the nearest edge when close enough, else floating there.
fn landing(a: &Area, cx: i32, cy: i32) -> Edge {
    match nearest(a, cx, cy) {
        (edge, d) if d <= STICK => edge,
        _ => Edge::Float,
    }
}

fn centre(w: &WebviewWindow) -> Option<(i32, i32)> {
    let (p, s) = (w.outer_position().ok()?, w.outer_size().ok()?);
    Some((p.x + s.width as i32 / 2, p.y + s.height as i32 / 2))
}

/// Moves and shapes the tab, and remembers where it is. `stick`: play the "just stuck" motion.
fn put(w: &WebviewWindow, edge: Edge, (x, y, wd, ht): (i32, i32, i32, i32), stick: bool) {
    let _ = w.set_size(PhysicalSize::new(wd as u32, ht as u32));
    let _ = w.set_position(PhysicalPosition::new(x, y));
    let _ = w.eval(&format!("anchor.shape('{}', {stick})", edge.name()));
    let mut s = Settings::load();
    if s.ask_tab_at != Some((x, y)) || Edge::parse(s.ask_tab_edge.as_deref()) != edge {
        s.ask_tab_at = Some((x, y));
        s.ask_tab_edge = (edge != Edge::Right).then(|| edge.name().to_string());
        let _ = s.save();
    }
}

/// Shows the anchor the settings ask for: the tab, or nothing extra (the tray icon is always there).
pub fn apply(app: &AppHandle) {
    let s = Settings::load();
    if s.ask_in_taskbar() {
        if let Some(w) = app.get_webview_window("anchor") {
            let _ = w.close();
        }
        return;
    }
    if let Some(w) = app.get_webview_window("anchor") {
        place(&w, &s);
        return;
    }
    let Ok(w) = WebviewWindowBuilder::new(app, "anchor", WebviewUrl::App("anchor.html".into()))
        .title("Ask Gopu")
        .inner_size(DOCK.0, DOCK.1)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focusable(false) // a click on it never takes the keyboard from the user's window
        .visible(false)
        .build()
    else {
        return;
    };
    if let Ok(h) = w.hwnd() {
        platform::hide_from_alt_tab(h.0);
    }
    place(&w, &s);
    if let Ok(h) = w.hwnd() {
        platform::show_quietly(h.0);
    }
    // a size set before the window is first shown doesn't stick (it came out 42 px wide instead of 30): again
    place(&w, &s);
    follow(w);
}

/// Puts the tab where it was left (or a bit above the middle of the main screen's right edge), in its shape.
fn place(w: &WebviewWindow, s: &Settings) {
    let app = w.app_handle();
    let edge = Edge::parse(s.ask_tab_edge.as_deref());
    let saved = s.ask_tab_at.and_then(|(x, y)| {
        let a = area_at(app, x as f64 + 4.0, y as f64 + 4.0).filter(|a| x >= a.x - 4 && x < a.x + a.w && y >= a.y - 4 && y < a.y + a.h)?;
        let (lw, lh) = edge.size();
        Some((a, x + (lw * a.scale) as i32 / 2, y + (lh * a.scale) as i32 / 2))
    });
    let start = saved.or_else(|| {
        let a = area(&app.primary_monitor().ok().flatten()?);
        Some((a, a.x + a.w, a.y + (a.h as f64 * 0.4) as i32))
    });
    if let Some((a, cx, cy)) = start {
        put(w, edge, spot(edge, &a, cx, cy), false);
    }
}

/// Every second: back on top (other always-on-top windows rise over it). Stuck to an edge, it also moves to the same
/// edge of the screen the mouse has been on for two seconds. Not while the box is open, or while being dragged.
// ponytail: the mouse's screen stands for the screen being worked on; the focused window's screen would be exact
fn follow(w: WebviewWindow) {
    std::thread::spawn(move || {
        let mut away = 0;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let Ok(h) = w.hwnd() else { break }; // closed (switched to the taskbar)
            platform::raise(h.0);
            let app = w.app_handle();
            let s = Settings::load();
            let edge = Edge::parse(s.ask_tab_edge.as_deref());
            let box_open = app.get_webview_window("ask").is_some_and(|b| b.is_visible().unwrap_or(false));
            let (Ok(c), Some((cx, cy))) = (app.cursor_position(), centre(&w)) else { continue };
            let (Some(here), Some(there)) = (area_at(app, cx as f64, cy as f64), area_at(app, c.x, c.y)) else { continue };
            if edge == Edge::Float || box_open || platform::mouse_down() || (there.x, there.y) == (here.x, here.y) {
                away = 0;
                continue;
            }
            away += 1;
            if away < 2 {
                continue;
            }
            away = 0;
            // the same edge, at the same place along it
            let fx = (cx - here.x) as f64 / here.w.max(1) as f64;
            let fy = (cy - here.y) as f64 / here.h.max(1) as f64;
            let (nx, ny) = (there.x + (fx * there.w as f64) as i32, there.y + (fy * there.h as f64) as i32);
            put(&w, edge, spot(edge, &there, nx, ny), false);
        }
    });
}

/// A drag of the tab: it turns into the floating capsule under the mouse, stretches toward an edge it comes near,
/// and on release glides onto that edge and sticks, or stays floating where it was dropped.
fn drag(w: &WebviewWindow) {
    let app = w.app_handle().clone();
    let Ok(c) = app.cursor_position() else { return };
    let Some(a) = area_at(&app, c.x, c.y) else { return };
    let (x, y, wd, ht) = spot(Edge::Float, &a, c.x as i32, c.y as i32);
    let _ = w.set_size(PhysicalSize::new(wd as u32, ht as u32));
    let _ = w.set_position(PhysicalPosition::new(x, y));
    let _ = w.eval("anchor.shape('float', false)");
    let _ = w.start_dragging();
    let w = w.clone();
    std::thread::spawn(move || {
        let mut last = String::new();
        loop {
            std::thread::sleep(Duration::from_millis(25));
            let held = platform::mouse_down();
            let Some((cx, cy)) = centre(&w) else { return };
            let Some(a) = area_at(&app, cx as f64, cy as f64) else { continue };
            if !held {
                // let go: glide onto the edge (or stay), then take its shape
                let edge = landing(&a, cx, cy);
                let target = spot(edge, &a, cx, cy);
                let float = spot(Edge::Float, &a, target.0 + target.2 / 2, target.1 + target.3 / 2);
                if let Ok(p) = w.outer_position() {
                    for i in 1..=6 {
                        let t = i as f64 / 6.0;
                        let ease = 1.0 - (1.0 - t).powi(3);
                        let gx = p.x + ((float.0 - p.x) as f64 * ease) as i32;
                        let gy = p.y + ((float.1 - p.y) as f64 * ease) as i32;
                        let _ = w.set_position(PhysicalPosition::new(gx, gy));
                        std::thread::sleep(Duration::from_millis(14));
                    }
                }
                put(&w, edge, target, edge != Edge::Float);
                return;
            }
            // near an edge: the capsule leans and stretches toward it
            let (edge, d) = nearest(&a, cx, cy);
            let pull = ((STICK + 10.0 - d) / 30.0).clamp(0.0, 1.0);
            let now = format!("anchor.pull('{}', {:.1})", edge.name(), (pull * 4.0).round() / 4.0);
            if now != last {
                let _ = w.eval(&now);
                last = now;
            }
        }
    });
}

/// Where the box opens from the tab: (x, y, grows_down) in physical px for a box `width` logical px wide. Growing
/// up, `y` is the box's bottom; growing down, its top. None in taskbar mode (the box takes the corner by the clock).
pub fn box_corner(app: &AppHandle, width: f64) -> Option<(i32, i32, bool)> {
    let w = app.get_webview_window("anchor")?;
    let (p, size) = (w.outer_position().ok()?, w.outer_size().ok()?);
    let (tw, th) = (size.width as i32, size.height as i32);
    let a = area_at(app, (p.x + tw / 2) as f64, (p.y + th / 2) as f64)?;
    let (bw, gap) = ((width * a.scale) as i32, (8.0 * a.scale) as i32);
    let fit_x = |x: i32| x.clamp(a.x, (a.x + a.w - bw).max(a.x));
    Some(match Edge::parse(Settings::load().ask_tab_edge.as_deref()) {
        Edge::Right => (p.x - bw - gap, p.y + th, false),
        Edge::Left => (p.x + tw + gap, p.y + th, false),
        Edge::Top => (fit_x(p.x + tw / 2 - bw / 2), p.y + th + gap, true),
        Edge::Bottom => (fit_x(p.x + tw / 2 - bw / 2), p.y - gap, false),
        Edge::Float => {
            // beside the capsule on the side with more room, growing away from the nearer top or bottom
            let x = if p.x + tw / 2 > a.x + a.w / 2 { p.x - bw - gap } else { p.x + tw + gap };
            if p.y + th / 2 < a.y + a.h / 2 { (fit_x(x), p.y, true) } else { (fit_x(x), p.y + th, false) }
        }
    })
}

/// Gopu is thinking with the box folded away (or not any more): the tab glows, or the tray icon pulses.
pub fn thinking(app: &AppHandle, on: bool) {
    if let Some(w) = app.get_webview_window("anchor") {
        let _ = w.eval(&format!("anchor.thinking({on})"));
    }
    if on && Settings::load().ask_in_taskbar() && !PULSING.swap(true, Ordering::SeqCst) {
        let app = app.clone();
        std::thread::spawn(move || {
            let mut dim = false;
            while PULSING.load(Ordering::SeqCst) {
                dim = !dim;
                crate::memory_ui::tray_icon(&app, dim);
                std::thread::sleep(Duration::from_millis(500));
            }
            crate::memory_ui::refresh_tray(&app);
        });
    }
    if !on {
        PULSING.store(false, Ordering::SeqCst);
    }
}

/// The answer is in: the thinking stops, and the tab hops.
pub fn done(app: &AppHandle) {
    thinking(app, false);
    if let Some(w) = app.get_webview_window("anchor") {
        let _ = w.eval("anchor.hop()");
    }
}

/// A click on the tab or a left-click on the tray icon: opens the box, or folds it away when it's showing.
pub fn toggle(app: &AppHandle) {
    let showing = app.get_webview_window("ask").is_some_and(|w| w.is_visible().unwrap_or(false));
    if showing { crate::ask_ui::close(app) } else { crate::ask_ui::open(app) }
}

// ---------------------------------------------------------------- commands for ui/anchor.html

/// Async: a sync command runs on the main thread, and opening a window from there deadlocks on Windows.
#[tauri::command]
pub async fn anchor_click(app: AppHandle) {
    toggle(&app);
}

/// The tab is being dragged (sync on purpose: the drag has to start on the main thread, while the button is down).
#[tauri::command]
pub fn anchor_drag(app: AppHandle) {
    if let Some(w) = app.get_webview_window("anchor") {
        drag(&w);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn where_the_tab_lands() {
        let a = Area { x: 0, y: 0, w: 1920, h: 1040, scale: 1.0 };
        // near an edge it sticks there; in the middle it floats
        assert_eq!(landing(&a, 1900, 500), Edge::Right);
        assert_eq!(landing(&a, 30, 500), Edge::Left);
        assert_eq!(landing(&a, 900, 20), Edge::Top);
        assert_eq!(landing(&a, 900, 1010), Edge::Bottom);
        assert_eq!(landing(&a, 900, 500), Edge::Float);
        // stuck tabs sit flush with their edge, turned to lie along it, and stay on the screen
        assert_eq!(spot(Edge::Right, &a, 1900, 500), (1890, 463, 30, 74));
        assert_eq!(spot(Edge::Top, &a, 900, 20), (863, 0, 74, 30));
        assert_eq!(spot(Edge::Bottom, &a, 5, 1010), (0, 1010, 74, 30));
        assert_eq!(spot(Edge::Float, &a, 900, 500), (883, 469, 34, 62));
        // a second screen to the right, scaled: the same rules in its own pixels
        let b = Area { x: 1920, y: -115, w: 1920, h: 1160, scale: 1.25 };
        assert_eq!(landing(&b, 3830, 400), Edge::Right);
        assert_eq!(spot(Edge::Right, &b, 3830, 400).0, 3840 - 38);
    }
}
