//! Throwaway bench for memory ticket 02: `cargo run -p gopu-core --example capture_bench --release`
//! Times the slice-2 reader (one UIA cache request for a window subtree) on the main window of each heavy app,
//! measures our CPU and the target's CPU, then counts WinEvents for 15 s. Prints numbers only, never captured text.
//! `cached_tree` is copied from platform/windows.rs (it reads the foreground window; this reads background ones).
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::{Duration, Instant};
use windows::core::BOOL;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const APPS: &[&str] = &["Code.exe", "Discord.exe", "firefox.exe", "sublime_text.exe", "chrome.exe", "msedge.exe"];
const RUNS: usize = 10;

unsafe fn cached_tree(uia: &IUIAutomation, hwnd: HWND) -> Option<IUIAutomationElement> {
    let req = uia.CreateCacheRequest().ok()?;
    for id in [UIA_NamePropertyId, UIA_ControlTypePropertyId, UIA_IsPasswordPropertyId, UIA_IsOffscreenPropertyId, UIA_BoundingRectanglePropertyId] {
        req.AddProperty(id).ok()?;
    }
    req.SetTreeScope(TreeScope_Subtree).ok()?;
    req.SetTreeFilter(&uia.ControlViewCondition().ok()?).ok()?;
    req.SetAutomationElementMode(AutomationElementMode_None).ok()?;
    uia.ElementFromHandle(hwnd).ok()?.BuildUpdatedCache(&req).ok()
}

/// (nodes, visible leaf chars, hash of the leaf text)
unsafe fn walk(el: &IUIAutomationElement, n: &mut usize, chars: &mut usize, h: &mut DefaultHasher) {
    *n += 1;
    match el.GetCachedChildren().ok().filter(|k| k.Length().unwrap_or(0) > 0) {
        Some(k) => (0..k.Length().unwrap_or(0)).filter_map(|i| k.GetElement(i).ok()).for_each(|c| walk(&c, n, chars, h)),
        None if !el.CachedIsOffscreen().map(|b| b.as_bool()).unwrap_or(false) => {
            let s = el.CachedName().map(|b| b.to_string()).unwrap_or_default();
            *chars += s.chars().count();
            s.hash(h);
        }
        None => {}
    }
}

fn cpu_ms(h: HANDLE) -> f64 {
    let (mut a, mut b, mut k, mut u) = Default::default();
    unsafe { GetProcessTimes(h, &mut a, &mut b, &mut k, &mut u).ok() };
    let t = |f: FILETIME| ((f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64) as f64 / 10_000.0;
    t(k) + t(u)
}

fn exe(pid: u32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 1024];
        let mut n = buf.len() as u32;
        let _ = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut n);
        let _ = CloseHandle(h);
        String::from_utf16_lossy(&buf[..n as usize]).rsplit('\\').next().unwrap_or_default().to_string()
    }
}

/// The biggest visible, titled, unowned top-level window per app.
fn windows_by_app() -> Vec<(String, HWND, u32)> {
    unsafe extern "system" fn cb(h: HWND, lp: LPARAM) -> BOOL {
        let out = &mut *(lp.0 as *mut Vec<(HWND, u32)>);
        if IsWindowVisible(h).as_bool() && GetWindowTextLengthW(h) > 0 && GetWindow(h, GW_OWNER).is_err() {
            let mut pid = 0;
            GetWindowThreadProcessId(h, Some(&mut pid));
            out.push((h, pid));
        }
        TRUE
    }
    let mut all: Vec<(HWND, u32)> = vec![];
    unsafe { let _ = EnumWindows(Some(cb), LPARAM(&mut all as *mut _ as isize)); }
    APPS.iter()
        .filter_map(|app| all.iter().find(|(_, pid)| exe(*pid).eq_ignore_ascii_case(app)).map(|(h, p)| (app.to_string(), *h, *p)))
        .collect()
}

static COUNTS: std::sync::Mutex<Option<HashMap<u32, u64>>> = std::sync::Mutex::new(None);
unsafe extern "system" fn on_event(_: HWINEVENTHOOK, ev: u32, _: HWND, _: i32, _: i32, _: u32, _: u32) {
    *COUNTS.lock().unwrap().get_or_insert_with(HashMap::new).entry(ev).or_default() += 1;
}

fn main() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).unwrap();
        let me = GetCurrentProcess();
        println!("{:<18}{:>7}{:>9}{:>9}{:>9}{:>8}{:>9}{:>11}{:>11}", "app", "nodes", "chars", "first", "median", "max", "hash ms", "our cpu", "their cpu");
        for (app, hwnd, pid) in windows_by_app() {
            let target = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).unwrap_or_default();
            let (c0, t0) = (cpu_ms(me), cpu_ms(target));
            let mut times = vec![];
            let (mut nodes, mut chars, mut hash_ms, mut hashes) = (0, 0, 0.0, std::collections::HashSet::new());
            for _ in 0..RUNS {
                let s = Instant::now();
                let Some(root) = cached_tree(&uia, hwnd) else { break };
                times.push(s.elapsed().as_secs_f64() * 1000.0);
                let s = Instant::now();
                let mut h = DefaultHasher::new();
                (nodes, chars) = (0, 0);
                walk(&root, &mut nodes, &mut chars, &mut h);
                hash_ms += s.elapsed().as_secs_f64() * 1000.0;
                hashes.insert(h.finish());
            }
            if times.is_empty() { println!("{app:<18} read failed"); continue; }
            let first = times[0];
            times.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let n = times.len() as f64;
            println!("{app:<18}{nodes:>7}{chars:>9}{first:>8.0}m{:>8.0}m{:>7.0}m{:>9.1}{:>10.0}m{:>10.0}m  (distinct hashes {})",
                times[times.len() / 2], times[times.len() - 1], hash_ms / n, (cpu_ms(me) - c0) / n, (cpu_ms(target) - t0) / n, hashes.len());
            let _ = CloseHandle(target);
        }

        // the foreground window through the real slice-2 reader (field rect matches nothing, so all text is "before")
        let ctx = gopu_core::platform::on_screen_context(&gopu_core::Field::default());
        println!("\non_screen_context(foreground {}): {} ms, {} chars kept", ctx.app, ctx.took_ms, ctx.before.chars().count());

        // event rates, out of context, for 15 s
        let secs = 15;
        let evs = [
            (EVENT_SYSTEM_FOREGROUND, "SYSTEM_FOREGROUND"), (EVENT_OBJECT_FOCUS, "OBJECT_FOCUS"), (EVENT_OBJECT_NAMECHANGE, "OBJECT_NAMECHANGE"),
            (EVENT_OBJECT_VALUECHANGE, "OBJECT_VALUECHANGE"), (EVENT_OBJECT_REORDER, "OBJECT_REORDER"), (EVENT_OBJECT_SHOW, "OBJECT_SHOW"),
            (EVENT_OBJECT_LOCATIONCHANGE, "OBJECT_LOCATIONCHANGE"),
        ];
        let hooks: Vec<_> = evs.iter().map(|(e, _)| SetWinEventHook(*e, *e, None, Some(on_event), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS)).collect();
        println!("\ncounting WinEvents for {secs} s (use the machine normally)...");
        let end = Instant::now() + Duration::from_secs(secs);
        let mut msg = MSG::default();
        while Instant::now() < end {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() { DispatchMessageW(&msg); }
            std::thread::sleep(Duration::from_millis(10));
        }
        hooks.into_iter().for_each(|h| { let _ = UnhookWinEvent(h); });
        let counts = COUNTS.lock().unwrap().take().unwrap_or_default();
        for (e, name) in evs { println!("{name:<22}{:>8} total {:>8.1}/s", counts.get(&e).copied().unwrap_or(0), counts.get(&e).copied().unwrap_or(0) as f64 / secs as f64); }
    }
}
