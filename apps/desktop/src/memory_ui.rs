//! Memory in the app: the tray's memory items, the tray icon and tooltip, and the commands behind the settings
//! window's Search and Privacy pages (memory-v1.1 slice 2). Words follow GLOSSARY.md.

use gopu_core::memory::{self, clock_text, now, today_start, Filter, Store};
use gopu_core::people::{self, Fact};
use gopu_core::privacy;
use gopu_core::settings::{MemoryState, Settings};
use serde_json::{json, Value};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Wry};

fn store() -> Result<Store, String> {
    Store::open(&memory::db_path().ok_or("no local app data folder")?)
}

/// Changes the saved settings in place.
fn update(change: impl FnOnce(&mut Settings)) -> Result<(), String> {
    let mut s = Settings::load();
    change(&mut s);
    s.save()
}

/// The tray menu for the current state: memory items first, then the Gopu key's pause, settings and quit.
fn tray_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let s = Settings::load();
    let item = |id: &str, text: &str| MenuItem::with_id(app, id, text, true, None::<&str>);
    let ask = item("ask", "Ask Gopu…")?;
    let search = item("mem_search", "Search memory…")?;
    let state: Box<dyn IsMenuItem<Wry>> = match s.memory_state(now()) {
        MemoryState::On => Box::new(Submenu::with_items(
            app,
            "Pause memory",
            true,
            &[&item("mem_pause_15", "For 15 minutes")?, &item("mem_pause_60", "For 1 hour")?, &item("mem_pause_tomorrow", "Until tomorrow")?],
        )?),
        MemoryState::Paused(until) => Box::new(item("mem_resume", &format!("Resume memory (paused until {})", clock_text(until)))?),
        MemoryState::Off => Box::new(item("mem_on", "Turn memory on")?),
    };
    let forget = item("mem_forget_15", "Forget the last 15 minutes")?;
    let sep = PredefinedMenuItem::separator(app)?;
    let key = CheckMenuItem::with_id(app, "pause", "Pause the Gopu key", true, s.paused, None::<&str>)?;
    let settings = item("settings", "Settings…")?;
    let update = match crate::update::ready() {
        Some(v) => item("update", &format!("Update to Gopu {v} and restart"))?,
        None => item("update_check", "Check for updates…")?,
    };
    let quit = item("quit", "Quit Gopu")?;
    let items: [&dyn IsMenuItem<Wry>; 9] = [&ask, &search, state.as_ref(), &forget, &sep, &key, &settings, &update, &quit];
    Menu::with_items(app, &items)
}

pub fn tooltip(s: &Settings) -> String {
    let key = if s.paused {
        "the Gopu key is paused".to_string()
    } else {
        format!("tap {} in any text field", gopu_core::settings::gopu_key_name(s.key_vk()))
    };
    let memory = match s.memory_state(now()) {
        MemoryState::On => "Gopu is remembering".to_string(),
        MemoryState::Paused(until) => format!("Memory paused until {}", clock_text(until)),
        MemoryState::Off => "Memory is off".to_string(),
    };
    format!("Gopu: {key}\n{memory}")
}

/// Rebuilds the tray's menu, tooltip and icon (dimmed while memory is paused or off) from the saved settings.
pub fn refresh_tray(app: &AppHandle) {
    let Some(tray) = app.tray_by_id("main") else { return };
    if let Ok(menu) = tray_menu(app) {
        let _ = tray.set_menu(Some(menu));
    }
    let s = Settings::load();
    let ready = crate::update::ready().map(|v| format!("
Gopu {v} is ready: update from this menu")).unwrap_or_default();
    let _ = tray.set_tooltip(Some(tooltip(&s) + &ready));
    if let Some(icon) = app.default_window_icon() {
        let on = s.memory_state(now()) == MemoryState::On;
        let _ = tray.set_icon(Some(if on { icon.clone() } else { dim(icon) }));
    }
}

fn dim(icon: &Image) -> Image<'static> {
    let mut px = icon.rgba().to_vec();
    for a in px.iter_mut().skip(3).step_by(4) {
        *a = (*a as u16 * 2 / 5) as u8;
    }
    Image::new_owned(px, icon.width(), icon.height())
}

/// The tray icon alone, normal or dimmed (Ask Gopu pulses it while thinking in taskbar mode).
pub fn tray_icon(app: &AppHandle, dimmed: bool) {
    if let (Some(tray), Some(icon)) = (app.tray_by_id("main"), app.default_window_icon()) {
        let _ = tray.set_icon(Some(if dimmed { dim(icon) } else { icon.clone() }));
    }
}

/// Refreshes the tray when a pause runs out on its own.
pub fn watch_pause(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last = None;
        loop {
            let state = Settings::load().memory_state(now());
            if last.is_some_and(|l| l != state) {
                refresh_tray(&app);
            }
            last = Some(state);
            std::thread::sleep(std::time::Duration::from_secs(20));
        }
    });
}

fn pause(until: Option<i64>) -> Result<(), String> {
    update(|s| s.memory_paused_until = until)
}

/// The tray's memory items. False when `id` isn't one of them.
pub fn on_menu(app: &AppHandle, id: &str) -> bool {
    let n = now();
    let done = match id {
        "mem_search" => {
            crate::open_settings(app, Some("search"));
            Ok(())
        }
        "mem_pause_15" => pause(Some(n + 15 * 60)),
        "mem_pause_60" => pause(Some(n + 3600)),
        "mem_pause_tomorrow" => pause(Some(today_start(n) + 86_400)),
        "mem_resume" => pause(None),
        "mem_on" => update(|s| (s.memory_off, s.memory_paused_until) = (false, None)),
        "mem_forget_15" => {
            memory::forget_clips_since(n - 15 * 60);
            store().and_then(|s| s.forget_since(n - 15 * 60)).map(|gone| println!("forgot {gone} captures"))
        }
        _ => return false,
    };
    if let Err(e) = done {
        println!("memory: {e}");
    }
    refresh_tray(app);
    true
}

// ---------------------------------------------------------------- commands for the settings window

/// Everything the Privacy page and the footer show.
#[tauri::command]
pub fn memory_state() -> Result<Value, String> {
    let s = Settings::load();
    let (state, until) = match s.memory_state(now()) {
        MemoryState::On => ("on", None),
        MemoryState::Paused(u) => ("paused", Some(u)),
        MemoryState::Off => ("off", None),
    };
    let st = store()?;
    let (bytes, since) = st.stats()?;
    Ok(json!({
        "state": state, "until": until, "until_text": until.map(clock_text), "bytes": bytes, "since": since,
        "keep_days": s.keep_days(), "ignore": s.ignore, "apps": st.apps()?,
    }))
}

/// `range`: "any", "today" or "week". A blank `q` lists the most recent captures.
#[tauri::command]
pub fn memory_search(q: String, range: String, app: Option<String>) -> Result<Value, String> {
    let n = now();
    let since = match range.as_str() {
        "today" => Some(today_start(n)),
        "week" => Some(n - 7 * 86_400),
        _ => None,
    };
    let hits = store()?.search(&q, &Filter { since, app: app.filter(|a| !a.is_empty()) }, 50)?;
    Ok(json!(hits
        .iter()
        .map(|h| json!({ "id": h.id, "app": h.app, "title": h.title, "site": h.site, "last_seen": h.last_seen, "snippet": h.snippet }))
        .collect::<Vec<_>>()))
}

#[tauri::command]
pub fn memory_get(id: i64) -> Result<Value, String> {
    let (app, title, site, text, first_seen, last_seen) = store()?.get(id)?.ok_or("Gopu has already forgotten that")?;
    Ok(json!({ "app": app, "title": title, "site": site, "text": text, "first_seen": first_seen, "last_seen": last_seen }))
}

/// `what`: "one" (with `id`), "15m", "hour", "today" or "all". Returns how many captures were forgotten.
#[tauri::command]
pub fn memory_forget(app: AppHandle, what: String, id: Option<i64>) -> Result<usize, String> {
    let (st, n) = (store()?, now());
    // the clipboard ring is memory too: a forgotten span forgets its copies
    match what.as_str() {
        "15m" => memory::forget_clips_since(n - 15 * 60),
        "hour" => memory::forget_clips_since(n - 3600),
        "today" => memory::forget_clips_since(today_start(n)),
        "all" => memory::forget_clips_since(0),
        _ => {}
    }
    let gone = match what.as_str() {
        "one" => st.forget(id.ok_or("which one?")?),
        "15m" => st.forget_since(n - 15 * 60),
        "hour" => st.forget_since(n - 3600),
        "today" => st.forget_since(today_start(n)),
        "all" => st.forget_all(),
        _ => Err("unknown".into()),
    }?;
    refresh_tray(&app);
    Ok(gone)
}

/// The ignore list. `add` without `confirm`: what the entry covers, nothing changed yet ({entry, count}).
/// `add` with `confirm`: forgets those captures and adds the entry. Not `add`: removes the entry.
#[tauri::command]
pub fn memory_ignore(entry: String, add: bool, confirm: bool) -> Result<Value, String> {
    let entry = privacy::ignore_entry(&entry).ok_or("type an app or a site")?;
    if !add {
        update(|s| s.ignore.retain(|e| *e != entry))?;
        return Ok(json!({ "entry": entry, "count": 0 }));
    }
    let count = store()?.ignored(&entry, confirm)?;
    if confirm {
        update(|s| {
            if !s.ignore.contains(&entry) {
                s.ignore.push(entry.clone());
            }
        })?;
    }
    Ok(json!({ "entry": entry, "count": count }))
}

/// Memory on or off, a pause ("15m", "1h", "tomorrow", or "resume"), and how long captures are kept.
#[tauri::command]
pub fn memory_set(app: AppHandle, on: Option<bool>, pause: Option<String>, keep_days: Option<u32>) -> Result<(), String> {
    let n = now();
    update(|s| {
        if let Some(on) = on {
            (s.memory_off, s.memory_paused_until) = (!on, None);
        }
        if let Some(p) = pause {
            s.memory_paused_until = match p.as_str() {
                "15m" => Some(n + 15 * 60),
                "1h" => Some(n + 3600),
                "tomorrow" => Some(today_start(n) + 86_400),
                _ => None,
            };
        }
        if let Some(d) = keep_days.filter(|d| [0, 7, 30, 90].contains(d)) {
            s.keep_days = Some(d);
        }
    })?;
    if keep_days.is_some() {
        let _ = store().and_then(|st| st.purge(n, Settings::load().keep_days())); // a shorter keep applies now
    }
    refresh_tray(&app);
    Ok(())
}

// ---------------------------------------------------------------- people pages

/// All people pages, most recently updated first.
#[tauri::command]
pub fn people_list() -> Result<Value, String> {
    Ok(json!(people::list(&store()?)?))
}

/// Saves the user's edits: from now on the model only adds facts to this page. `facts`: one per line, "YYYY-MM-DD fact".
#[tauri::command]
pub fn people_save(id: i64, summary: String, language: String, aliases: String, facts: String) -> Result<(), String> {
    let st = store()?;
    let mut p = people::get(&st, id)?.ok_or("that page is gone")?;
    p.summary = summary.trim().to_string();
    p.language = language.trim().to_string();
    p.aliases = aliases.split(',').map(str::trim).filter(|a| !a.is_empty()).map(String::from).collect();
    p.facts = facts
        .lines()
        .filter_map(|l| {
            let l = l.trim().trim_start_matches(['-', '•']).trim();
            let (date, fact) = l.split_once(char::is_whitespace)?;
            let fact = fact.trim().trim_start_matches([':', '—', '-']).trim();
            (date.len() == 10 && date.chars().all(|c| c.is_ascii_digit() || c == '-') && !fact.is_empty())
                .then(|| Fact { date: date.into(), fact: fact.into() })
        })
        .collect();
    p.edited = true;
    people::save(&st, &p, now()).map(|_| ())
}

/// Deletes a page; `never`: and never make one for that person again.
#[tauri::command]
pub fn people_delete(id: i64, never: bool) -> Result<(), String> {
    people::delete(&store()?, id, never)
}

// ---------------------------------------------------------------- style rules and the kept rate

/// The Style page: the kept rate per app or site, and the rules.
#[tauri::command]
pub fn style_state() -> Result<Value, String> {
    let st = store()?;
    let kept: Vec<Value> = gopu_core::style::kept_rates(&st, now())?
        .into_iter()
        .map(|(surface, pct, n, before_pct, before_n)| json!({ "surface": surface, "pct": pct, "n": n, "before_pct": before_pct, "before_n": before_n }))
        .collect();
    Ok(json!({ "kept": kept, "rules": gopu_core::style::list(&st)? }))
}

/// The user's edit of a rule: its words and whether it's on. The learner leaves it alone from then on.
#[tauri::command]
pub fn style_save(id: i64, rule: String, on: bool) -> Result<(), String> {
    if rule.trim().is_empty() {
        return Err("a rule needs some words".into());
    }
    gopu_core::style::save(&store()?, id, &rule, on, now())
}

#[tauri::command]
pub fn style_delete(id: i64) -> Result<(), String> {
    gopu_core::style::delete(&store()?, id)
}

// ---------------------------------------------------------------- MCP server (ticket 13)

/// The MCP section: on or off, the server's path for the configs, and each client from the access log.
#[tauri::command]
pub fn mcp_state() -> Result<Value, String> {
    let s = Settings::load();
    let exe = std::env::current_exe().map_err(|e| e.to_string())?.with_file_name("gopu-mcp.exe");
    let clients: Vec<Value> = gopu_core::mcp::clients(today_start(now()))
        .into_iter()
        .map(|(name, today, last)| json!({ "name": name, "today": today, "last": last, "blocked": s.mcp_blocked.contains(&name) }))
        .collect();
    Ok(json!({ "on": s.mcp_on, "exe": exe, "exe_found": exe.exists(), "clients": clients }))
}

/// Turns MCP on or off, or cuts a client off (`blocked`) or lets it back in.
#[tauri::command]
pub fn mcp_set(on: Option<bool>, client: Option<String>, blocked: Option<bool>) -> Result<(), String> {
    update(|s| {
        if let Some(on) = on {
            s.mcp_on = on;
        }
        if let (Some(c), Some(b)) = (client, blocked) {
            s.mcp_blocked.retain(|x| *x != c);
            if b {
                s.mcp_blocked.push(c);
            }
        }
    })
}
