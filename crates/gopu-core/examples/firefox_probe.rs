//! Throwaway probe for memory ticket 10: `cargo run -p gopu-core --example firefox_probe --release`
//! Reads every Firefox top-level window with the control, raw and content views, twice (the second read sees any
//! tree the first one woke up). Prints node and character counts only, never page text.
use std::time::{Duration, Instant};
use windows::core::BOOL;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::*;

unsafe fn tree(uia: &IUIAutomation, root: &IUIAutomationElement, cond: &IUIAutomationCondition) -> Option<IUIAutomationElement> {
    let req = uia.CreateCacheRequest().ok()?;
    for id in [UIA_NamePropertyId, UIA_ControlTypePropertyId, UIA_IsOffscreenPropertyId, UIA_FrameworkIdPropertyId, UIA_ProviderDescriptionPropertyId] {
        req.AddProperty(id).ok()?;
    }
    req.SetTreeScope(TreeScope_Subtree).ok()?;
    req.SetTreeFilter(cond).ok()?;
    req.SetAutomationElementMode(AutomationElementMode_Full).ok()?;
    root.BuildUpdatedCache(&req).ok()
}

#[derive(Default, Debug)]
struct Count { nodes: usize, chars: usize, docs: usize, docs_onscreen: usize, max_depth: usize, leaves: usize, leaves_offscreen: usize,
    leaf_chars_any: usize, all_name_chars: usize, onscreen_name_chars: usize, doc_text_chars: Vec<usize>, leaf_types: std::collections::BTreeMap<i32, usize> }

static VIS: std::sync::Mutex<Option<std::collections::BTreeMap<Vec<i32>, usize>>> = std::sync::Mutex::new(None);
static PATH: std::sync::Mutex<Vec<i32>> = std::sync::Mutex::new(Vec::new());
unsafe fn walk(el: &IUIAutomationElement, d: usize, c: &mut Count) {
    let mut path = PATH.lock().unwrap(); path.truncate(d); path.push(el.CachedControlType().unwrap_or_default().0); drop(path);
    c.nodes += 1;
    c.max_depth = c.max_depth.max(d);
    let off = el.CachedIsOffscreen().map(|b| b.as_bool()).unwrap_or(false);
    let name = el.CachedName().map(|b| b.len()).unwrap_or(0);
    c.all_name_chars += name;
    if !off { c.onscreen_name_chars += name; }
    if el.CachedControlType().unwrap_or_default() == UIA_DocumentControlTypeId {
        c.docs += 1;
        if !off {
            c.docs_onscreen += 1;
            println!("    onscreen document path (control types) {:?}", PATH.lock().unwrap());
            // live element only when the request kept it (mode Full)
            let n = el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId).ok()
                .and_then(|p| p.DocumentRange().ok()?.GetText(-1).ok()).map(|b| b.len()).unwrap_or(usize::MAX);
            c.doc_text_chars.push(n);
        }
    }
    match el.GetCachedChildren().ok().filter(|k| k.Length().unwrap_or(0) > 0) {
        Some(k) => (0..k.Length().unwrap_or(0)).filter_map(|i| k.GetElement(i).ok()).for_each(|e| walk(&e, d + 1, c)),
        None => {
            c.leaves += 1;
            c.leaf_chars_any += name;
            *c.leaf_types.entry(el.CachedControlType().unwrap_or_default().0).or_default() += 1;
            if off { c.leaves_offscreen += 1 } else { c.chars += name; if name > 0 && std::env::args().any(|a| a == "--paths") { *VIS.lock().unwrap().get_or_insert_with(Default::default).entry(PATH.lock().unwrap().clone()).or_default() += name; } }
        }
    }
}

fn exe(pid: u32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 1024];
        let mut n = buf.len() as u32;
        let _ = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut n);
        let _ = CloseHandle(h);
        String::from_utf16_lossy(&buf[..n as usize]).rsplit('\\').next().unwrap_or_default().to_lowercase()
    }
}

fn class(h: HWND) -> String {
    let mut b = [0u16; 256];
    let n = unsafe { GetClassNameW(h, &mut b) } as usize;
    String::from_utf16_lossy(&b[..n])
}

fn main() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).unwrap();
        unsafe extern "system" fn cb(h: HWND, lp: LPARAM) -> BOOL {
            let mut pid = 0;
            GetWindowThreadProcessId(h, Some(&mut pid));
            let want: Option<u32> = std::env::args().skip_while(|a| a != "--pid").nth(1).and_then(|v| v.parse().ok());
            if IsWindowVisible(h).as_bool() && exe(pid) == "firefox.exe" && want.map_or(true, |w| w == pid) { (*(lp.0 as *mut Vec<HWND>)).push(h); }
            TRUE
        }
        let mut wins: Vec<HWND> = vec![];
        let _ = EnumWindows(Some(cb), LPARAM(&mut wins as *mut _ as isize));
        let views = [("control", uia.ControlViewCondition().unwrap()), ("raw", uia.RawViewCondition().unwrap()), ("content", uia.ContentViewCondition().unwrap())];
        for h in wins {
            let mut r = RECT::default();
            let _ = GetWindowRect(h, &mut r);
            if std::env::args().any(|a| a == "--front") { let _ = SetForegroundWindow(h); std::thread::sleep(Duration::from_millis(1500)); }
            let mut kids = 0;
            unsafe extern "system" fn kc(_: HWND, lp: LPARAM) -> BOOL { *(lp.0 as *mut i32) += 1; TRUE }
            let _ = EnumChildWindows(Some(h), Some(kc), LPARAM(&mut kids as *mut _ as isize));
            println!("hwnd {:?} iconic {} class {} {}x{} child hwnds {} titled {} foreground {}", h.0, IsIconic(h).as_bool(), class(h), r.right - r.left, r.bottom - r.top,
                kids, GetWindowTextLengthW(h) > 0, GetForegroundWindow() == h);
            let Ok(root) = uia.ElementFromHandle(h) else { println!("  ElementFromHandle failed"); continue };
            println!("  framework {:?} provider {:?}", root.CurrentFrameworkId().map(|b| b.to_string()).unwrap_or_default(),
                root.CurrentProviderDescription().map(|b| b.to_string()).unwrap_or_default().chars().take(160).collect::<String>());
            if std::env::args().any(|a| a == "--paths") { let _ = tree(&uia, &root, &views[0].1).map(|t| walk(&t, 0, &mut Count::default())); }
            if std::env::args().any(|a| a == "--timeline") {
                // how the tree grows after the first UIA query of a fresh process
                {
                let field = if std::env::args().any(|a| a == "--default-field") { gopu_core::Field::default() } else { gopu_core::Field { rect: gopu_core::Rect { x: -7, y: -7, w: 1, h: 1 }, ..Default::default() } }; // the bench used Field::default(); the other matches no element
                let ctx = gopu_core::platform::on_screen_context(&field);
                println!("  on_screen_context({}): {} ms, before {} chars, after {} chars, anchored {}", ctx.app, ctx.took_ms, ctx.before.chars().count(), ctx.after.chars().count(), ctx.anchored);
                }
                let t0 = Instant::now();
                for _ in 0..4 {
                    let c = tree(&uia, &root, &views[0].1).map(|t| { let mut c = Count::default(); walk(&t, 0, &mut c); (c.nodes, c.chars, c.docs) });
                    println!("  t+{:>5} ms control (nodes, visible leaf chars, docs) {:?}", t0.elapsed().as_millis(), c);
                    std::thread::sleep(Duration::from_millis(250));
                }
                continue;
            }
            for (k, v) in VIS.lock().unwrap().take().unwrap_or_default() { println!("    visible chars {v:>5} under {k:?}"); }
            for pass in 1..=1 {
                for (name, cond) in &views {
                    let s = Instant::now();
                    let c = tree(&uia, &root, cond).map(|t| { let mut c = Count::default(); walk(&t, 0, &mut c); c });
                    println!("  pass {pass} {name:<8} {:>5.0} ms {:?}", s.elapsed().as_secs_f64() * 1000.0, c);
                }
                std::thread::sleep(Duration::from_millis(1500));
            }
        }
    }
}
