//! Throwaway probe (memory slice 5): where does WhatsApp Web put the open chat's name in the UIA tree?
//! `cargo run -p gopu-core --example whatsapp_probe -- "<chat name>"`. Finds the Chrome/Edge window showing WhatsApp and
//! prints the elements around the given chat name and any selected list item: depth, control type, ARIA role, selected.
//! Every other name is masked to its length, so no message text is printed.
use windows::core::BOOL;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::*;

fn main() {
    let want = std::env::args().nth(1).unwrap_or_default();
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
        unsafe extern "system" fn cb(h: HWND, lp: LPARAM) -> BOOL {
            let mut buf = [0u16; 256];
            let n = GetWindowTextW(h, &mut buf) as usize;
            if String::from_utf16_lossy(&buf[..n]).contains("WhatsApp") && IsWindowVisible(h).as_bool() {
                *(lp.0 as *mut HWND) = h;
                return FALSE;
            }
            TRUE
        }
        let mut hwnd = HWND::default();
        let _ = EnumWindows(Some(cb), LPARAM(&mut hwnd as *mut _ as isize));
        if hwnd.is_invalid() {
            return println!("no window titled WhatsApp");
        }
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).unwrap();
        let req = uia.CreateCacheRequest().unwrap();
        for id in [UIA_NamePropertyId, UIA_ControlTypePropertyId, UIA_AriaRolePropertyId, UIA_SelectionItemIsSelectedPropertyId, UIA_IsOffscreenPropertyId] {
            req.AddProperty(id).unwrap();
        }
        req.AddPattern(UIA_SelectionItemPatternId).unwrap();
        req.SetTreeScope(TreeScope_Subtree).unwrap();
        req.SetTreeFilter(&uia.ControlViewCondition().unwrap()).unwrap();
        req.SetAutomationElementMode(AutomationElementMode_None).unwrap();
        let root = uia.ElementFromHandle(hwnd).unwrap().BuildUpdatedCache(&req).unwrap();
        let mut rows: Vec<(usize, i32, String, bool, String)> = vec![]; // depth, type, role, selected, name
        unsafe fn walk(e: &IUIAutomationElement, d: usize, rows: &mut Vec<(usize, i32, String, bool, String)>) {
            let ty = e.CachedControlType().map(|t| t.0).unwrap_or(0);
            let role = e.CachedAriaRole().map(|b| b.to_string()).unwrap_or_default();
            let sel = e
                .GetCachedPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId)
                .and_then(|p| p.CachedIsSelected())
                .map(|b| b.as_bool())
                .unwrap_or(false);
            let name = e.CachedName().map(|b| b.to_string()).unwrap_or_default();
            rows.push((d, ty, role, sel, name));
            if let Ok(k) = e.GetCachedChildren() {
                for i in 0..k.Length().unwrap_or(0) {
                    if let Ok(c) = k.GetElement(i) {
                        walk(&c, d + 1, rows);
                    }
                }
            }
        }
        walk(&root, 0, &mut rows);
        println!("{} elements", rows.len());
        // what Gopu itself would decide for this window
        let lines: Vec<String> = gopu_core::platform::read_window(hwnd.0).map(|w| w.text.lines().map(String::from).collect()).unwrap_or_default();
        println!("chat_header on the real window: {:?}", gopu_core::people::chat_header(&lines));
        let show = |n: &str| if n == want || n.ends_with(", You") || n.trim().is_empty() { n.to_string() } else { format!("<{} chars>", n.chars().count()) };
        let hits: Vec<usize> = rows.iter().enumerate().filter(|(_, r)| r.4 == want || r.3 || r.4.ends_with(", You")).map(|(i, _)| i).collect();
        for &i in &hits {
            println!("--- around element {i} ({:.0}% through the walk)", 100.0 * i as f64 / rows.len() as f64);
            for (j, r) in rows.iter().enumerate().take(i + 4).skip(i.saturating_sub(4)) {
                println!("{}{} d={} type={} role={:?} selected={} name={}", if j == i { "> " } else { "  " }, j, r.0, r.1, r.2, r.3, show(&r.4));
            }
        }
    }
}
