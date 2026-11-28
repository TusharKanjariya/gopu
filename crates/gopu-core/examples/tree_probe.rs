//! Why does an app give memory only its title (#2)? `cargo run -p gopu-core --example tree_probe -- sublime_text`
//! Finds the app's visible top-level window and prints its raw UIA tree: depth, control type, class, the length of each
//! name and value, and the length of any TextPattern text. Lengths only, so nothing private is printed.
use windows::core::{BOOL, Interface};
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::*;

static mut WANT: String = String::new();

fn main() {
    unsafe {
        WANT = std::env::args().nth(1).expect("an exe name").to_lowercase().trim_end_matches(".exe").to_string();
        CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
        unsafe extern "system" fn cb(h: HWND, lp: LPARAM) -> BOOL {
            let mut pid = 0;
            GetWindowThreadProcessId(h, Some(&mut pid));
            let Ok(p) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return TRUE };
            let mut buf = [0u16; 260];
            let mut n = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut n).is_ok();
            let _ = CloseHandle(p);
            let exe = String::from_utf16_lossy(&buf[..n as usize]).to_lowercase();
            #[allow(static_mut_refs)]
            if ok && exe.ends_with(&format!("\\{}.exe", WANT)) && IsWindowVisible(h).as_bool() && GetWindowTextLengthW(h) > 0 {
                *(lp.0 as *mut HWND) = h;
                return FALSE;
            }
            TRUE
        }
        let mut hwnd = HWND::default();
        let _ = EnumWindows(Some(cb), LPARAM(&mut hwnd as *mut _ as isize));
        if hwnd.is_invalid() {
            return println!("no visible window for that exe");
        }
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).unwrap();
        let walker = uia.RawViewWalker().unwrap();
        let root = uia.ElementFromHandle(hwnd).unwrap();
        let mut count = 0;
        walk(&walker, &root, 0, &mut count);
        println!("{count} elements");
    }
}

unsafe fn walk(w: &IUIAutomationTreeWalker, e: &IUIAutomationElement, depth: usize, count: &mut usize) {
    *count += 1;
    if *count > 400 || depth > 25 {
        return;
    }
    let ty = e.CurrentLocalizedControlType().map(|b| b.to_string()).unwrap_or_default();
    let class = e.CurrentClassName().map(|b| b.to_string()).unwrap_or_default();
    let name = e.CurrentName().map(|b| b.to_string().chars().count()).unwrap_or(0);
    let value = e
        .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
        .and_then(|v| v.CurrentValue())
        .map(|b| b.to_string().chars().count())
        .ok();
    let text = e
        .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        .and_then(|t| t.DocumentRange())
        .and_then(|r| r.GetText(-1))
        .map(|b| b.to_string().chars().count())
        .ok();
    let fw = e.CurrentFrameworkId().map(|b| b.to_string()).unwrap_or_default();
    println!("{}{ty} [{class}] {fw} name={name} value={value:?} textpattern={text:?}", "  ".repeat(depth));
    let mut c = w.GetFirstChildElement(e).ok();
    while let Some(k) = c {
        walk(w, &k, depth + 1, count);
        c = w.GetNextSiblingElement(&k).ok();
    }
    let _ = Interface::as_raw(e);
}
