//! The Ask Gopu box (ask-gopu slice 01, look A: a flyout): opened by a Gopu-key tap outside a text box or from the
//! tray. It grows upward from its bottom edge, closes when the user clicks elsewhere unless pinned, and a pinned box
//! remembers where it was dragged.

use gopu_core::llm::Llm;
use gopu_core::settings::Settings;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder, WindowEvent};

const WIDTH: f64 = 480.0;
/// Where the box sits (physical px): (x, y, grows_down). Growing up, `y` is its bottom edge, so a longer chat grows
/// upward; growing down (a tab on the top edge), `y` is its top edge.
static ANCHOR: Mutex<Option<(i32, i32, bool)>> = Mutex::new(None);
/// A question is out to the model.
static PENDING: AtomicBool = AtomicBool::new(false);
/// The box was just created: its first fit shows it. Later fits only resize (hiding the window makes the page report
/// a new size, and showing it then would undo every close).
static FIRST_FIT: AtomicBool = AtomicBool::new(false);
/// The box being created shows without taking the keyboard (the reminder opened it).
static QUIET: AtomicBool = AtomicBool::new(false);
/// The saved ask the end-of-day note offers, until the box picks it up.
static REMIND: Mutex<Option<String>> = Mutex::new(None);
/// One-off reminders showing now (their ids), until Done or Snooze; the tab glows while any are.
static DUE: Mutex<Vec<i64>> = Mutex::new(Vec::new());
/// The box was shown for a note: the page shows only the note card (look A), not the whole box.
static NOTE_ONLY: AtomicBool = AtomicBool::new(false);

/// Where the box goes: where it was pinned, else next to the tab, else the corner of the screen the mouse is on.
fn spot(app: &AppHandle) -> Option<(i32, i32, bool)> {
    let s = Settings::load();
    s.ask_at
        .filter(|&(x, y)| s.ask_pinned && on_a_screen(app, x, y))
        .map(|(x, y)| (x, y, false))
        .or_else(|| crate::anchor::box_corner(app, WIDTH))
        .or_else(|| corner(app))
}

/// The box's top for a box `h` px tall at `at`.
fn top_of((_, y, down): (i32, i32, bool), h: i32) -> i32 {
    if down { y } else { y - h }
}

/// Opens the box, or brings it forward with the cursor in the question.
pub fn open(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("ask") {
        if !w.is_visible().unwrap_or(false) {
            place(app, &w);
        }
        front(&w);
        let _ = w.eval("ask.full(); ask.refresh(); ask.focus()"); // saved asks may have changed on the Style page
        return;
    }
    *ANCHOR.lock().unwrap() = spot(app);
    FIRST_FIT.store(true, Ordering::SeqCst);
    let Ok(w) = WebviewWindowBuilder::new(app, "ask", WebviewUrl::App("ask.html".into()))
        .title("Ask Gopu")
        .inner_size(WIDTH, 120.0)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false) // shown by ask_fit once the page knows its height
        .build()
    else {
        return;
    };
    let win = w.clone();
    w.on_window_event(move |e| match e {
        // clicking elsewhere folds it away, unless pinned; an answer on its way shows on the anchor
        // Tauri also reports focus lost when the keyboard moves into the page inside the box, so it only counts if,
        // a moment later, the box isn't the window in front any more
        WindowEvent::Focused(false) if !Settings::load().ask_pinned => {
            let win = win.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(200));
                let in_front = win.hwnd().is_ok_and(|h| gopu_core::platform::is_foreground(h.0));
                if in_front || Settings::load().ask_pinned {
                    return;
                }
                hide(&win);
                if PENDING.load(Ordering::SeqCst) {
                    crate::anchor::thinking(win.app_handle(), true);
                }
            });
        }
        // moved (dragged, or placed by fit): keep growing the same way from the new spot; pinned, it's remembered
        WindowEvent::Moved(p) => {
            let h = win.outer_size().map_or(0, |s| s.height as i32);
            let mut anchor = ANCHOR.lock().unwrap();
            let down = anchor.is_some_and(|a| a.2);
            *anchor = Some(if down { (p.x, p.y, true) } else { (p.x, p.y + h, false) });
            drop(anchor);
            let mut s = Settings::load();
            if s.ask_pinned && s.ask_at != Some((p.x, p.y + h)) {
                s.ask_at = Some((p.x, p.y + h));
                let _ = s.save();
            }
        }
        _ => {}
    });
}

/// The anchor may have moved: the box goes next to it before it shows.
fn place(app: &AppHandle, w: &tauri::WebviewWindow) {
    let at = spot(app);
    *ANCHOR.lock().unwrap() = at;
    if let (Some(at), Ok(size)) = (at, w.outer_size()) {
        let _ = w.set_position(PhysicalPosition::new(at.0, top_of(at, size.height as i32)));
    }
}

/// The end-of-day note: the box shows, without taking the keyboard, offering the saved ask `ask`. Nothing is read or
/// sent until the user clicks it.
pub fn remind(app: &AppHandle, ask: String) {
    *REMIND.lock().unwrap() = Some(ask);
    show_note(app);
}

/// Shows the box without taking the keyboard, for a note. A box that's already showing keeps its chat; the note joins
/// it at the top. A hidden or new one shows only the note.
fn show_note(app: &AppHandle) {
    let Some(w) = app.get_webview_window("ask") else {
        QUIET.store(true, Ordering::SeqCst);
        NOTE_ONLY.store(true, Ordering::SeqCst);
        return open(app); // the new page picks the note up when it loads
    };
    if !w.is_visible().unwrap_or(false) {
        NOTE_ONLY.store(true, Ordering::SeqCst);
        place(app, &w);
        if let Ok(h) = w.hwnd() {
            gopu_core::platform::show_quietly(h.0);
        }
    }
    let _ = w.eval("ask.refresh()");
}

/// Ask Gopu's end-of-day reminder (slice 05): off unless set on the Style page; weekdays, once a day, skipped when
/// memory caught nothing today. It only looks at whether anything was captured; no model call until it's clicked.
pub fn watch_reminder(app: AppHandle) {
    std::thread::spawn(move || {
        let mut shown = None;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(30));
            let s = Settings::load();
            one_offs(&app, &s);
            let Some(r) = s.ask_reminder.filter(|r| s.ask_saved.iter().any(|a| a.name == r.ask)) else { continue };
            let now = gopu_core::memory::now();
            let Some(day) = gopu_core::settings::reminder_due(r.at, now, gopu_core::platform::utc_offset_secs()) else { continue };
            if shown == Some(day) {
                continue;
            }
            shown = Some(day);
            let captured = gopu_core::memory::db_path()
                .and_then(|p| gopu_core::memory::Store::open_read_only(&p).ok())
                .and_then(|st| st.windows(gopu_core::memory::today_start(now), now + 1).ok())
                .is_some_and(|w| !w.is_empty());
            if captured {
                remind(&app, r.ask);
            }
        }
    });
}

/// One-off reminders that just came due (or were missed while the PC slept) join the note, and the tab glows.
fn one_offs(app: &AppHandle, s: &Settings) {
    let now = gopu_core::memory::now();
    let mut due = DUE.lock().unwrap();
    due.retain(|id| s.reminders.iter().any(|r| r.id == *id)); // deleted in Settings meanwhile
    let fresh: Vec<i64> = s.reminders.iter().filter(|r| r.due(now).is_some() && !due.contains(&r.id)).map(|r| r.id).collect();
    if fresh.is_empty() {
        return;
    }
    due.extend(fresh);
    drop(due);
    crate::anchor::due(app, true);
    show_note(app);
}

/// A reminder left the note (done, snoozed or deleted): the tab stops glowing once none are left.
fn settle(app: &AppHandle, id: i64) {
    let mut due = DUE.lock().unwrap();
    due.retain(|d| *d != id);
    if due.is_empty() {
        crate::anchor::due(app, false);
    }
}

/// The box is the window in front (a Gopu-key tap there closes it instead of writing into it).
pub fn in_front(app: &AppHandle) -> bool {
    app.get_webview_window("ask").is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false))
}

pub fn close(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("ask") {
        hide(&w);
    }
}

/// The box is shown with raw Windows calls (`front`, or quietly when an answer arrives), so it's hidden the same way.
fn hide(w: &tauri::WebviewWindow) {
    if let Ok(h) = w.hwnd() {
        gopu_core::platform::hide(h.0);
    }
}

/// Bottom right of the work area of the screen the mouse is on.
fn corner(app: &AppHandle) -> Option<(i32, i32, bool)> {
    let c = app.cursor_position().ok()?;
    let m = app.monitor_from_point(c.x, c.y).ok().flatten().or_else(|| app.primary_monitor().ok().flatten())?;
    let a = m.work_area();
    let w = (WIDTH * m.scale_factor()) as i32;
    Some((a.position.x + a.size.width as i32 - w - 12, a.position.y + a.size.height as i32 - 12, false))
}

fn on_a_screen(app: &AppHandle, x: i32, y: i32) -> bool {
    app.monitor_from_point(x as f64 + 20.0, y as f64 - 20.0).ok().flatten().is_some()
}

// ---------------------------------------------------------------- commands for ui/ask.html

/// The page's height changed: resize, keeping the edge it grows from where it is, and show the first time.
#[tauri::command]
pub fn ask_fit(app: AppHandle, height: f64) {
    let Some(w) = app.get_webview_window("ask") else { return };
    let scale = w.scale_factor().unwrap_or(1.0);
    let Some(at) = *ANCHOR.lock().unwrap() else { return };
    let (pw, ph) = ((WIDTH * scale) as u32, (height * scale) as u32);
    let _ = w.set_size(PhysicalSize::new(pw, ph));
    // always on the screen: never above its top, never below its bottom
    let y = top_of(at, ph as i32);
    // the screen the box is going to, not the one it's on now (it may be moving to another screen)
    let target = app.monitor_from_point(at.0 as f64 + 4.0, at.1 as f64).ok().flatten();
    let y = match target.or_else(|| w.current_monitor().ok().flatten()).map(|m| *m.work_area()) {
        Some(a) => y.min(a.position.y + a.size.height as i32 - ph as i32).max(a.position.y),
        None => y,
    };
    let _ = w.set_position(PhysicalPosition::new(at.0, y));
    if FIRST_FIT.swap(false, Ordering::SeqCst) {
        if QUIET.swap(false, Ordering::SeqCst) {
            if let Ok(h) = w.hwnd() {
                gopu_core::platform::show_quietly(h.0);
            }
        } else {
            front(&w);
        }
    }
}

/// Shows the box in front with the keyboard, from any thread (a Gopu-key tap runs on the key thread, and Windows
/// won't hand the foreground to an app over a key it didn't see itself; see `platform::bring_to_front`).
fn front(w: &tauri::WebviewWindow) {
    let Ok(hwnd) = w.hwnd() else { return };
    let hwnd = hwnd.0 as isize;
    let _ = w.run_on_main_thread(move || gopu_core::platform::bring_to_front(hwnd as *mut _));
}

/// At start: a pinned box is part of the screen, so it comes back.
pub fn restore(app: &AppHandle) {
    if Settings::load().ask_pinned {
        open(app);
    }
}

/// The mascot on the bar is the grip: drag the box anywhere.
#[tauri::command]
pub fn ask_drag(app: AppHandle) {
    if let Some(w) = app.get_webview_window("ask") {
        let _ = w.start_dragging();
    }
}

#[tauri::command]
pub fn ask_close(app: AppHandle) {
    close(&app);
}

#[tauri::command]
pub fn ask_state() -> Value {
    let s = Settings::load();
    json!({ "pinned": s.ask_pinned, "saved": s.ask_saved, "reminder": s.ask_reminder, "reminders": s.reminders })
}

/// What the note shows: `{ due: [{ id, title, at, missed }], eod, only, fold }`. `eod`: the saved ask the end-of-day
/// note offers (given once); `only`: the box was shown just for the note; `fold`: fold it back into the tab if ignored
/// (with the taskbar anchor there's no tab to fold into, so it stays).
#[tauri::command]
pub fn ask_note() -> Value {
    let s = Settings::load();
    let now = gopu_core::memory::now();
    let due = DUE.lock().unwrap().clone();
    let list: Vec<Value> = s
        .reminders
        .iter()
        .filter(|r| due.contains(&r.id))
        .map(|r| json!({ "id": r.id, "title": r.title, "at": r.at, "missed": r.due(now).unwrap_or(false) }))
        .collect();
    json!({ "due": list, "eod": REMIND.lock().unwrap().take(), "only": NOTE_ONLY.swap(false, Ordering::SeqCst), "fold": !s.ask_in_taskbar() })
}

/// The Reminders page: a new one-off (`id` None) or an edit. `at`: Unix seconds; `early`: minutes. Returns the list,
/// soonest first.
#[tauri::command]
pub fn reminder_save(id: Option<i64>, title: String, at: i64, early: u32) -> Result<Vec<gopu_core::settings::OneOff>, String> {
    let title = title.trim().chars().take(80).collect::<String>();
    if title.is_empty() {
        return Err("Say what it's for.".into());
    }
    if at <= gopu_core::memory::now() {
        return Err("Pick a time that's still ahead.".into());
    }
    let mut s = Settings::load();
    let next = s.reminders.iter().map(|r| r.id).max().unwrap_or(0) + 1;
    let r = gopu_core::settings::OneOff { id: id.unwrap_or(next), title, at, early: early.min(24 * 60), snoozed: None };
    match id {
        Some(id) => *s.reminders.iter_mut().find(|r| r.id == id).ok_or("That reminder is gone; open the list again.")? = r,
        None => s.reminders.push(r),
    }
    s.reminders.sort_by_key(|r| r.at);
    s.save()?;
    Ok(s.reminders)
}

#[tauri::command]
pub fn reminder_delete(app: AppHandle, id: i64) -> Result<Vec<gopu_core::settings::OneOff>, String> {
    let mut s = Settings::load();
    s.reminders.retain(|r| r.id != id);
    s.save()?;
    settle(&app, id);
    Ok(s.reminders)
}

/// Done on the note: a one-off is finished, so it goes.
#[tauri::command]
pub fn reminder_done(app: AppHandle, id: i64) -> Result<(), String> {
    reminder_delete(app, id).map(|_| ())
}

/// Snooze on the note: it comes back at `until` (Unix seconds).
#[tauri::command]
pub fn reminder_snooze(app: AppHandle, id: i64, until: i64) -> Result<(), String> {
    let mut s = Settings::load();
    if let Some(r) = s.reminders.iter_mut().find(|r| r.id == id) {
        r.snoozed = Some(until.max(gopu_core::memory::now() + 60));
        s.save()?;
    }
    settle(&app, id);
    Ok(())
}

/// The Style page's reminder: `at` minutes after midnight and the saved ask's name; `None` turns it off.
#[tauri::command]
pub fn ask_reminder_set(at: Option<u32>, ask: String) -> Result<(), String> {
    let mut s = Settings::load();
    s.ask_reminder = at.filter(|&m| m < 24 * 60).map(|at| gopu_core::settings::Reminder { at, ask });
    s.save()
}

/// Saves an ask (`index` None: a new one; else replaces that one). Returns the list.
#[tauri::command]
pub fn ask_saved_set(index: Option<usize>, name: String, text: String) -> Result<Vec<gopu_core::settings::SavedAsk>, String> {
    let (name, text) = (name.trim().chars().take(40).collect::<String>(), text.trim().to_string());
    if name.is_empty() || text.is_empty() {
        return Err("Give it a name and the request it sends.".into());
    }
    let mut s = Settings::load();
    let ask = gopu_core::settings::SavedAsk { name, text };
    match index {
        Some(i) if i < s.ask_saved.len() => {
            // a renamed ask keeps its reminder
            if let Some(r) = s.ask_reminder.as_mut().filter(|r| r.ask == s.ask_saved[i].name) {
                r.ask = ask.name.clone();
            }
            s.ask_saved[i] = ask
        }
        Some(_) => return Err("That ask is gone; open the list again.".into()),
        None => s.ask_saved.push(ask),
    }
    s.save()?;
    Ok(s.ask_saved)
}

#[tauri::command]
pub fn ask_saved_delete(index: usize) -> Result<Vec<gopu_core::settings::SavedAsk>, String> {
    let mut s = Settings::load();
    if index < s.ask_saved.len() {
        s.ask_saved.remove(index);
        s.save()?;
    }
    Ok(s.ask_saved)
}

#[tauri::command]
pub fn ask_pin(app: AppHandle, on: bool) -> Result<(), String> {
    let mut s = Settings::load();
    s.ask_pinned = on;
    let rect = app.get_webview_window("ask").and_then(|w| Some((w.outer_position().ok()?, w.outer_size().ok()?)));
    s.ask_at = rect.filter(|_| on).map(|(p, size)| (p.x, p.y + size.height as i32));
    s.save()?;
    if let Some(w) = app.get_webview_window("ask") {
        let _ = w.set_focus();
    }
    Ok(())
}

/// A cited capture, opened in Settings → Search. Async: opening a window from a sync command deadlocks on Windows.
#[tauri::command]
pub async fn ask_open_capture(app: AppHandle, id: i64) {
    match app.get_webview_window("settings") {
        Some(w) => {
            let _ = w.unminimize();
            let _ = w.set_focus();
            let _ = w.eval(&format!("showPage('search'); openCapture({id})"));
        }
        None => crate::open_settings(&app, Some(&format!("search:{id}"))),
    }
}

/// One question, answered from memory: `{ text, sources: [{ n, id, app, title, site, when }] }`. The error is a
/// sentence for the box.
#[tauri::command]
/// `history`: the chat so far, [question, answer] oldest first. `keep`: the capture ids the last answer used.
/// `chat`: the kept chat this continues (None: a new one); `label`: a saved ask's name, shown instead of the request.
pub async fn ask_question(
    app: AppHandle, question: String, history: Vec<(String, String)>, keep: Vec<i64>, chat: Option<i64>, label: Option<String>,
) -> Result<Value, String> {
    PENDING.store(true, Ordering::SeqCst);
    let app2 = app.clone();
    let answer = tauri::async_runtime::spawn_blocking(move || {
        let saved = Settings::load();
        let me = saved.resolve(gopu_core::setting);
        let now = gopu_core::memory::now();
        let needs_key = gopu_core::settings::provider(&me.provider).is_some_and(|p| p.needs_key);
        let llm = Llm::new(&me);
        if needs_key && !llm.has_key() {
            crate::open_settings(&app, Some("model"));
            return Err("I need a model key to answer. I've opened Settings: paste one there.".into());
        }
        if saved.memory_state(now) == gopu_core::settings::MemoryState::Off {
            return Err("Memory is off, so I have nothing to go on. Turn it on in Settings → Privacy.".into());
        }
        let store = gopu_core::memory::db_path()
            .ok_or("no memory folder".to_string())
            .and_then(|p| gopu_core::memory::Store::open_read_only(&p))
            .map_err(|_| "I don't remember anything yet.".to_string())?;
        let a = gopu_core::ask::ask(&llm, &store, &question, &history, &keep, &me, now).map_err(|e| {
            println!("ask: {e}");
            gopu_core::llm::problem(&e).map_or("Something went wrong. Try again in a moment.".to_string(), |(say, _)| say.to_string())
        })?;
        log(&saved, &question, &a);
        let sources: Vec<Value> = a
            .sources
            .iter()
            .map(|s| {
                json!({ "n": s.n, "id": s.id, "app": s.app.trim_end_matches(".exe"), "title": s.title, "site": s.site,
                        "when": gopu_core::memory::clock_text(s.last_seen) })
            })
            .collect();
        // kept on this PC for 30 days, so the chat can be opened again
        let chat = gopu_core::memory::db_path()
            .ok_or("no memory folder".to_string())
            .and_then(|p| gopu_core::memory::Store::open(&p))
            .and_then(|st| gopu_core::ask::keep_turn(&st, chat, now, &question, label.as_deref(), &a.text, &json!(sources).to_string()))
            .map_err(|e| println!("ask: couldn't keep the chat: {e}"))
            .ok();
        Ok(json!({ "text": a.text, "sources": sources, "chat": chat }))
    })
    .await;
    // folded away while thinking: the answer opens by itself, without taking the keyboard
    PENDING.store(false, Ordering::SeqCst);
    crate::anchor::done(&app2);
    if let Some(w) = app2.get_webview_window("ask").filter(|w| !w.is_visible().unwrap_or(true)) {
        if let Ok(h) = w.hwnd() {
            gopu_core::platform::show_quietly(h.0);
        }
    }
    answer
    .map_err(|e| e.to_string())?
}

/// With the debug switch on (About, or `GOPU_DEBUG`), the last asks go to `last-asks.txt` in the log folder: what was
/// sent and what came back, for checking answers. It holds screen text, so it's off by default and trimmed.
fn log(s: &Settings, question: &str, a: &gopu_core::ask::Answer) {
    let Some(path) = gopu_core::platform::log_dir().map(|d| d.join("last-asks.txt")) else { return };
    if !s.debug && gopu_core::setting("GOPU_DEBUG").is_none() {
        let _ = std::fs::remove_file(path);
        return;
    }
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let old: String = old.chars().rev().take(150_000).collect::<Vec<_>>().into_iter().rev().collect();
    let at = gopu_core::memory::clock_text(gopu_core::memory::now());
    let entry = format!("===== {at} · {question}\n----- SENT -----\n{}\n----- ANSWER -----\n{}\n\n", a.sent, a.text);
    let _ = std::fs::write(path, old + &entry);
}

/// The kept chats for the box's list: `[{ chat, title, at, turns, when }]`, newest first.
#[tauri::command]
pub fn ask_chats() -> Result<Value, String> {
    let st = gopu_core::memory::Store::open_read_only(&gopu_core::memory::db_path().ok_or("no memory folder")?)?;
    let now = gopu_core::memory::now();
    let today = gopu_core::memory::today_start(now);
    let list: Vec<Value> = gopu_core::ask::chats(&st, now)?
        .into_iter()
        .map(|c| {
            let when = if c.at >= today { gopu_core::memory::clock_text(c.at) } else { gopu_core::people::local_date(c.at) };
            json!({ "chat": c.chat, "title": c.title, "at": c.at, "turns": c.turns, "when": when })
        })
        .collect();
    Ok(json!(list))
}

/// One kept chat, to open again: `[{ question, label, answer, sources }]`, oldest first.
#[tauri::command]
pub fn ask_chat(chat: i64) -> Result<Value, String> {
    let st = gopu_core::memory::Store::open_read_only(&gopu_core::memory::db_path().ok_or("no memory folder")?)?;
    Ok(json!(gopu_core::ask::chat(&st, chat)?))
}
