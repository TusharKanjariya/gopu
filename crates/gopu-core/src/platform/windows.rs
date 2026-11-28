//! Windows platform layer. Answers from the v1 platform spike (ADR 0001 § "Spike results: Windows").

use crate::{keep_head, keep_tail, plausible_caret, Field, OnScreenContext, Rect, MAX_AFTER, MAX_BEFORE};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_NOREPEAT};
use std::sync::{mpsc, OnceLock};
use std::time::Duration;
use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, SAFEARRAY};
use windows::Win32::System::Ole::{SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetUBound, SafeArrayUnaccessData};
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::*;

// ---------------------------------------------------------------- the Gopu key

const MAX_HOLD_MS: u32 = 500;

/// The Gopu key (Right Ctrl until `set_keys` says otherwise) and the backup shortcut (0 = none).
static KEY_VK: AtomicU32 = AtomicU32::new(0xA3);
static CHORD_MODS: AtomicU32 = AtomicU32::new(0);
static CHORD_VK: AtomicU32 = AtomicU32::new(0);
/// The hook thread, which owns the backup shortcut's registration.
static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);
const HOTKEY_ID: i32 = 1;
const WM_APP_KEYS: u32 = WM_APP + 1;

#[derive(Debug, Clone, Copy)]
enum Ev {
    Key { vk: u32, down: bool, t: u32 },
    Mouse,
    /// The backup shortcut was pressed.
    Hotkey,
}

/// Uses `key` as the Gopu key and `chord` (modifier flags, key) as the backup shortcut from now on.
pub fn set_keys(key: u32, chord: Option<(u32, u32)>) {
    let (mods, vk) = chord.unwrap_or((0, 0));
    KEY_VK.store(key, Ordering::SeqCst);
    CHORD_MODS.store(mods, Ordering::SeqCst);
    CHORD_VK.store(vk, Ordering::SeqCst);
    let thread = HOOK_THREAD.load(Ordering::SeqCst);
    if thread != 0 {
        unsafe {
            let _ = PostThreadMessageW(thread, WM_APP_KEYS, WPARAM(0), LPARAM(0));
        }
    }
}

/// Can `mods`+`vk` be registered right now (no other app holds it)? Our own current shortcut counts as free.
pub fn chord_free(mods: u32, vk: u32) -> bool {
    if (mods, vk) == (CHORD_MODS.load(Ordering::SeqCst), CHORD_VK.load(Ordering::SeqCst)) {
        return true;
    }
    unsafe {
        let ok = RegisterHotKey(None, 0x7FFF, HOT_KEY_MODIFIERS(mods | MOD_NOREPEAT.0), vk).is_ok();
        if ok {
            let _ = UnregisterHotKey(None, 0x7FFF);
        }
        ok
    }
}

/// Lone-tap detector: the Gopu key down then up within 500ms with no other key, mouse button or wheel between.
#[derive(Default)]
struct Tap {
    down_at: Option<u32>,
    spoiled: bool,
}

impl Tap {
    fn feed(&mut self, ev: Ev) -> bool {
        match ev {
            Ev::Hotkey => return true,
            Ev::Mouse => self.spoiled |= self.down_at.is_some(),
            Ev::Key { vk, down, .. } if vk != KEY_VK.load(Ordering::SeqCst) => self.spoiled |= down && self.down_at.is_some(),
            Ev::Key { down: true, t, .. } => {
                if self.down_at.is_none() {
                    self.down_at = Some(t); // auto-repeat keeps the first timestamp
                    self.spoiled = false;
                }
            }
            Ev::Key { down: false, t, .. } => {
                if let Some(t0) = self.down_at.take() {
                    return !self.spoiled && t.wrapping_sub(t0) < MAX_HOLD_MS;
                }
            }
        }
        false
    }
}

static KEY_EVENTS: OnceLock<mpsc::Sender<Ev>> = OnceLock::new();
static BUSY: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);
const VK_ESC: u32 = 0x1B;

/// The error `write_back` returns when the user pressed Esc.
pub const STOPPED: &str = "stopped";

/// A run starts (true) or ends (false). While one runs, Esc cancels it and never reaches the app underneath.
pub fn busy(on: bool) {
    CANCEL.store(false, Ordering::SeqCst);
    BUSY.store(on, Ordering::SeqCst);
}

/// Esc was pressed during the current run.
pub fn cancelled() -> bool {
    CANCEL.load(Ordering::SeqCst)
}

unsafe extern "system" fn kb_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let k = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        // our own SendInput keys (write-back paste) never count
        if k.flags.0 & LLKHF_INJECTED.0 == 0 {
            // Esc during a run is Gopu's: swallowed, so the app doesn't also close or clear something
            if k.vkCode == VK_ESC && BUSY.load(Ordering::SeqCst) {
                if matches!(wp.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN) {
                    CANCEL.store(true, Ordering::SeqCst);
                }
                return LRESULT(1);
            }
            if let Some(tx) = KEY_EVENTS.get() {
                let down = matches!(wp.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
                let _ = tx.send(Ev::Key { vk: k.vkCode, down, t: k.time });
            }
        }
    }
    CallNextHookEx(None, code, wp, lp) // report only, never swallow (Esc during a run aside)
}

unsafe extern "system" fn mouse_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && matches!(wp.0 as u32, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN | WM_MOUSEWHEEL | WM_MOUSEHWHEEL)
    {
        if let Some(tx) = KEY_EVENTS.get() {
            let _ = tx.send(Ev::Mouse);
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

/// (Re)registers the backup shortcut for this thread. Must run on the hook thread.
unsafe fn register_chord() {
    let _ = UnregisterHotKey(None, HOTKEY_ID);
    let (mods, vk) = (CHORD_MODS.load(Ordering::SeqCst), CHORD_VK.load(Ordering::SeqCst));
    if vk != 0 && RegisterHotKey(None, HOTKEY_ID, HOT_KEY_MODIFIERS(mods | MOD_NOREPEAT.0), vk).is_err() {
        println!("backup shortcut is taken by another app; only the Gopu key works");
    }
}

/// Calls `on_tap` on a background thread each time the Gopu key is tapped or the backup shortcut pressed. Call once,
/// after `set_keys` for the saved keys.
pub fn on_gopu_key(on_tap: impl Fn() + Send + 'static) {
    let (tx, rx) = mpsc::channel();
    KEY_EVENTS.set(tx.clone()).expect("on_gopu_key called twice");
    // the hooks get their own thread and message loop so slow work can't make Windows drop them
    std::thread::spawn(move || unsafe {
        SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_hook), None, 0).expect("keyboard hook");
        SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), None, 0).expect("mouse hook");
        let mut msg = MSG::default();
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE); // the thread's queue exists before anyone posts to it
        HOOK_THREAD.store(windows::Win32::System::Threading::GetCurrentThreadId(), Ordering::SeqCst);
        register_chord();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            match msg.message {
                WM_HOTKEY => {
                    let _ = tx.send(Ev::Hotkey);
                }
                WM_APP_KEYS => register_chord(),
                _ => {
                    DispatchMessageW(&msg);
                }
            }
        }
    });
    std::thread::spawn(move || {
        let mut tap = Tap::default();
        for ev in rx {
            if tap.feed(ev) {
                on_tap();
            }
        }
    });
}

// ---------------------------------------------------------------- the focused field

/// The focused field's rectangle and caret, via UI Automation. Call from any non-UI thread.
pub fn focused_field() -> Option<Field> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED); // S_FALSE after the first call on this thread is fine
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        let mut el = uia.GetFocusedElement().ok()?;
        // Chromium builds its accessibility tree on the first UIA query: give it one beat, then ask again
        if el.CurrentFrameworkId().map(|f| f == "Chrome").unwrap_or(false) && text_pattern(&el).is_none() {
            std::thread::sleep(Duration::from_millis(150));
            el = uia.GetFocusedElement().ok()?;
        }
        if el.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(true) {
            return None; // Gopu never reads or writes password fields
        }
        let rect = to_rect(el.CurrentBoundingRectangle().ok()?);
        if rect.w <= 0 || rect.h <= 0 {
            return None;
        }
        let caret = caret_text_pattern2(&el)
            .or_else(|| caret_selection(&el))
            .or_else(caret_win32)
            .and_then(|c| plausible_caret(rect, c));
        let tp = text_pattern(&el);
        let has_text_pattern = tp.is_some();
        let text = tp
            .as_ref()
            .and_then(|p| p.DocumentRange().ok()?.GetText(MAX_FIELD_CHARS).ok())
            .or_else(|| {
                let v: IUIAutomationValuePattern = el.GetCurrentPatternAs(UIA_ValuePatternId).ok()?;
                v.CurrentValue().ok()
            })
            .map(|b| b.to_string())
            .unwrap_or_default();
        // Firefox stands each embedded element (a rich editor's paragraphs) in for one U+FFFC: read the real text
        let rebuilt = text.contains(OBJ);
        let text = if rebuilt { subtree_text(&uia, &el).unwrap_or_default() } else { text };
        // the text up to the caret: the document range from its start to the selection's start
        let before_caret = match &tp {
            _ if rebuilt => None, // rebuilt text doesn't line up with the app's ranges
            Some(p) => (|| {
                let caret = p.GetSelection().ok()?.GetElement(0).ok()?;
                let r = p.DocumentRange().ok()?;
                r.MoveEndpointByRange(TextPatternRangeEndpoint_End, &caret, TextPatternRangeEndpoint_Start).ok()?;
                Some(r.GetText(MAX_FIELD_CHARS).ok()?.to_string())
            })(),
            None => win32_sel().map(|(start, _)| String::from_utf16_lossy(&text.encode_utf16().take(start).collect::<Vec<_>>())),
        };
        let selection = match tp {
            Some(p) => (|| {
                let sel = p.GetSelection().ok()?;
                (sel.Length().ok()? > 0).then_some(())?;
                let r = sel.GetElement(0).ok()?;
                // a bare caret is no selection, whatever text the app hands back for it
                (r.CompareEndpoints(TextPatternRangeEndpoint_Start, &r, TextPatternRangeEndpoint_End).ok()? != 0).then_some(())?;
                r.GetText(MAX_FIELD_CHARS).ok()
            })()
            .map(|b| b.to_string().replace(OBJ, "\n"))
            .unwrap_or_default(),
            None => win32_selection(&text), // classic Edit controls (Windows 10 Notepad) have no TextPattern
        };
        // a value the app says is read-only (a browser page holds its address there) can't be typed into; with no
        // value at all, only a text pattern or a caret marks a field (rich editors often have just those)
        let read_only = el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId).and_then(|v| v.CurrentIsReadOnly()).map(|b| b.as_bool());
        let not_editable = read_only.unwrap_or(!has_text_pattern && caret.is_none());
        Some(Field { rect, caret, text, selection, before_caret, not_editable })
    }
}

const MAX_FIELD_CHARS: i32 = 20_000;
const OBJ: char = '\u{FFFC}'; // OBJECT REPLACEMENT CHARACTER

/// A rich editor's text from its children: one line per top-level child (paragraph), each the visible leaf text
/// joined. An empty editor (`<p><br></p>`) gives "".
unsafe fn subtree_text(uia: &IUIAutomation, el: &IUIAutomationElement) -> Option<String> {
    let req = uia.CreateCacheRequest().ok()?;
    req.AddProperty(UIA_NamePropertyId).ok()?;
    req.SetTreeScope(TreeScope_Subtree).ok()?;
    req.SetTreeFilter(&uia.ControlViewCondition().ok()?).ok()?;
    req.SetAutomationElementMode(AutomationElementMode_None).ok()?;
    let root = el.BuildUpdatedCache(&req).ok()?;
    unsafe fn leaves(e: &IUIAutomationElement, out: &mut String) {
        match e.GetCachedChildren().ok().filter(|k| k.Length().unwrap_or(0) > 0) {
            Some(k) => (0..k.Length().unwrap_or(0)).filter_map(|i| k.GetElement(i).ok()).for_each(|c| leaves(&c, out)),
            None => out.push_str(&e.CachedName().map(|b| b.to_string()).unwrap_or_default()),
        }
    }
    let kids = root.GetCachedChildren().ok()?;
    let lines: Vec<String> = (0..kids.Length().ok()?)
        .filter_map(|i| kids.GetElement(i).ok())
        .map(|c| {
            let mut s = String::new();
            leaves(&c, &mut s);
            s
        })
        .collect();
    Some(lines.join("\n"))
}

/// The selected text of a classic Win32 Edit/RichEdit control, asked with EM_GETSEL.
// ponytail: EM_GETSEL's return value packs 16-bit offsets, so selections past 65,535 characters read as none;
// pass pointers through a shared buffer if that ever matters
fn win32_selection(text: &str) -> String {
    let Some((start, end)) = win32_sel() else { return String::new() };
    let units: Vec<u16> = text.encode_utf16().collect();
    if start < end && end <= units.len() {
        String::from_utf16_lossy(&units[start..end])
    } else {
        String::new()
    }
}

/// The focused classic Edit control's selection as UTF-16 offsets (start == end: the caret), via EM_GETSEL.
fn win32_sel() -> Option<(usize, usize)> {
    const EM_GETSEL: u32 = 0x00B0;
    unsafe {
        let tid = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let mut gi = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        if GetGUIThreadInfo(tid, &mut gi).is_err() || gi.hwndFocus.is_invalid() {
            return None;
        }
        let mut r = 0usize;
        // a timeout, because a hung app must never hang the Gopu key
        if SendMessageTimeoutW(gi.hwndFocus, EM_GETSEL, WPARAM(0), LPARAM(0), SMTO_ABORTIFHUNG, 200, Some(&mut r)).0 == 0 {
            return None;
        }
        Some((r & 0xFFFF, (r >> 16) & 0xFFFF))
    }
}

// ---------------------------------------------------------------- the on-screen context

/// The focused window's visible text in document order, split around the field.
pub fn on_screen_context(field: &Field) -> OnScreenContext {
    let t0 = std::time::Instant::now();
    let fg = unsafe { GetForegroundWindow() };
    let mut w = Walk { field: field.rect, ..Default::default() };
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        if let Some(root) = cached_tree(fg, None) {
            w.visit(&root, false);
        }
    }
    OnScreenContext {
        app: process_name(fg),
        window_title: window_title(fg),
        before: keep_tail(&w.before, MAX_BEFORE),
        after: keep_head(&w.after, MAX_AFTER),
        anchored: w.anchored,
        chat_header: crate::people::chat_header(&w.before),
        url: w.url,
        took_ms: t0.elapsed().as_millis(),
    }
}

/// The whole window subtree in ONE cross-process call (a UIA cache request), not one call per element.
/// `timeout_ms` caps how long a slow or hung app can hold the call (UIA's default is 20 s).
unsafe fn cached_tree(hwnd: HWND, timeout_ms: Option<u32>) -> Option<IUIAutomationElement> {
    let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
    if let (Some(ms), Ok(u2)) = (timeout_ms, windows::core::Interface::cast::<IUIAutomation2>(&uia)) {
        let _ = u2.SetTransactionTimeout(ms);
        let _ = u2.SetConnectionTimeout(ms);
    }
    let req = uia.CreateCacheRequest().ok()?;
    for id in [
        UIA_NamePropertyId, UIA_ControlTypePropertyId, UIA_IsPasswordPropertyId, UIA_IsOffscreenPropertyId,
        UIA_BoundingRectanglePropertyId, UIA_ValueValuePropertyId, UIA_HasKeyboardFocusPropertyId,
    ] {
        req.AddProperty(id).ok()?;
    }
    req.AddPattern(UIA_ValuePatternId).ok()?; // a browser's address bar holds the page's address
    req.AddPattern(UIA_TextPatternId).ok()?; // a terminal holds its text here, not in its children
    req.SetTreeScope(TreeScope_Subtree).ok()?;
    req.SetTreeFilter(&uia.ControlViewCondition().ok()?).ok()?;
    req.SetAutomationElementMode(AutomationElementMode_None).ok()?;
    uia.ElementFromHandle(hwnd).ok()?.BuildUpdatedCache(&req).ok()
}

#[derive(Default)]
struct Walk {
    field: Rect,
    before: Vec<String>,
    after: Vec<String>,
    anchored: bool,
    nodes: usize,
    /// The address bar: the first edit or combo box in the app's chrome that holds an address.
    url: Option<String>,
}

impl Walk {
    const MAX_NODES: usize = 8000;

    /// `quiet`: inside app chrome (menus, toolbars, tabs, title and status bars). Still walked, because the field
    /// can live there (e.g. a browser address bar sits in a toolbar), but its text is dropped as noise.
    unsafe fn visit(&mut self, el: &IUIAutomationElement, quiet: bool) {
        self.nodes += 1;
        if self.nodes > Self::MAX_NODES || el.CachedIsPassword().map(|b| b.as_bool()).unwrap_or(false) {
            return;
        }
        let ty = el.CachedControlType().unwrap_or_default();
        if ty == UIA_ScrollBarControlTypeId {
            return;
        }
        let quiet = quiet
            || [
                // not Button: chat apps make usernames buttons, and who said what matters more than stray labels
                UIA_MenuBarControlTypeId, UIA_MenuControlTypeId, UIA_MenuItemControlTypeId, UIA_TitleBarControlTypeId,
                UIA_ToolBarControlTypeId, UIA_StatusBarControlTypeId, UIA_TabControlTypeId,
            ]
            .contains(&ty);
        // Chrome's address bar is an edit box, Firefox's a combo box; the first one holding an address wins. Not while
        // the user types in it: then it holds what they typed, not the page (a new tab read as the site being typed)
        if quiet && self.url.is_none() && (ty == UIA_EditControlTypeId || ty == UIA_ComboBoxControlTypeId) {
            let typing = el.CachedHasKeyboardFocus().map(|b| b.as_bool()).unwrap_or(false);
            self.url = value(el).filter(|v| !typing && crate::privacy::site_from_url(v).is_some());
        }
        // the field: everything after it goes in `after`; its own text comes from `Field`, so skip its subtree.
        // No field (capture): nothing to anchor on, all text goes in `before`.
        if !self.anchored && self.field.w > 0 && el.CachedBoundingRectangle().map(to_rect).ok() == Some(self.field) {
            self.anchored = true;
            return;
        }
        let hidden = quiet || el.CachedIsOffscreen().map(|b| b.as_bool()).unwrap_or(false);
        // an edit box holds its text in its value (its name is a label like "Text Editor"), and may have
        // scrollbar children, so it's read whole here, leaf or not
        if ty == UIA_EditControlTypeId && !hidden {
            if let Some(v) = value(el) {
                return self.push(&v);
            }
        }
        let kids = el.GetCachedChildren().ok().filter(|k| k.Length().unwrap_or(0) > 0);
        match kids {
            Some(kids) => {
                let had = self.before.len() + self.after.len();
                for i in 0..kids.Length().unwrap_or(0) {
                    if let Ok(k) = kids.GetElement(i) {
                        self.visit(&k, quiet);
                    }
                }
                // children with no words (a terminal's scroll bar and image): the text is the element's own, so read
                // the part on screen (Windows Terminal had only its title in memory, #2)
                if !hidden && !self.anchored && self.before.len() + self.after.len() == had {
                    if let Some(t) = visible_text(el) {
                        self.push(&t);
                    }
                }
            }
            // leaves carry the readable text; container names mostly repeat their children. A document leaf (a
            // plain-text editor) holds its text in its value. Not a document with children: a browser page's
            // value is its address.
            // the window's own caption buttons: apps that draw their own title bar (Chrome, VS Code) don't put them in
            // a title bar control, so their names led every capture ("Minimize Restore Close")
            None if ty == UIA_ButtonControlTypeId
                && el.CachedName().is_ok_and(|n| CAPTION_BUTTONS.contains(&n.to_string().trim())) => {}
            None if !hidden => {
                let text = (ty == UIA_DocumentControlTypeId).then(|| value(el)).flatten();
                self.push(&text.unwrap_or_else(|| el.CachedName().map(|b| b.to_string()).unwrap_or_default()));
            }
            None => {}
        }
    }

    /// Adds a line, unless it repeats the line just before it. Only the one before: a chat repeats a speaker's name
    /// each time they speak again, and dropping that name made their replies read as the user's (a Discord DM, 2026-11-24).
    fn push(&mut self, text: &str) {
        // icon-font glyphs (private use, e.g. VS Code's codicons) carry no words
        let text: String = text.chars().filter(|c| !('\u{E000}'..='\u{F8FF}').contains(c)).collect();
        let text = text.trim();
        let out = if self.anchored { &mut self.after } else { &mut self.before };
        if !text.is_empty() && out.last().map(String::as_str) != Some(text) {
            out.push(text.to_string());
        }
    }
}

// ponytail: English names only, like the rest of the UI in 0.1; other Windows languages keep them in captures
const CAPTION_BUTTONS: &[&str] = &["Minimize", "Maximize", "Restore", "Restore Down", "Close"];

/// The on-screen part of an element's TextPattern text, when it has any.
unsafe fn visible_text(el: &IUIAutomationElement) -> Option<String> {
    let ranges = el.GetCachedPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId).ok()?.GetVisibleRanges().ok()?;
    let text: Vec<String> =
        (0..ranges.Length().ok()?).filter_map(|i| ranges.GetElement(i).ok()?.GetText(MAX_CAPTURE as i32).ok()).map(|b| b.to_string()).collect();
    Some(text.join("\n")).filter(|t| !t.trim().is_empty())
}

/// An element's cached value (ValuePattern), when it has a non-blank one.
unsafe fn value(el: &IUIAutomationElement) -> Option<String> {
    let v = el.GetCachedPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId).and_then(|v| v.CachedValue()).ok()?;
    Some(v.to_string()).filter(|v| !v.trim().is_empty())
}

// ---------------------------------------------------------------- capture (memory)

/// The most text one capture keeps.
const MAX_CAPTURE: usize = 20_000;

/// The window in front, read for memory.
pub struct WindowText {
    pub app: String,
    pub title: String,
    /// The address bar's text, for browsers.
    pub url: Option<String>,
    /// Visible text in document order; password fields are never read.
    pub text: String,
}

/// Reads the window in front. None for Gopu's own windows, hung apps, or when the read fails or times out (2 s).
pub fn foreground_text() -> Option<WindowText> {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() || IsHungAppWindow(fg).as_bool() {
            return None;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        if pid == windows::Win32::System::Threading::GetCurrentProcessId() {
            return None;
        }
        read_window(fg.0)
    }
}

/// Reads any top-level window the way capture does (a 2 s timeout). For `foreground_text` and dev checks.
pub fn read_window(hwnd: *mut core::ffi::c_void) -> Option<WindowText> {
    let hwnd = HWND(hwnd);
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let root = cached_tree(hwnd, Some(2000))?;
        let mut w = Walk::default();
        w.visit(&root, false);
        Some(WindowText { app: process_name(hwnd), title: window_title(hwnd), url: w.url, text: keep_head(&w.before, MAX_CAPTURE) })
    }
}

// ---------------------------------------------------------------- send detection (memory-v1.1 ticket 07)

/// How a send watch ended.
#[derive(Debug, PartialEq)]
pub enum SendEnd {
    /// The field cleared and the window then showed its last text: sent. Holds that text.
    Sent(String),
    /// The field disappeared (a compose window closed). Holds its last text.
    Gone(String),
    /// Cleared without the text showing up, timed out, or a newer tap took over.
    Unseen,
}

/// Bumped by each new watch, so an older one stops.
static WATCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Holds the focused field (call right after a write-back) and reads it once a second for up to `max`, keeping its last
/// non-empty text. When it clears, the window is read at +1 s and +4 s for that text. Runs on the calling thread.
pub fn watch_send(max: Duration) -> SendEnd {
    let me = WATCH.fetch_add(1, Ordering::SeqCst) + 1;
    let squash = |s: &str| s.chars().filter(|c| !c.is_whitespace() && !matches!(c, '\u{200B}'..='\u{200F}' | '\u{2060}' | '\u{FEFF}')).count();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(uia) = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) else { return SendEnd::Unseen };
        if let Ok(u2) = windows::core::Interface::cast::<IUIAutomation2>(&uia) {
            let _ = u2.SetTransactionTimeout(2000);
            let _ = u2.SetConnectionTimeout(2000);
        }
        let hwnd = GetForegroundWindow();
        let Ok(mut el) = uia.GetFocusedElement() else { return SendEnd::Unseen };
        let read = |el: &IUIAutomationElement| -> Option<String> {
            text_pattern(el)
                .and_then(|p| p.DocumentRange().ok()?.GetText(MAX_FIELD_CHARS).ok())
                .or_else(|| el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId).ok()?.CurrentValue().ok())
                .map(|b| b.to_string())
        };
        let mut last = read(&el).unwrap_or_default();
        let t0 = std::time::Instant::now();
        while t0.elapsed() < max {
            std::thread::sleep(Duration::from_secs(1));
            if WATCH.load(Ordering::SeqCst) != me {
                return SendEnd::Unseen; // a newer tap took over
            }
            match read(&el) {
                Some(t) if squash(&t) > 0 => last = t,
                Some(_) if squash(&last) > 0 => {
                    // cleared: sent, if the thread now shows it
                    for wait in [1, 3] {
                        std::thread::sleep(Duration::from_secs(wait));
                        if window_text(hwnd).is_some_and(|w| crate::memory::shows_in(&w, &last)) {
                            return SendEnd::Sent(last);
                        }
                    }
                    return SendEnd::Unseen;
                }
                Some(_) => {}
                None => {
                    // the app may have rebuilt the composer (React re-mounts): take the focused field if it holds our text
                    let again = (GetForegroundWindow() == hwnd).then(|| uia.GetFocusedElement().ok()).flatten();
                    match again {
                        Some(e) if read(&e).is_some_and(|t| squash(&t) == squash(&last)) => el = e,
                        _ => return SendEnd::Gone(last),
                    }
                }
            }
        }
        SendEnd::Unseen
    }
}

/// A window's visible text (one cached read, ~2 s timeout).
fn window_text(hwnd: HWND) -> Option<String> {
    unsafe {
        let root = cached_tree(hwnd, Some(2000))?;
        let mut w = Walk::default();
        w.visit(&root, false);
        Some(w.before.join("\n"))
    }
}

/// Changes every time anything is copied.
pub fn clipboard_seq() -> u32 {
    unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() }
}

/// The clipboard's text, unless the app that copied it marked it private: password managers set the formats that keep
/// a copy out of clipboard monitors and history (Gopu's own write-back copies do too).
pub fn clipboard_text() -> Option<String> {
    use windows::Win32::System::DataExchange::{GetClipboardData, IsClipboardFormatAvailable, RegisterClipboardFormatW};
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    let format = |name: &str| unsafe { RegisterClipboardFormatW(&windows::core::HSTRING::from(name)) };
    let available = |f: u32| unsafe { IsClipboardFormatAvailable(f) }.is_ok();
    if ["ExcludeClipboardContentFromMonitorProcessing", "Clipboard Viewer Ignore"].iter().any(|n| available(format(n))) {
        return None;
    }
    let _open = clip::open(HWND::default()).ok()?;
    let bytes = |f: u32| -> Option<Vec<u8>> {
        unsafe {
            let h = GetClipboardData(f).ok()?;
            let g = windows::Win32::Foundation::HGLOBAL(h.0);
            let p = GlobalLock(g);
            if p.is_null() {
                return None;
            }
            let v = std::slice::from_raw_parts(p as *const u8, GlobalSize(g)).to_vec();
            let _ = GlobalUnlock(g);
            Some(v)
        }
    };
    // a copy that asked to stay out of clipboard history counts as private too
    let history = format("CanIncludeInClipboardHistory");
    if available(history) && bytes(history).is_some_and(|b| b.len() >= 4 && b[..4] == [0, 0, 0, 0]) {
        return None;
    }
    let wide: Vec<u16> = bytes(13)?.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).take_while(|&c| c != 0).collect(); // CF_UNICODETEXT
    Some(String::from_utf16_lossy(&wide)).filter(|t| !t.trim().is_empty())
}

/// Local time minus UTC, in seconds (IST: +19800), daylight saving included.
pub fn utc_offset_secs() -> i64 {
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    let mut tz = TIME_ZONE_INFORMATION::default();
    let id = unsafe { GetTimeZoneInformation(&mut tz) };
    let extra = if id == 2 { tz.DaylightBias } else { tz.StandardBias }; // 2 = TIME_ZONE_ID_DAYLIGHT
    -((tz.Bias + extra) as i64) * 60
}

/// Milliseconds since the user last touched the keyboard or mouse.
pub fn idle_ms() -> u64 {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    unsafe {
        if !GetLastInputInfo(&mut li).as_bool() {
            return 0;
        }
        windows::Win32::System::SystemInformation::GetTickCount().wrapping_sub(li.dwTime) as u64
    }
}

/// When capture should read next, in ms since `clock()`'s start; 0 = no read asked for.
static READ_AT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn clock() -> u64 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
}

/// A window came to the front, or the front window's title changed (another tab or chat): read 1 s from now. A newer
/// event pushes the read back, so a burst of switches costs one read.
unsafe extern "system" fn on_window_event(_: HWINEVENTHOOK, event: u32, hwnd: HWND, id_object: i32, _: i32, _: u32, _: u32) {
    if event == EVENT_OBJECT_NAMECHANGE && (id_object != OBJID_WINDOW.0 || hwnd != GetForegroundWindow()) {
        return;
    }
    READ_AT.store(clock() + 1000, Ordering::SeqCst);
}

const POLL_MS: u64 = 10_000;
const IDLE_MS: u64 = 60_000;

/// Is a read due? After a window event's delay, every `POLL_MS` while active, and at once when the user comes back
/// from idle. Never while idle.
fn read_due(now: u64, last_read: u64, read_at: u64, idle: bool, was_idle: bool) -> bool {
    !idle && (was_idle || (read_at != 0 && now >= read_at) || now.saturating_sub(last_read) >= POLL_MS)
}

/// Runs forever on the calling thread, calling `read` whenever capture should read the window in front (ticket 02).
pub fn capture_loop(mut read: impl FnMut()) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        for ev in [EVENT_SYSTEM_FOREGROUND, EVENT_OBJECT_NAMECHANGE] {
            SetWinEventHook(ev, ev, None, Some(on_window_event), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS);
        }
        let (mut last_read, mut was_idle, mut msg) = (0u64, false, MSG::default());
        loop {
            // out-of-context events arrive through this thread's message queue
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
            std::thread::sleep(Duration::from_millis(250));
            let (now, idle) = (clock(), idle_ms() >= IDLE_MS);
            if read_due(now, last_read, READ_AT.load(Ordering::SeqCst), idle, was_idle) {
                (last_read, was_idle) = (now, false);
                READ_AT.store(0, Ordering::SeqCst);
                read();
            } else if idle {
                was_idle = true;
            }
        }
    }
}

fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n])
}

fn process_name(hwnd: HWND) -> String {
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 1024];
        let mut n = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut n).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..n as usize]);
        path.rsplit('\\').next().unwrap_or_default().to_string()
    }
}

unsafe fn text_pattern(el: &IUIAutomationElement) -> Option<IUIAutomationTextPattern> {
    el.GetCurrentPatternAs(UIA_TextPatternId).ok()
}

unsafe fn caret_text_pattern2(el: &IUIAutomationElement) -> Option<Rect> {
    let p: IUIAutomationTextPattern2 = el.GetCurrentPatternAs(UIA_TextPattern2Id).ok()?;
    let mut active = BOOL::default();
    range_caret(&p.GetCaretRange(&mut active).ok()?)
}

unsafe fn caret_selection(el: &IUIAutomationElement) -> Option<Rect> {
    let sel = text_pattern(el)?.GetSelection().ok()?;
    if sel.Length().ok()? < 1 {
        return None;
    }
    range_caret(&sel.GetElement(0).ok()?)
}

/// Caret from a (usually zero-length) range. If it has no rectangle, use the right edge of the character before it.
unsafe fn range_caret(r: &IUIAutomationTextRange) -> Option<Rect> {
    if let Some(&b) = rects(r).first() {
        return Some(Rect { w: b.w.max(1), ..b });
    }
    let before = r.Clone().ok()?;
    before.MoveEndpointByUnit(TextPatternRangeEndpoint_Start, TextUnit_Character, -1).ok()?;
    let b = *rects(&before).last()?;
    Some(Rect { x: b.x + b.w, y: b.y, w: 1, h: b.h })
}

unsafe fn rects(r: &IUIAutomationTextRange) -> Vec<Rect> {
    let Ok(psa) = r.GetBoundingRectangles() else { return vec![] };
    let v = doubles(psa);
    let _ = SafeArrayDestroy(psa);
    v.chunks_exact(4).map(|q| Rect { x: q[0] as i32, y: q[1] as i32, w: q[2] as i32, h: q[3] as i32 }).collect()
}

unsafe fn doubles(psa: *mut SAFEARRAY) -> Vec<f64> {
    if psa.is_null() {
        return vec![];
    }
    let Ok(ub) = SafeArrayGetUBound(psa, 1) else { return vec![] };
    let mut p = std::ptr::null_mut();
    if SafeArrayAccessData(psa, &mut p).is_err() {
        return vec![];
    }
    let v = std::slice::from_raw_parts(p as *const f64, (ub + 1).max(0) as usize).to_vec();
    let _ = SafeArrayUnaccessData(psa);
    v
}

/// The classic Win32 system caret (Notepad and other plain Edit controls have no TextPattern).
fn caret_win32() -> Option<Rect> {
    unsafe {
        let tid = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let mut gi = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        GetGUIThreadInfo(tid, &mut gi).ok()?;
        if gi.hwndCaret.is_invalid() {
            return None;
        }
        let c = gi.rcCaret;
        let mut pt = POINT { x: c.left, y: c.top };
        let _ = ClientToScreen(gi.hwndCaret, &mut pt);
        Some(Rect { x: pt.x, y: pt.y, w: (c.right - c.left).max(1), h: c.bottom - c.top })
    }
}

fn to_rect(r: RECT) -> Rect {
    Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
}

// ---------------------------------------------------------------- write-back

/// Pastes `text` into the field captured at tap time: over the user's draft when `replace_draft` (kept text such as a
/// signature or quoted thread below it stays, see `split_kept`), otherwise over the selection or at the caret. Refuses if the field moved or changed since the tap (the user's draft is safe).
/// The text goes in a few words at a time so Gopu can walk with it: `progress` gets the field after each piece lands.
/// The user's clipboard is restored afterwards; Gopu's temporary copies stay out of clipboard history and cloud sync.
/// `owner`: one of our windows; the clipboard needs an owner for SetClipboardData to succeed.
// ponytail: each piece is its own undo step in most apps, so getting the draft back takes several Ctrl+Z; one paste
// would be one step but makes the text appear before Gopu moves
pub fn write_back(
    at_tap: &Field,
    replace_draft: bool,
    text: &str,
    owner: *mut core::ffi::c_void,
    mut progress: impl FnMut(&Field),
) -> Result<(), String> {
    use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
    let now = focused_field().ok_or("the field is gone")?;
    if now.rect != at_tap.rect || now.text != at_tap.text || now.selection != at_tap.selection {
        return Err("the field changed since the tap, so Gopu left it alone".into());
    }
    // with kept text below, the gap before it goes back in after the new text so the two don't run together
    let (draft, gap, kept) = crate::split_kept(&at_tap.text);
    let full = format!("{text}{gap}");
    let text = if replace_draft { full.as_str() } else { text };
    let owner = HWND(owner);
    let window = unsafe { GetForegroundWindow() };
    let saved = clip::open(owner)?.snapshot();
    let mut ours = 0;
    let pasted = (|| -> Result<(), String> {
        let mut written = String::new();
        for (i, piece) in crate::chunks(text, 3, 16).into_iter().enumerate() {
            if cancelled() {
                return Err(STOPPED.into());
            }
            if unsafe { GetForegroundWindow() } != window {
                return Err("you switched windows mid-write, so Gopu stopped (Ctrl+Z undoes it)".into());
            }
            clip::open(owner)?.set_text(piece)?;
            ours = unsafe { GetClipboardSequenceNumber() };
            if i == 0 && replace_draft {
                if !select_draft(draft, gap, kept) {
                    return Err("Gopu couldn't select just your text above the signature or thread, so left it alone".into());
                }
                std::thread::sleep(Duration::from_millis(60)); // editors apply a new selection asynchronously
            }
            send_ctrl_chord(b'V');
            written.push_str(piece);
            // Apps read the clipboard when they handle Ctrl+V, maybe later: wait for this piece to land before the
            // clipboard changes again, or pieces could swap or repeat.
            let t0 = std::time::Instant::now();
            while t0.elapsed() < Duration::from_millis(800) {
                std::thread::sleep(Duration::from_millis(25));
                if let Some(f) = focused_field().filter(|f| crate::landed(&f.text, &written, None)) {
                    progress(&f);
                    break;
                }
            }
        }
        Ok(())
    })();
    std::thread::sleep(Duration::from_millis(100));
    // if the sequence moved, the user copied something meanwhile: keep theirs rather than our snapshot
    if unsafe { GetClipboardSequenceNumber() } == ours {
        clip::open(owner)?.restore(&saved);
    }
    pasted?;
    let after = focused_field().map(|f| f.text).unwrap_or_default();
    let replaced = replace_draft.then_some(draft);
    if crate::landed(&after, text, replaced) {
        Ok(())
    } else if crate::landed(&after, text, None) {
        Err("the new text went in but the old text is still there (Ctrl+Z undoes it)".into())
    } else if after != at_tap.text {
        // the field changed, just not into exactly our text: an editor's own suggestions (Copilot), auto-format or a
        // view that only exposes the visible lines
        Err("Gopu wrote it but couldn't confirm all of it landed; check the field (Ctrl+Z undoes it)".into())
    } else {
        // the app ignored Ctrl+V: an elevated app blocks simulated keys, and some web editors drop a paste
        Err("the paste didn't land: the app ignored it, so nothing changed".into())
    }
}

/// Selects the focused field's draft + gap: all of its text when there's no kept text. Through UI Automation (exact,
/// and rich web editors like Discord's ignore a synthetic Ctrl+A); fields without a TextPattern (classic Win32 edits
/// like Notepad's) get Ctrl+A, or EM_SETSEL up to the kept text. False when the draft couldn't be selected exactly.
fn select_draft(draft: &str, gap: &str, kept: &str) -> bool {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(uia) = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) else { return false };
        let Ok(el) = uia.GetFocusedElement() else { return false };
        let Some(p) = text_pattern(&el) else {
            if kept.is_empty() {
                send_ctrl_chord(b'A');
                return true;
            }
            return win32_select(0, draft.encode_utf16().count() + gap.encode_utf16().count());
        };
        let Ok(doc) = p.DocumentRange() else { return false };
        if kept.is_empty() {
            return doc.Select().is_ok();
        }
        // end the range where kept text starts: find its first line, skipping copies of it inside the draft
        // (counting UIA characters instead breaks on editors that map paragraphs and embeds differently)
        let first = kept.lines().next().unwrap_or_default().trim();
        let skip = draft.matches(first).count();
        let Ok(rest) = doc.Clone() else { return false };
        for n in 0..=skip {
            let Ok(hit) = rest.FindText(&windows::core::BSTR::from(first), false, false) else { return false };
            if n == skip {
                return doc.MoveEndpointByRange(TextPatternRangeEndpoint_End, &hit, TextPatternRangeEndpoint_Start).is_ok()
                    && doc.Select().is_ok();
            }
            if rest.MoveEndpointByRange(TextPatternRangeEndpoint_Start, &hit, TextPatternRangeEndpoint_End).is_err() {
                return false;
            }
        }
        false
    }
}

/// Selects UTF-16 offsets `start..end` in the focused classic Win32 Edit/RichEdit control (EM_SETSEL).
fn win32_select(start: usize, end: usize) -> bool {
    const EM_SETSEL: u32 = 0x00B1;
    unsafe {
        let tid = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let mut gi = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        if GetGUIThreadInfo(tid, &mut gi).is_err() || gi.hwndFocus.is_invalid() {
            return false;
        }
        SendMessageTimeoutW(gi.hwndFocus, EM_SETSEL, WPARAM(start), LPARAM(end as isize), SMTO_ABORTIFHUNG, 200, None).0 != 0
    }
}

/// Ctrl + an ASCII letter via SendInput, with scan codes (some apps ignore keys without them).
/// Injected keys carry LLKHF_INJECTED, so the Gopu key hook ignores them.
fn send_ctrl_chord(letter: u8) {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: unsafe { MapVirtualKeyW(vk.0 as u32, MAPVK_VK_TO_VSC) } as u16,
                dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() },
                ..Default::default()
            },
        },
    };
    let k = VIRTUAL_KEY(letter as u16);
    let inputs = [key(VK_CONTROL, false), key(k, false), key(k, true), key(VK_CONTROL, true)];
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
}

mod clip {
    use windows::core::w;
    use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
    use windows::Win32::System::DataExchange::*;
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};

    /// An open clipboard; closed on drop.
    pub struct Open;

    pub fn open(owner: HWND) -> Result<Open, String> {
        for _ in 0..20 {
            if unsafe { OpenClipboard(Some(owner)) }.is_ok() {
                return Ok(Open);
            }
            std::thread::sleep(std::time::Duration::from_millis(15)); // another app has it open
        }
        Err("the clipboard is busy".into())
    }

    impl Drop for Open {
        fn drop(&mut self) {
            let _ = unsafe { CloseClipboard() };
        }
    }

    /// GDI-handle formats can't be copied as bytes; Windows re-synthesizes bitmaps from CF_DIB, which we do keep.
    fn is_gdi(f: u32) -> bool {
        matches!(f, 2 | 3 | 9 | 14 | 0x80 | 0x82 | 0x83 | 0x8E | 0x300..=0x3FF)
    }

    impl Open {
        pub fn snapshot(&self) -> Vec<(u32, Vec<u8>)> {
            let mut out = vec![];
            let mut f = 0;
            loop {
                f = unsafe { EnumClipboardFormats(f) };
                if f == 0 {
                    return out;
                }
                if is_gdi(f) {
                    continue;
                }
                if let Ok(h) = unsafe { GetClipboardData(f) } {
                    let g = HGLOBAL(h.0);
                    let p = unsafe { GlobalLock(g) };
                    if !p.is_null() {
                        let n = unsafe { GlobalSize(g) };
                        out.push((f, unsafe { std::slice::from_raw_parts(p as *const u8, n) }.to_vec()));
                        let _ = unsafe { GlobalUnlock(g) };
                    }
                }
            }
        }

        pub fn restore(&self, saved: &[(u32, Vec<u8>)]) {
            let _ = unsafe { EmptyClipboard() };
            for (f, bytes) in saved {
                let _ = put(*f, bytes);
            }
        }

        pub fn set_text(&self, text: &str) -> Result<(), String> {
            unsafe { EmptyClipboard() }.map_err(|e| format!("clipboard: {e}"))?;
            let wide: Vec<u8> = text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
            put(13, &wide)?; // CF_UNICODETEXT
            // keep Gopu's temporary copy out of clipboard managers, Win+V history and cloud sync
            let _ = put(unsafe { RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing")) }, &[0]);
            let _ = put(unsafe { RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory")) }, &0u32.to_le_bytes());
            let _ = put(unsafe { RegisterClipboardFormatW(w!("CanUploadToCloudClipboard")) }, &0u32.to_le_bytes());
            Ok(())
        }
    }

    fn put(f: u32, bytes: &[u8]) -> Result<(), String> {
        unsafe {
            let g = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).map_err(|e| format!("clipboard: {e}"))?;
            let p = GlobalLock(g);
            if p.is_null() {
                let _ = GlobalFree(Some(g));
                return Err("clipboard: out of memory".into());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
            let _ = GlobalUnlock(g);
            if SetClipboardData(f, Some(HANDLE(g.0))).is_err() {
                let _ = GlobalFree(Some(g)); // on success the clipboard owns the memory
                return Err("clipboard: couldn't set data".into());
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------- settings

/// Gopu's config folder: %APPDATA%\Gopu.
pub fn config_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("APPDATA").map(|d| std::path::PathBuf::from(d).join("Gopu"))
}

/// Gopu's log folder: %LOCALAPPDATA%\Gopu, so logs (they hold screen text) never roam with the profile.
pub fn log_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| std::path::PathBuf::from(d).join("Gopu"))
}

/// Opens a folder in File Explorer, or a web address in the default browser.
pub fn shell_open(target: impl AsRef<std::ffi::OsStr>) -> Result<(), String> {
    std::process::Command::new("explorer.exe").arg(target).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// A secret from Windows Credential Manager (a generic credential named `name`).
pub fn secret_get(name: &str) -> Option<String> {
    use windows::core::HSTRING;
    use windows::Win32::Security::Credentials::{CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC};
    unsafe {
        let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
        CredReadW(&HSTRING::from(name), CRED_TYPE_GENERIC, None, &mut cred).ok()?;
        let c = &*cred;
        let blob = std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec();
        CredFree(cred as *const _);
        // written as UTF-8 by secret_set; `cmdkey /pass:` writes UTF-16, so take that too
        let s = match String::from_utf8(blob.clone()) {
            Ok(s) if !s.contains('\0') => s,
            _ => String::from_utf16_lossy(&blob.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect::<Vec<_>>()),
        };
        Some(s.trim().to_string()).filter(|s| !s.is_empty())
    }
}

/// Saves `value` as the generic credential `name`, or deletes it with `None`.
pub fn secret_set(name: &str, value: Option<&str>) -> Result<(), String> {
    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::Security::Credentials::{CredDeleteW, CredWriteW, CREDENTIALW, CRED_FLAGS, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC};
    let target = HSTRING::from(name);
    unsafe {
        let Some(value) = value else {
            let _ = CredDeleteW(&target, CRED_TYPE_GENERIC, None); // already gone is fine
            return Ok(());
        };
        let mut user: Vec<u16> = "gopu\0".encode_utf16().collect();
        let mut blob = value.as_bytes().to_vec();
        let cred = CREDENTIALW {
            Flags: CRED_FLAGS(0),
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(target.as_ptr() as *mut _),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: PWSTR(user.as_mut_ptr()),
            ..Default::default()
        };
        CredWriteW(&cred, 0).map_err(|e| format!("couldn't save the key: {e}"))
    }
}

/// A saved user environment variable (HKCU\Environment), for processes started before it was set.
pub fn user_env(name: &str) -> Option<String> {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ};
    let mut buf = [0u16; 2048];
    let mut size = std::mem::size_of_val(&buf) as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from("Environment"),
            &HSTRING::from(name),
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    }
    .is_ok();
    let n = (size as usize / 2).saturating_sub(1); // bytes → u16s, minus the trailing NUL
    ok.then(|| String::from_utf16_lossy(&buf[..n])).filter(|s| !s.is_empty())
}

/// Adds or removes Gopu's Startup entry (HKCU Run, value "Gopu", the name the uninstaller removes).
pub fn set_autostart(on: bool) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
    let key = HSTRING::from(r"Software\Microsoft\Windows\CurrentVersion\Run");
    let name = HSTRING::from("Gopu");
    unsafe {
        if !on {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, &key, &name); // already gone is fine
            return Ok(());
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let cmd: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().chain([0]).collect();
        RegSetKeyValueW(HKEY_CURRENT_USER, &key, &name, REG_SZ.0, Some(cmd.as_ptr().cast()), (cmd.len() * 2) as u32)
            .ok()
            .map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------- the overlay

/// The primary monitor's work area (the screen minus the taskbar), in physical pixels.
pub fn work_area() -> Option<Rect> {
    let mut r = RECT::default();
    unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut r as *mut RECT as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) }.ok()?;
    Some(to_rect(r))
}

/// Puts `hwnd` in front with the keyboard. Windows only lets the app the user last used take the foreground, and a
/// key caught by our hook doesn't count, so borrow the foreground window's input queue for the switch.
pub fn bring_to_front(hwnd: *mut core::ffi::c_void) {
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    unsafe {
        let fg = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let me = GetCurrentThreadId();
        let attached = fg != 0 && fg != me && AttachThreadInput(me, fg, true).as_bool();
        let _ = ShowWindow(HWND(hwnd), SW_SHOW);
        let _ = SetForegroundWindow(HWND(hwnd));
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(HWND(hwnd)));
        if attached {
            let _ = AttachThreadInput(me, fg, false);
        }
    }
}

/// Shows `hwnd` without taking the keyboard from the window the user is typing in.
pub fn show_quietly(hwnd: *mut core::ffi::c_void) {
    unsafe {
        let _ = ShowWindow(HWND(hwnd), SW_SHOWNOACTIVATE);
    }
}

/// The left mouse button is held down right now (a drag is still going).
pub fn mouse_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000 != 0 }
}

/// Hides `hwnd`. Windows shown with `show_quietly` or `bring_to_front` must be hidden with this too: Tauri tracks
/// visibility itself, never saw them shown, and ignores its own hide.
pub fn hide(hwnd: *mut core::ffi::c_void) {
    unsafe {
        let _ = ShowWindow(HWND(hwnd), SW_HIDE);
    }
}

/// Puts `hwnd` back on top of other always-on-top windows (the taskbar rises over it when clicked), without focus.
pub fn raise(hwnd: *mut core::ffi::c_void) {
    unsafe {
        let _ = SetWindowPos(HWND(hwnd), Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// Is `hwnd` the window the user is in right now?
/// The program of the window in front ("Code.exe"), for the Gopu key's per-app off list.
pub fn foreground_app() -> String {
    process_name(unsafe { GetForegroundWindow() })
}

pub fn is_foreground(hwnd: *mut core::ffi::c_void) -> bool {
    unsafe { GetForegroundWindow() == HWND(hwnd) }
}

/// Keeps the overlay out of Alt+Tab; Tauri's config can't set WS_EX_TOOLWINDOW (spike 02).
pub fn hide_from_alt_tab(hwnd: *mut core::ffi::c_void) {
    let hwnd = HWND(hwnd);
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_TOOLWINDOW.0 as isize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const VK_RCTRL: u32 = 0xA3; // the default Gopu key
    fn k(vk: u32, down: bool, t: u32) -> Ev {
        Ev::Key { vk, down, t }
    }

    #[test]
    fn speakers_survive() {
        // a Discord DM: Rahul speaks, then the user, then Rahul again
        let mut w = Walk::default();
        for line in ["Rahul Mehta", "should I send the whole file?", "Alex Morgan", "check it once before you send", "Rahul Mehta", "ok checking now", "ok checking now"] {
            w.push(line);
        }
        assert_eq!(w.before.iter().filter(|l| *l == "Rahul Mehta").count(), 2); // his second turn keeps his name
        assert_eq!(w.before.last().unwrap(), "ok checking now");
        assert_eq!(w.before.len(), 6); // a line repeated right after itself still goes
    }

    #[test]
    fn capture_schedule() {
        assert!(!read_due(5_000, 0, 0, false, false)); // nothing new, poll not due yet
        assert!(read_due(10_000, 0, 0, false, false)); // the 10 s poll
        assert!(read_due(5_000, 4_000, 4_500, false, false)); // a window switch, its 1 s delay passed
        assert!(!read_due(4_400, 4_000, 4_500, false, false)); // ... not before
        assert!(!read_due(99_000, 0, 50_000, true, false)); // never while idle
        assert!(read_due(99_000, 98_000, 0, false, true)); // back from idle: at once
    }

    #[test]
    fn secret_round_trip() {
        let name = "Gopu:test-only";
        secret_set(name, Some("sk-test 123")).unwrap();
        assert_eq!(secret_get(name).as_deref(), Some("sk-test 123"));
        secret_set(name, None).unwrap();
        assert_eq!(secret_get(name), None);
    }

    #[test]
    fn lone_tap_only() {
        let mut s = Tap::default();
        assert!(!s.feed(k(VK_RCTRL, true, 0)));
        assert!(s.feed(k(VK_RCTRL, false, 100))); // lone tap

        s.feed(k(VK_RCTRL, true, 1000));
        s.feed(k(0x43, true, 1050)); // Ctrl+C
        assert!(!s.feed(k(VK_RCTRL, false, 1100)));

        s.feed(k(VK_RCTRL, true, 2000));
        s.feed(k(VK_RCTRL, true, 2300)); // auto-repeat
        assert!(!s.feed(k(VK_RCTRL, false, 2600))); // held too long

        s.feed(k(VK_RCTRL, true, 3000));
        s.feed(Ev::Mouse); // Ctrl+click / Ctrl+wheel
        assert!(!s.feed(k(VK_RCTRL, false, 3100)));

        assert!(!s.feed(k(VK_RCTRL, false, 4000))); // stray key-up

        s.feed(k(VK_RCTRL, true, u32::MAX - 50)); // tick count wraps
        assert!(s.feed(k(VK_RCTRL, false, 49)));

        s.feed(k(0x41, true, 5000)); // a key before Right Ctrl doesn't spoil the next tap
        s.feed(k(VK_RCTRL, true, 5100));
        assert!(s.feed(k(VK_RCTRL, false, 5200)));
    }
}
