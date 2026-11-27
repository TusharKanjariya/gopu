// Gopu desktop app: tray icon, Gopu key, overlay. A tap drops Gopu by the caret, reads the on-screen context, asks the
// model for the field's new text, writes it back into the field, and the Gopu Walk shows each step.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use gopu_core::assist::{self, FieldCase, Outcome};
use gopu_core::llm::Llm;
use gopu_core::settings::{Resolved, Settings};
use gopu_core::{platform, Field, OnScreenContext};
use tauri::tray::TrayIconBuilder;

mod anchor;
mod ask_ui;
mod memory_ui;
mod update;
use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            settings_get, settings_save, settings_test, open_logs, welcome_done, show_settings, open_link,
            memory_ui::memory_state, memory_ui::memory_search, memory_ui::memory_get, memory_ui::memory_forget,
            memory_ui::memory_ignore, memory_ui::memory_set, memory_ui::people_list, memory_ui::people_save,
            memory_ui::people_delete, memory_ui::style_state, memory_ui::style_save, memory_ui::style_delete, memory_ui::mcp_state, memory_ui::mcp_set,
            update::update_state, update::update_check, update::update_install,
            anchor::anchor_click, anchor::anchor_drag, ask_ui::ask_fit, ask_ui::ask_drag, ask_ui::ask_close, ask_ui::ask_state, ask_ui::ask_pin, ask_ui::ask_open_capture, ask_ui::ask_question, ask_ui::ask_saved_set, ask_ui::ask_saved_delete, ask_ui::ask_chats, ask_ui::ask_chat, ask_ui::ask_note, ask_ui::ask_reminder_set, ask_ui::reminder_save, ask_ui::reminder_delete, ask_ui::reminder_done, ask_ui::reminder_snooze,
        ])
        .setup(|app| {
            let overlay = app.get_webview_window("overlay").expect("overlay window");
            overlay.set_ignore_cursor_events(true)?;
            platform::hide_from_alt_tab(overlay.hwnd()?.0);

            apply_keys(&Settings::load());
            apply_autostart(&Settings::load());
            // the menu, tooltip and icon come from refresh_tray, so they always match the saved settings
            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().expect("icon").clone())
                // left-click asks Gopu (the taskbar anchor); right-click is the menu
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, e| {
                    if let tauri::tray::TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, button_state: tauri::tray::MouseButtonState::Up, .. } = e {
                        anchor::toggle(tray.app_handle());
                    }
                })
                .on_menu_event(move |app, e| match e.id().as_ref() {
                    id if memory_ui::on_menu(app, id) => {}
                    "pause" => {
                        let mut s = Settings::load();
                        s.paused = !s.paused;
                        if let Err(e) = s.save() {
                            println!("couldn't save pause: {e}");
                        }
                        memory_ui::refresh_tray(app);
                    }
                    "settings" => open_settings(app, None),
                    "update" => update::install(app),
                    "update_check" => open_settings(app, Some("about")),
                    "ask" => ask_ui::open(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            memory_ui::refresh_tray(app.handle());
            memory_ui::watch_pause(app.handle().clone());
            update::start(app.handle().clone());
            anchor::apply(app.handle());
            ask_ui::restore(app.handle());
            ask_ui::watch_reminder(app.handle().clone());

            report(&Settings::load().resolve(gopu_core::setting));
            if needs_welcome(&Settings::load()) {
                open_welcome(app.handle());
            }

            // memory: capture the window in front in the background (its own on/off, pause and ignore list)
            gopu_core::memory::start_capture();

            platform::on_gopu_key(move || {
                if Settings::load().paused {
                    return;
                }
                // while the welcome flow has focus, the key plays its demo instead of a real tap
                let welcome = overlay.get_webview_window("welcome");
                if let Some(w) = welcome.filter(|w| w.hwnd().is_ok_and(|h| platform::is_foreground(h.0))) {
                    let _ = w.eval("welcome.keyTapped()");
                    return;
                }
                // a tap in the Ask Gopu box closes it; a tap outside any text box opens it
                if ask_ui::in_front(overlay.app_handle()) {
                    return ask_ui::close(overlay.app_handle());
                }
                let Some(field) = platform::focused_field() else { return ask_ui::open(overlay.app_handle()) };
                if field.not_editable {
                    return ask_ui::open(overlay.app_handle());
                }
                platform::busy(true);
                tap(&overlay, &field);
                platform::busy(false);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Gopu");
}

/// One Gopu-key run: Gopu drops by the caret, reads the screen, asks the model, writes back. Esc stops it at any point.
fn tap(overlay: &WebviewWindow, field: &Field) {
    // Gopu first, so the tap feels instant: drop by the caret and think
    let stage = Stage::place(overlay, field);
    stage.js(&format!("gopu.start({:.1})", stage.x(field.stand_x())));

    let ctx = platform::on_screen_context(field);
    // read fresh each tap, so a settings change applies on the next one
    let saved = Settings::load();
    let settings = saved.resolve(gopu_core::setting);
    // no model key (the welcome was finished without one): say so and open where it goes, instead of a failed call
    // that reads as "Oops"
    let needs_key = gopu_core::settings::provider(&settings.provider).is_some_and(|p| p.needs_key);
    if needs_key && !Llm::new(&settings).has_key() {
        let say = serde_json::json!("I need a model key to write. I've opened Settings: paste one there.");
        stage.js(&format!("gopu.note({say})"));
        open_settings(overlay.app_handle(), Some("model"));
        return;
    }
    // memory may be used here: on, not paused, this window not blocked or ignored. Then the tap gets recall (unless
    // "Use memory in replies" is off) and its result is remembered, with what the user ends up sending.
    let now = gopu_core::memory::now();
    let site = ctx.url.as_deref().and_then(gopu_core::privacy::site_from_url);
    let memory_ok = gopu_core::memory::allowed_here(&saved, now, &ctx.app, &ctx.window_title, site.as_deref());
    let surface = gopu_core::memory::surface(&ctx.app, site.as_deref());
    // who the message is for (one-to-one chats and email), so their people page can come along
    let recipient = memory_ok
        .then(|| gopu_core::people::recipient(
            &ctx.app, &ctx.window_title, site.as_deref(), &ctx.before, ctx.chat_header.as_deref(), settings.user_name.as_deref(),
        ))
        .flatten();
    let recall = (memory_ok && !saved.recall_off)
        .then(|| gopu_core::memory::recall(now, &ctx.app, &ctx.window_title, &surface, recipient.as_deref()))
        .flatten();
    let t0 = std::time::Instant::now();
    // the model call can take seconds: run it aside so Esc stops Gopu at once (a late reply is dropped)
    let (tx, rx) = std::sync::mpsc::channel();
    {
        let (llm, field, ctx, me, recall) = (Llm::new(&settings), field.clone(), ctx.clone(), settings.clone(), recall.clone());
        std::thread::spawn(move || {
            let _ = tx.send(assist::run(&llm, &field, &ctx, &me, recall.as_deref()));
        });
    }
    let outcome = loop {
        if platform::cancelled() {
            println!("  stopped before writing");
            stage.js("gopu.stopped(false)");
            return;
        }
        match rx.recv_timeout(std::time::Duration::from_millis(50)) {
            Ok(o) => break o,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => break Err("the model call crashed".into()),
        }
    };
    dump(field, &ctx, recall.as_deref(), &outcome, t0.elapsed().as_millis());

    // a lasting preference ("never use emojis") becomes a style rule, where memory is allowed
    let remembered = match &outcome {
        Ok(Outcome { remember: Some(r), .. }) if memory_ok => {
            let scope = if r.everywhere { String::new() } else { surface.clone() };
            let saved = gopu_core::memory::db_path()
                .ok_or("no folder".to_string())
                .and_then(|p| gopu_core::memory::Store::open(&p))
                .and_then(|st| gopu_core::style::add(&st, &r.rule, &scope, "instruction", now));
            match saved {
                Ok(()) => Some(r.rule.clone()),
                Err(e) => {
                    println!("  couldn't keep the rule: {e}");
                    None
                }
            }
        }
        Ok(Outcome { remember: Some(_), .. }) => {
            stage.js(&format!("gopu.note({})", serde_json::json!("Memory is off here, so I can't keep that.")));
            None
        }
        _ => None,
    };
    if let Ok(o) = &outcome {
        if o.text.trim().is_empty() {
            // only a preference: nothing to write, the field stays as it is
            match &remembered {
                Some(rule) => {
                    println!("  remembered: {rule}");
                    stage.js(&format!("gopu.remembered({})", serde_json::json!(rule)));
                }
                None if memory_ok => stage.js("gopu.error()"), // the rule couldn't be kept (the note covers memory-off)
                None => {}
            }
            return;
        }
    }
    let written = outcome.and_then(|o| {
        stage.js(&format!("gopu.writing('{}')", if o.asked { "On it" } else { writing_label(o.case) }));
        let owner = overlay.hwnd().map_err(|e| e.to_string())?;
        // the text goes in a few words at a time and Gopu walks along with it
        platform::write_back(field, o.replaces_draft(), &o.text, owner.0, |now| {
            stage.js(&format!("gopu.walkTo({:.1})", stage.x(now.stand_x())));
        })
        .map(|()| o)
    });
    match written {
        Ok(o) => {
            println!("  wrote it into the field");
            if memory_ok {
                remember_result(field, &ctx, site, recipient.clone(), &o);
                if let Some(name) = recipient {
                    // the page grows only from text this tap already sent
                    let conversation = format!("{}\n[the user's reply, written with Gopu] {}\n{}", ctx.before, o.text, ctx.after);
                    gopu_core::people::update_after_tap(settings.clone(), name, conversation);
                }
            }
            match &remembered {
                Some(rule) => {
                    println!("  remembered: {rule}");
                    stage.js(&format!("gopu.remembered({})", serde_json::json!(rule)));
                }
                None => stage.js("gopu.done()"), // the happy hop once the walk finishes
            }
            if let Some(w) = overlay.get_webview_window("welcome") {
                let _ = w.eval("welcome.didIt()"); // the first real tap finishes onboarding
            }
        }
        Err(e) if e == platform::STOPPED => {
            println!("  stopped mid-write");
            stage.js("gopu.stopped(true)");
        }
        Err(e) => {
            println!("  not written: {e}");
            match gopu_core::llm::problem(&e) {
                // the model call failed for a reason the user can fix: say which, and open Settings when the fix is there
                Some((say, settings)) => {
                    stage.js(&format!("gopu.note({})", serde_json::json!(say)));
                    if settings {
                        open_settings(overlay.app_handle(), Some("model"));
                    }
                }
                None => stage.js("gopu.error()"), // "Oops. Your draft is safe."
            }
        }
    }
}

/// Keeps this tap as a key result and starts watching what the user sends from the field (the field still has focus).
fn remember_result(field: &Field, ctx: &OnScreenContext, site: Option<String>, recipient: Option<String>, o: &Outcome) {
    use gopu_core::memory::{self, KeyResult, Store};
    let user_text = if o.case == FieldCase::Selection { field.selection.clone() } else { gopu_core::split_kept(&field.text).0.to_string() };
    let k = KeyResult {
        app: ctx.app.clone(), site: site.clone(), title: ctx.window_title.clone(), case: format!("{:?}", o.case).to_lowercase(),
        user_text, output: o.text.clone(), recipient,
    };
    let stored = memory::db_path().ok_or("no folder".to_string()).and_then(|p| Store::open(&p)).and_then(|st| st.add_key_result(&k, memory::now()));
    match stored {
        Ok(id) => memory::watch_send(id, site),
        Err(e) => println!("  memory: couldn't keep the result: {e}"),
    }
}

/// The saved Gopu key and backup shortcut take over from now on.
fn apply_keys(s: &Settings) {
    platform::set_keys(s.key_vk(), s.chord.map(|c| (c.mods, c.vk)));
}

/// Show the welcome flow at launch until it has finished, and again whenever no model is connected (Gopu can't
/// write without one).
fn needs_welcome(s: &Settings) -> bool {
    let provider = s.resolve(gopu_core::setting).provider;
    let needs_key = gopu_core::settings::provider(&provider).is_some_and(|p| p.needs_key);
    !s.onboarded || (needs_key && gopu_core::settings::api_key(&provider).is_none())
}

/// The welcome flow (ui/welcome.html): Gopu standing on the taskbar at the bottom right, talking in his bubble.
fn open_welcome(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("welcome") {
        let _ = w.set_focus();
        return;
    }
    const W: f64 = 470.0;
    const H: f64 = 560.0; // room for the tallest step; the page is transparent around the bubble
    let Ok(w) = WebviewWindowBuilder::new(app, "welcome", WebviewUrl::App("welcome.html".into()))
        .title("Welcome to Gopu")
        .inner_size(W, H)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .build()
    else {
        return;
    };
    // bottom right, feet on the taskbar's top edge: the bottom of the work area, asked from Windows itself
    // (the window's real size in physical pixels, so display scaling can't shift it)
    if let (Some(area), Ok(size)) = (platform::work_area(), w.outer_size()) {
        let x = area.x + area.w - size.width as i32 - 16;
        let y = area.y + area.h - size.height as i32;
        let _ = w.set_position(PhysicalPosition::new(x, y));
    }
    let _ = w.show();
    let _ = w.set_focus();
}

/// Closes the welcome flow. `finished`: remember it's done; otherwise it comes back on the next launch.
#[tauri::command]
fn welcome_done(app: AppHandle, finished: bool) -> Result<(), String> {
    if finished {
        let mut s = Settings::load();
        s.onboarded = true;
        s.save()?;
    }
    if let Some(w) = app.get_webview_window("welcome") {
        let _ = w.close();
    }
    Ok(())
}

/// The settings window, for "More…" in the welcome flow. Async, like every command that may open a window: a sync
/// command runs on the main thread, and creating a window there deadlocks on Windows.
#[tauri::command]
async fn show_settings(app: AppHandle) {
    open_settings(&app, None);
}

/// Only Gopu's own links, so a page can't open anything it likes.
#[tauri::command]
fn open_link(which: String) -> Result<(), String> {
    match which.as_str() {
        "groq-keys" => platform::shell_open("https://console.groq.com/keys"),
        _ => Err("unknown link".into()),
    }
}

/// The settings window (ui/settings.html): opened from the tray, or brought forward if it's already open.
/// `page`: open on that page ("search" for the tray's Search memory).
fn open_settings(app: &AppHandle, page: Option<&str>) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.unminimize();
        let _ = w.set_focus();
        if let Some(p) = page {
            let _ = w.eval(&format!("showPage('{p}')"));
        }
        return;
    }
    let url = format!("settings.html{}", page.map(|p| format!("#{p}")).unwrap_or_default());
    let _ = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App(url.into()))
        .title("Gopu settings")
        .inner_size(760.0, 600.0)
        .min_inner_size(620.0, 460.0)
        .center()
        .build();
}

/// Saved settings and the provider presets, for the window. Keys never leave the credential store, only whether one exists.
#[tauri::command]
fn settings_get() -> serde_json::Value {
    let providers: Vec<_> = gopu_core::settings::PROVIDERS
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id, "name": p.name, "base_url": p.base_url, "model": p.model,
                "needs_key": p.needs_key, "has_key": gopu_core::settings::api_key(p.id).is_some(),
            })
        })
        .collect();
    let keys: Vec<_> = gopu_core::settings::KEYS.iter().map(|(vk, name)| serde_json::json!({ "vk": vk, "name": name })).collect();
    serde_json::json!({ "saved": Settings::load(), "providers": providers, "keys": keys, "version": env!("CARGO_PKG_VERSION") })
}

/// Opens the log folder (created if needed) in File Explorer.
#[tauri::command]
fn open_logs() -> Result<(), String> {
    let dir = platform::log_dir().ok_or("no log folder")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    platform::shell_open(&dir)
}

/// Saves the settings; `key` None leaves the provider's saved key alone, "" removes it. The Gopu key and backup
/// shortcut apply at once.
/// Async: it may create the Ask Gopu tab (see `show_settings`).
#[tauri::command]
async fn settings_save(app: AppHandle, mut saved: Settings, key: Option<String>) -> Result<(), String> {
    // the tray, the welcome flow and the memory commands own these; the windows don't send them
    let now = Settings::load();
    (saved.paused, saved.onboarded) = (now.paused, now.onboarded);
    (saved.memory_off, saved.memory_paused_until, saved.ignore, saved.keep_days) =
        (now.memory_off, now.memory_paused_until, now.ignore, now.keep_days);
    (saved.mcp_on, saved.mcp_blocked) = (now.mcp_on, now.mcp_blocked); // the Privacy page sets these at once
    (saved.ask_pinned, saved.ask_at, saved.ask_tab_at, saved.ask_tab_edge) = (now.ask_pinned, now.ask_at, now.ask_tab_at, now.ask_tab_edge);
    (saved.ask_saved, saved.ask_reminder, saved.reminders) = (now.ask_saved, now.ask_reminder, now.reminders); // the box and the Style page set these
    if let Some(c) = saved.chord {
        if !platform::chord_free(c.mods, c.vk) {
            return Err("that shortcut is already used by Windows or another app; pick another".into());
        }
    }
    if let Some(key) = key {
        let provider = saved.resolve(|_| None).provider;
        gopu_core::settings::set_api_key(&provider, Some(key.trim()).filter(|k| !k.is_empty()))?;
    }
    saved.save()?;
    apply_autostart(&saved);
    anchor::apply(&app);
    apply_keys(&saved);
    memory_ui::refresh_tray(&app);
    Ok(())
}

/// One tiny call with the window's unsaved settings; the time it took on success.
#[tauri::command]
async fn settings_test(saved: Settings, key: Option<String>) -> Result<u128, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let r = saved.resolve(gopu_core::setting);
        let llm = match key.filter(|k| !k.trim().is_empty()) {
            Some(k) => Llm::new(&r).with_key(k.trim().into()),
            None => Llm::new(&r),
        };
        let t0 = std::time::Instant::now();
        llm.chat("Reply with the single word OK.", "Connection test").map(|_| t0.elapsed().as_millis())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// What Gopu will run with, on the console at start.
fn report(s: &Resolved) {
    let key = if Llm::new(s).has_key() { "found" } else { "none" };
    println!("LLM: {} at {} via {} (key {key})", s.model, s.base_url, s.provider);
    println!("User name: {}", s.user_name.as_deref().unwrap_or("not set (replies can mix up who is who)"));
}

/// The bubble while writing (brand decision 15).
fn writing_label(case: FieldCase) -> &'static str {
    match case {
        FieldCase::Empty => "Writing a reply",
        FieldCase::Instruction | FieldCase::AtCaret => "On it",
        FieldCase::Draft | FieldCase::Selection => "Polishing",
    }
}

/// The overlay for one run: spans the field's width so Gopu can walk along the line it stands on (the field's top
/// border, or the caret's line in a tall field). The page's line sits at `LINE_Y` (see ui/overlay.html, #wrap).
// ponytail: the window stays shown (transparent, click-through) and only its content appears; showing and
// hiding a no-activate window risks stealing focus. Scale uses the monitor the window is on before the move,
// so mixed-DPI setups can be off for one tap; see the map's Fog.
struct Stage<'a> {
    window: &'a WebviewWindow,
    left: i32,
    scale: f64,
}

impl<'a> Stage<'a> {
    const LINE_Y: f64 = 70.0;
    const HEIGHT: f64 = 100.0;
    const PAD: f64 = 90.0; // room for the bubble and the walk past the field's ends

    fn place(window: &'a WebviewWindow, field: &Field) -> Self {
        let scale = window.scale_factor().unwrap_or(1.0);
        let left = field.rect.x - (Self::PAD * scale) as i32;
        let width = (field.rect.w as f64 / scale + 2.0 * Self::PAD).min(3000.0);
        let _ = window.set_size(LogicalSize::new(width, Self::HEIGHT));
        let _ = window.set_position(PhysicalPosition::new(left, field.stand_y() - (Self::LINE_Y * scale) as i32));
        Stage { window, left, scale }
    }

    /// A screen x (physical px) in the page's coordinates.
    fn x(&self, screen_x: i32) -> f64 {
        (screen_x - self.left) as f64 / self.scale
    }

    fn js(&self, code: &str) {
        let _ = self.window.eval(code);
    }
}

/// The last tap, written to the log folder only with the debug switch on (About screen, or `GOPU_DEBUG` for dev):
/// it holds screen text. With the switch off, a dump left from before is deleted.
fn dump(field: &Field, ctx: &OnScreenContext, recall: Option<&str>, outcome: &Result<Outcome, String>, llm_ms: u128) {
    let result = match outcome {
        Ok(o) => format!("===== RESULT: {:?} ({llm_ms} ms) =====\n{}", o.case, o.text),
        Err(e) => format!("===== ERROR ({llm_ms} ms) =====\n{e}"),
    };
    let text = format!(
        "app: {} | {}\nwalk: {} ms, field found in tree: {}\nbefore: {} chars, after: {} chars\n\n{result}\n\n\
         ===== BEFORE =====\n{}\n\n===== FIELD (selection: {:?}; text escaped so invisible characters show) =====\n{:?}\n\n===== AFTER =====\n{}\n\n===== RECALL =====\n{}\n",
        ctx.app, ctx.window_title, ctx.took_ms, ctx.anchored,
        ctx.before.chars().count(), ctx.after.chars().count(),
        ctx.before, field.selection, field.text, ctx.after, recall.unwrap_or("(none)")
    );
    if let Some(dir) = platform::log_dir() {
        let path = dir.join("last-tap.txt");
        if Settings::load().debug || gopu_core::setting("GOPU_DEBUG").is_some() {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(path, text);
        } else {
            let _ = std::fs::remove_file(path);
        }
    }
    let summary = match outcome {
        Ok(o) => format!("{:?} in {llm_ms} ms: {}", o.case, o.text.replace('\n', " ⏎ ")),
        Err(e) => format!("ERROR: {e}"),
    };
    let recall = recall.map_or("no recall".to_string(), |r| {
        format!("recall {} chars{}", r.chars().count(), if r.starts_with("<person>") { ", with a people page" } else { "" })
    });
    println!("tap in {} (walk {} ms, {recall}) -> {summary}", ctx.app, ctx.took_ms);
}

/// Start with Windows: the installed app keeps its Startup entry in step with the setting. Dev builds never add one
/// (it would point at `target\debug`).
fn apply_autostart(s: &Settings) {
    if cfg!(debug_assertions) {
        return;
    }
    if let Err(e) = platform::set_autostart(!s.autostart_off) {
        println!("start with Windows: {e}");
    }
}
