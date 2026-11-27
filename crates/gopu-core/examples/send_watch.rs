//! Throwaway probe for memory ticket 07: `cargo run -p gopu-core --example send_watch --release [secs]`
//! Finds the editable fields (UIA Edit, keyboard-focusable) in the Discord and Chrome main windows, times one field read
//! via TextPattern and via ValuePattern on a HELD element, then re-reads the held element every 500 ms for `secs`
//! (default 20) and reports when its length changes, when it goes empty, and whether the held reference goes stale.
//! Never types or sends anything. Prints lengths and timings only, never text.
use std::time::{Duration, Instant};
use windows::core::BOOL;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const APPS: &[&str] = &["Discord.exe", "chrome.exe", "msedge.exe"];

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

fn top_windows() -> Vec<(String, HWND)> {
    unsafe extern "system" fn cb(h: HWND, lp: LPARAM) -> BOOL {
        let out = &mut *(lp.0 as *mut Vec<HWND>);
        if IsWindowVisible(h).as_bool() && GetWindowTextLengthW(h) > 0 && GetWindow(h, GW_OWNER).is_err() {
            out.push(h);
        }
        TRUE
    }
    let mut all: Vec<HWND> = vec![];
    unsafe { let _ = EnumWindows(Some(cb), LPARAM(&mut all as *mut _ as isize)); }
    all.into_iter()
        .filter_map(|h| {
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
            let e = exe(pid);
            APPS.iter().any(|a| e.eq_ignore_ascii_case(a)).then_some((e, h))
        })
        .collect()
}

/// Length of the field text via TextPattern, then via ValuePattern: (chars, ms) each, None if unsupported/failed.
unsafe fn read(el: &IUIAutomationElement) -> (Option<(usize, f64)>, Option<(usize, f64)>) {
    let t = Instant::now();
    let tp = el
        .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        .ok()
        .and_then(|p| p.DocumentRange().ok()?.GetText(20_000).ok())
        .map(|b| (b.to_string().chars().count(), t.elapsed().as_secs_f64() * 1e3));
    let t = Instant::now();
    let vp = el
        .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
        .ok()
        .and_then(|p| p.CurrentValue().ok())
        .map(|b| (b.to_string().chars().count(), t.elapsed().as_secs_f64() * 1e3));
    (tp, vp)
}

fn main() {
    let secs: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(20);
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).unwrap();
        let req = uia.CreateCacheRequest().unwrap();
        for id in [UIA_ControlTypePropertyId, UIA_IsKeyboardFocusablePropertyId, UIA_IsPasswordPropertyId] {
            req.AddProperty(id).unwrap();
        }
        req.SetTreeScope(TreeScope_Subtree).unwrap();
        req.SetTreeFilter(&uia.ControlViewCondition().unwrap()).unwrap();
        let mut held = vec![];
        for (app, h) in top_windows() {
            let t = Instant::now();
            let Ok(root) = uia.ElementFromHandle(h).and_then(|r| r.BuildUpdatedCache(&req)) else { continue };
            let find_ms = t.elapsed().as_secs_f64() * 1e3;
            let mut found = vec![];
            fn edits(e: &IUIAutomationElement, out: &mut Vec<IUIAutomationElement>) {
                unsafe {
                    if e.CachedControlType().ok() == Some(UIA_EditControlTypeId)
                        && e.CachedIsKeyboardFocusable().map(|b| b.as_bool()).unwrap_or(false)
                        && !e.CachedIsPassword().map(|b| b.as_bool()).unwrap_or(true)
                    {
                        out.push(e.clone());
                    }
                    if let Ok(k) = e.GetCachedChildren() {
                        (0..k.Length().unwrap_or(0)).filter_map(|i| k.GetElement(i).ok()).for_each(|c| edits(&c, out));
                    }
                }
            }
            edits(&root, &mut found);
            let n = found.len();
            println!("{app}: {n} editable fields (cached walk {find_ms:.0} ms)");
            for el in found {
                let fw = el.CurrentFrameworkId().map(|b| b.to_string()).unwrap_or_default();
                let cls = el.CurrentClassName().map(|b| b.to_string()).unwrap_or_default();
                let (tp, vp) = read(&el);
                // a short value is a placeholder, not a message: show its code points so "empty" can be defined
                if let Some(b) = el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId).ok().and_then(|p| p.DocumentRange().ok()?.GetText(8).ok()) {
                    let s = b.to_string();
                    if s.chars().count() <= 4 {
                        println!("    short text code points: {:?}", s.chars().map(|c| format!("U+{:04X}", c as u32)).collect::<Vec<_>>());
                    }
                }
                println!("  #{} fw={fw} class={cls:.30} text={tp:?} value={vp:?}", held.len());
                held.push((app.clone(), el, tp.or(vp).map(|x| x.0)));
            }
        }
        println!("watching {} held fields for {secs}s (every 500 ms)", held.len());
        let t0 = Instant::now();
        let mut ms = vec![];
        while t0.elapsed() < Duration::from_secs(secs) {
            std::thread::sleep(Duration::from_millis(500));
            for (i, (app, el, last)) in held.iter_mut().enumerate() {
                let alive = el.CurrentBoundingRectangle().is_ok();
                let t = Instant::now();
                let (tp, vp) = read(el);
                ms.push(t.elapsed().as_secs_f64() * 1e3);
                let now = tp.or(vp).map(|x| x.0);
                if now != *last {
                    let s = t0.elapsed().as_secs_f64();
                    let what = match (*last, now) {
                        (Some(a), Some(0)) if a > 0 => "CLEARED",
                        (_, None) => "unreadable/stale",
                        _ => "changed",
                    };
                    println!("  {s:5.1}s #{i} {app}: {last:?} -> {now:?} chars ({what}, element alive={alive})");
                    *last = now;
                }
            }
        }
        ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if !ms.is_empty() {
            println!("held-field reads: n={} median {:.1} ms, p95 {:.1} ms, max {:.1} ms", ms.len(), ms[ms.len() / 2], ms[ms.len() * 95 / 100], ms[ms.len() - 1]);
        }
    }
}
