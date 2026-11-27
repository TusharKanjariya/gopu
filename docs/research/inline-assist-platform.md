# Inline assist + screen memory: platform research

Researched 2026-11-19. Every claim links to a primary source (vendor docs, specs, or the project's own repo/code). Anything I couldn't confirm from a primary page is marked **unverified**. Quotes and figures reflect the pages as fetched on that date. Rate limits, licences and default ports do change.

---

## Summary of recommendations

1. **Read the focused field with UIA.** Call `IUIAutomation::GetFocusedElement`, check `IsPassword` first, then read `TextPattern.DocumentRange`/`GetSelection` (`TextPattern2.GetCaretRange` for the caret). Fall back to `ValuePattern.CurrentValue`. [UIA props](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-automation-element-propids), [TextPattern](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nn-uiautomationclient-iuiautomationtextpattern), [GetCaretRange](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationtextpattern2-getcaretrange)
2. **Walk surrounding context in one cross-process call.** Use a `CacheRequest` (`TreeScope_Subtree`, ControlView filter, `AutomationElementMode_None`). [Caching](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-cachingforclients)
3. **Expect Chromium/Electron trees to be empty until your UIA/MSAA client asks.** Chromium enables accessibility on demand when it receives `WM_GETOBJECT` (`UiaRootObjectId` / `OBJID_CLIENT`). Re-query after the first touch. [legacy_render_widget_host_win.cc](https://chromium.googlesource.com/chromium/src/+/HEAD/content/browser/renderer_host/legacy_render_widget_host_win.cc), [a11y overview](https://chromium.googlesource.com/chromium/src/+/HEAD/docs/accessibility/overview.md)
4. **Write back with a simulated paste by default.** Use `ValuePattern.SetValue` only for plain single-value edits. `SetValue` replaces the whole value. Paste via `SendInput` keeps undo but is subject to UIPI and gets no failure signal. [SetValue](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationvaluepattern-setvalue), [SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)
5. **Hotkeys: use `RegisterHotKey` for chords, and a `WH_KEYBOARD_LL` hook on a dedicated thread only for lone-modifier taps.** Keep the hook callback well under 1 s, or Windows silently removes it. Skip events that have `LLKHF_INJECTED` set. [LowLevelKeyboardProc](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc), [KBDLLHOOKSTRUCT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-kbdllhookstruct)
6. **Keep temporary clipboard writes out of history and sync.** Tag them with `ExcludeClipboardContentFromMonitorProcessing`. Snapshot the clipboard, write, paste, then restore. Use `GetClipboardSequenceNumber` to detect races. [Clipboard formats](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats), [GetClipboardSequenceNumber](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getclipboardsequencenumber)
7. **macOS: use AXUIElement plus `AXIsProcessTrustedWithOptions`.** Block `kAXSecureTextFieldSubrole` fields, and treat `IsSecureEventInputEnabled()==true` as "do nothing". [AXIsProcessTrustedWithOptions](https://developer.apple.com/documentation/applicationservices/1459186-axisprocesstrustedwithoptions), [TN2150](https://developer.apple.com/library/archive/technotes/tn2150/_index.html)
8. **Linux: use AT-SPI2 for text, and on Wayland use the XDG GlobalShortcuts portal for hotkeys and the RemoteDesktop portal (or libei) for input injection.** Both portals require user approval. [GlobalShortcuts](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html), [RemoteDesktop](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html)
9. **Tauri v2 is a sensible shell.** It uses the system webview (WebView2 on Windows) and ships no bundled runtime. It has official global-shortcut, tray, autostart, sidecar and signed-updater plugins. [Architecture](https://v2.tauri.app/concept/architecture/), [Webview versions](https://v2.tauri.app/reference/webview-versions/)
10. **Storage: SQLite FTS5 external-content table with sync triggers, ranked by `bm25()` with `snippet()`, plus `sqlite-vec` `vec0` for vectors.** sqlite-vec is pre-v1, so pin the version. [FTS5](https://www.sqlite.org/fts5.html), [sqlite-vec](https://github.com/asg017/sqlite-vec)
11. **Embeddings: use fastembed-rs (ort + HF tokenizers) with `bge-small-en-v1.5`.** It is 384-dim, MIT licensed, and the fastembed default. [fastembed-rs](https://github.com/Anush008/fastembed-rs), [bge-small model card](https://huggingface.co/BAAI/bge-small-en-v1.5)
12. **MCP: expose stdio (simplest for local agents) and optionally Streamable HTTP on 127.0.0.1.** HTTP needs an Origin check and auth. The current spec revision (2026-07-28) has removed sessions and the GET stream. [Transports](https://modelcontextprotocol.io/specification/latest/basic/transports), [Streamable HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http.md)
13. **LLM: write one OpenAI-compatible client with a configurable `base_url`.** Groq is `https://api.groq.com/openai/v1`, Ollama is `http://localhost:11434/v1/`, and llama-server defaults to `127.0.0.1:9931`. [Groq](https://console.groq.com/docs/openai), [Ollama](https://docs.ollama.com/api/openai-compatibility), [llama.cpp server](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md)
14. **Prior art: screenpipe is no longer OSI open source** (Screenpipe Commercial License). Do not copy its code. OpenRecall is AGPL-3.0, Windrecorder GPL-2.0, Pensieve Apache-2.0, rem MIT, KeyType MIT, Cotabby AGPL-3.0, Writing Tools GPL-3.0. [screenpipe LICENSE.md](https://github.com/screenpipe/screenpipe/blob/main/LICENSE.md)
15. **Privacy defaults:** exclude password-manager processes, detect private-browsing windows by localized title strings, skip `IsPassword` elements, and run regex PII redaction before writing to the index. screenpipe does all four; code paths are in §8. [screenpipe tree/windows.rs](https://github.com/screenpipe/screenpipe/blob/main/crates/screenpipe-a11y/src/tree/windows.rs)

---

## 1. Windows UI Automation

### Focused element and its text
- **Focused element.** `IUIAutomation::GetFocusedElement` returns the element with keyboard focus ([doc](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation-getfocusedelement)). To follow focus without polling, subscribe with `AddFocusChangedEventHandler` ([doc](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation-addfocuschangedeventhandler)). `UIA_HasKeyboardFocusPropertyId` (30008) and `UIA_ProcessIdPropertyId` (30002) identify the element and its owning process ([property IDs](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-automation-element-propids)).
- **TextPattern** is the rich path ([IUIAutomationTextPattern](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nn-uiautomationclient-iuiautomationtextpattern)):
  - `DocumentRange` returns "a text range that encloses the main text of a document".
  - `GetSelection` returns the currently selected ranges.
  - `GetVisibleRanges` returns visible spans, which is good for limiting what you capture.
  - `RangeFromPoint` returns the range nearest a screen point.
- **Caret.** `IUIAutomationTextPattern2::GetCaretRange` (Windows 8+) returns a zero-length range at the caret plus an `isActive` flag. The doc warns: "If the *isActive* parameter is FALSE, the caret … might not be at the same location as the system caret" ([doc](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationtextpattern2-getcaretrange)).
- **ValuePattern** is the simple path. `CurrentValue` is a single string for the control ([IUIAutomationValuePattern](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nn-uiautomationclient-iuiautomationvaluepattern)). Use it when TextPattern is absent, for example single-line edits and many web `<input>` elements.
- **Password fields.** The doc for `UIA_IsPasswordPropertyId` (30019) says: "When the IsPassword property is TRUE and the element has the keyboard focus, a client application should disable keyboard echoing … Attempting to access the Value property of the protected element (edit control) may cause an error to occur" ([property IDs](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-automation-element-propids)). Check it **before** reading any text and abort if it is true.

### Walking a window's tree efficiently
- **Views.** The raw view mirrors the native structure. The control view contains only `IsControlElement=TRUE` items. The content view adds `IsContentElement=TRUE` and is described as "most interesting to a screen reader". Each view has a walker: `RawViewWalker`, `ControlViewWalker` and `ContentViewWalker` ([Tree overview](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-treeoverview)).
- **Caching (bulk fetch).** Fetching properties one element at a time "is slow and inefficient because it requires a cross-process call for each property". A `CacheRequest` instead fetches all requested properties for all scoped elements in one call. The key settings ([Caching](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-cachingforclients)):
  - `AddProperty` / `AddPattern` choose what to fetch. Caching a pattern does not cache its properties.
  - `TreeScope` must include `TreeScope_Element` to cache the root itself. Parent and Ancestors scopes are not allowed.
  - `TreeFilter` defaults to the control view.
  - `AutomationElementMode_None` gives lightweight cache-only references, and you then can't call pattern methods on them.
  - Refresh with `BuildUpdatedCache` or by passing a cache request to the event subscription.
- **Precedent in prior art.** screenpipe's Windows walker uses exactly this approach. It adds `UIA_IsPasswordPropertyId` to the cache request, uses `ControlViewCondition` and `AutomationElementMode_None`, and walks with `ControlViewWalker` ([windows_uia.rs ~L596-617](https://github.com/screenpipe/screenpipe/blob/main/crates/screenpipe-a11y/src/platform/windows_uia.rs)). It also enforces a wall-clock budget per walk ([tree/windows.rs ~L441-503](https://github.com/screenpipe/screenpipe/blob/main/crates/screenpipe-a11y/src/tree/windows.rs)). The source is source-available, not open source, so treat it as a reference and don't copy it (see §7).

### Chromium / Electron apps (Chrome, Edge, Slack, VS Code, WhatsApp desktop)
- **Off by default.** Chromium's overview says: "Accessibility features in Chrome are off by default and enabled automatically on-demand" ([overview.md](https://chromium.googlesource.com/chromium/src/+/HEAD/docs/accessibility/overview.md)).
- **How activation happens on Windows.** The `WM_GETOBJECT` handler in `legacy_render_widget_host_win.cc` ([source](https://chromium.googlesource.com/chromium/src/+/HEAD/content/browser/renderer_host/legacy_render_widget_host_win.cc)) does three things:
  - `UiaRootObjectId`: calls `AXPlatform::SetUiaRequested()`. If the native UIA provider is enabled, it returns the UIA provider. Otherwise the comment says the client "will most likely try again for OBJID_CLIENT".
  - `OBJID_CLIENT`: calls `SetMsaaRequested()` and returns the IAccessible root.
  - A "screen reader honeypot" fake event detects clients that want full support. If an MSAA client responds to it, "only basic accessibility support is enabled" at first.
- **What this means for us.** The first UIA query into a Chromium window flips accessibility on, so the first tree you get can be sparse. Re-query after a short delay. screenpipe's code has the comment "A cold Chromium profile may first expose only browser chrome" ([tree/windows.rs ~L572](https://github.com/screenpipe/screenpipe/blob/main/crates/screenpipe-a11y/src/tree/windows.rs)).
- **Mode can drop again.** In `browser_accessibility_state_impl.cc`, accessibility can be auto-disabled after a WebContents has been hidden for about 5 minutes. This applies only when the ProgressiveAccessibilityPhase2 feature is on and no screen reader is detected ([source](https://chromium.googlesource.com/chromium/src/+/HEAD/content/browser/accessibility/browser_accessibility_state_impl.cc)).
- **Flags** ([overview.md](https://chromium.googlesource.com/chromium/src/+/HEAD/docs/accessibility/overview.md), [state impl](https://chromium.googlesource.com/chromium/src/+/HEAD/content/browser/accessibility/browser_accessibility_state_impl.cc)):
  - `--force-renderer-accessibility[=basic|form-controls|complete]` forces accessibility on. The source also lists an `on-screen` bundle.
  - `--disable-renderer-accessibility` turns it off.
  - `--disable-platform-accessibility-integration` blocks platform activation.
  - `--enable-features=UiaProvider` enables the native UIA provider, per [uiautomation.md](https://chromium.googlesource.com/chromium/src/+/HEAD/docs/accessibility/browser/uiautomation.md). Whether it is now on by default in stable Chrome/Edge is **unverified**: that doc reads as dated, while current code gates on `IsUiaProviderEnabled()`.
- **Electron.**
  - `app.setAccessibilitySupportEnabled(enabled)` (macOS and Windows) "Manually enables Chromium's accessibility support". The docs warn that "Rendering accessibility tree can significantly affect the performance of your app". `isAccessibilitySupportEnabled()` returns true when "assistive technologies, such as screen readers, has been detected" ([app API](https://www.electronjs.org/docs/latest/api/app)).
  - On macOS, third-party AT can set `AXManualAccessibility` on an Electron app to turn accessibility on ([Electron accessibility](https://www.electronjs.org/docs/latest/tutorial/accessibility)).
  - Whether specific apps (Slack, VS Code, WhatsApp) honour `--force-renderer-accessibility` on their command line is **unverified** per app.

### Writing text back
- **`ValuePattern.SetValue`** requires `CurrentIsEnabled == TRUE` and `CurrentIsReadOnly == FALSE` ([SetValue](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationvaluepattern-setvalue)).
  - It replaces the whole value, with no notion of selection or caret (that follows from the API shape).
  - The following practical limits are **unverified** against a primary source: whether the app's undo stack and JS input events fire on SetValue varies by framework, and rich editors (contenteditable, Word, Slack composer) often don't expose a settable ValuePattern.
- **Simulated paste via `SendInput`** (Ctrl+V after placing text on the clipboard) ([SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)):
  - "This function is subject to UIPI. Applications are permitted to inject input only into applications that are at an equal or lesser integrity level." We can't paste into elevated windows unless we also run elevated.
  - "neither GetLastError nor the return value will indicate the failure was caused by UIPI blocking."
  - "This function does not reset the keyboard's current state". Release the user's still-held hotkey modifiers first, or Ctrl+V can become Ctrl+Alt+V. Check key state with `GetAsyncKeyState`.
  - Injected events carry `LLKHF_INJECTED`, which lets our own hook ignore them ([KBDLLHOOKSTRUCT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-kbdllhookstruct)).
- **Recommended order:**
  1. TextPattern selection replace via paste. This works in nearly every app and keeps native undo.
  2. `SetValue` for simple, non-rich edits when paste fails.
  3. Typing with `SendInput` `KEYEVENTF_UNICODE` as a last resort (slow, but bypasses the clipboard).

## 2. Global hotkeys and clipboard on Windows

### RegisterHotKey vs WH_KEYBOARD_LL
- **`RegisterHotKey`** ([doc](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerhotkey)):
  - It posts `WM_HOTKEY` to a window or thread queue. `MOD_NOREPEAT` suppresses auto-repeat.
  - It "Typically … fails if the keystrokes … have already been registered for another hot key".
  - F12 is reserved for the debugger. `MOD_WIN` combinations are "reserved for use by the operating system".
  - It requires a `vk` plus modifiers. A lone-modifier hotkey is not a documented use; that it can't be done this way is **unverified** as an explicit statement, but it follows from the signature.
- **`WH_KEYBOARD_LL`** ([LowLevelKeyboardProc](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc)):
  - The hook "is called in the context of the thread that installed it … the thread that installed the hook must have a message loop."
  - Timeout: the callback must finish within the `LowLevelHooksTimeout` value (in ms) under `HKCU\Control Panel\Desktop`. "on Windows 7 and later, the hook is silently removed without being called. There is no way for the application to know whether the hook is removed."
  - On Windows 10 1709 and later, the maximum timeout the system allows is 1000 ms.
  - Microsoft's advice is to "run the hooks on a dedicated thread that passes the work off to a worker thread and then immediately returns", and to consider Raw Input for passive monitoring.
  - Returning nonzero swallows the key. Otherwise call `CallNextHookEx`.
  - `GetAsyncKeyState` is not updated yet inside the callback, so track modifier state yourself.
- **`LLKHF_INJECTED`** (0x10) marks injected events, and `LLKHF_LOWER_IL_INJECTED` (0x02) marks events injected from a lower integrity level ([KBDLLHOOKSTRUCT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-kbdllhookstruct)). Ignore injected events so our own paste can't trigger the hotkey.
- **Lone modifier tap.** This is our design and comes from no primary source. In the LL hook:
  1. On the modifier's key-down (no other keys down), record the timestamp.
  2. If any other key goes down before release, cancel.
  3. On key-up within about 300 ms with no intervening key, fire. Post to a worker; never do UIA work in the hook.
  - Avoid lone Alt (it activates the menu bar) and lone Win (it opens Start). This is common Windows behaviour but **unverified** against a primary doc here.
- **Tauri's global-shortcut plugin** takes combos like `CommandOrControl+Shift+C` built from `Modifiers` + `Code` ([plugin docs](https://v2.tauri.app/plugin/global-shortcut/)). Its docs say nothing about lone-modifier support, so plan on a custom LL hook for that one feature.

### Clipboard
- **`GetClipboardSequenceNumber`.** "This number is incremented whenever the contents of the clipboard change or the clipboard is emptied". With delayed rendering it is "not incremented until the changes are rendered" ([doc](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getclipboardsequencenumber)). Snapshot it before and after our write so we can tell whether the user copied something during our operation; if they did, don't restore over it.
- **History and cloud-sync exclusion.** These are registered format names, obtained via `RegisterClipboardFormat` ([Clipboard formats › Cloud Clipboard and Clipboard History Formats](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats)):
  - `ExcludeClipboardContentFromMonitorProcessing`: "Place any data on the clipboard in this format to prevent all clipboard formats being included in the clipboard history or synchronized to the user's other devices."
  - `CanIncludeInClipboardHistory`: a DWORD `0` excludes the item from local history and does not affect sync.
  - `CanUploadToCloudClipboard`: a DWORD `0` blocks sync and does not affect local history.
- **Recommendation.** For every temporary write, set `ExcludeClipboardContentFromMonitorProcessing` plus `CanIncludeInClipboardHistory=0` and `CanUploadToCloudClipboard=0` (belt and braces). Then restore the user's prior clipboard formats.

## 3. macOS and Linux

### macOS (AXUIElement)
- **Permission.** `AXIsProcessTrustedWithOptions` "Returns whether the current process is a trusted accessibility client". The option `kAXTrustedCheckOptionPrompt` makes the system inform the user; "Prompting occurs asynchronously and does not affect the return value" ([doc](https://developer.apple.com/documentation/applicationservices/1459186-axisprocesstrustedwithoptions)).
- **Reading the focused field.** Use `AXUIElementCreateSystemWide` → `kAXFocusedUIElementAttribute` ([doc](https://developer.apple.com/documentation/applicationservices/kaxfocuseduielementattribute)) → `kAXValueAttribute` / `kAXSelectedTextAttribute`. The selected-text attribute is "required for all accessibility objects that represent editable text elements" ([doc](https://developer.apple.com/documentation/applicationservices/kaxselectedtextattribute)).
- **Password fields.** `kAXSecureTextFieldSubrole` is "A text field intended to contain sensitive data" ([doc](https://developer.apple.com/documentation/applicationservices/kaxsecuretextfieldsubrole)).
- **Secure input** ([TN2150](https://developer.apple.com/library/archive/technotes/tn2150/_index.html)):
  - When any process enables `EnableSecureEventInput`, the system will "stop passing keyboard events to any intercept process … whether that process is in the foreground or background."
  - Check this with `IsSecureEventInputEnabled`. Treat it as "a password/secure context is active; don't read, don't inject."
- **Hotkeys via `CGEventTap`** ([CGEvent.tapCreate](https://developer.apple.com/documentation/coregraphics/cgevent/tapcreate(tap:place:options:eventsofinterest:callback:userinfo:))):
  - Taps receive key events if the process runs as root or "Access for assistive devices is enabled". That text is historical (OS X 10.4).
  - HID-level taps need root. If no requested events are permitted, `tapCreate` returns NULL.
  - macOS 10.15+ adds `CGPreflightListenEventAccess()` (Input Monitoring) ([doc](https://developer.apple.com/documentation/coregraphics/cgpreflightlisteneventaccess())). Exactly which TCC permission a listen-only tap needs versus an active tap is **unverified** from Apple's page text.
  - Tap auto-disable on timeout (`kCGEventTapDisabledByTimeout`) is **unverified** here.

### Linux
- **AT-SPI2.** at-spi2-core contains "the DBus interface definitions for AT-SPI" and "the basic daemons of the accessibility stack" ([GNOME/at-spi2-core](https://github.com/GNOME/at-spi2-core)). Relevant libatspi APIs ([docs.gtk.org/atspi2](https://docs.gtk.org/atspi2/)):
  - Interfaces: `Accessible`, `Text` (text, caret offset, selections), `EditableText` (set/insert text) and `EventListener`.
  - Enums: `StateType` (FOCUSED, EDITABLE) and `Role` (PASSWORD_TEXT).
  - That AT-SPI works the same under Wayland, since it is D-Bus not X11, is an inference and **unverified** from a primary page.
- **Wayland hotkeys.** The `org.freedesktop.portal.GlobalShortcuts` portal registers shortcuts "activated regardless of the focused state of the application window". It works through a session plus `BindShortcuts`, and the user may see a configuration dialog. It emits `Activated`/`Deactivated` signals and is at interface version 2 ([portal doc](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html)). Lone-modifier taps are probably not expressible through it (**unverified**).
- **Wayland input injection.** Use the `org.freedesktop.portal.RemoteDesktop` portal ([portal doc](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html)):
  - `Start()` "presents a dialog to the user", and `NotifyKeyboardKeysym` requires granted KEYBOARD access.
  - `ConnectToEIS()` (libei) is the recommended route.
  - `persist_mode=2` with a single-use `restore_token` avoids re-prompting.
- **X11.** That X11 allows global grabs and XTest injection without prompts is common knowledge, but **unverified** in this doc.

## 4. Tauri v2

| Need | Official doc | Notes |
|---|---|---|
| Global hotkey | [global-shortcut plugin](https://v2.tauri.app/plugin/global-shortcut/) | Crate `tauri-plugin-global-shortcut`, JS `@tauri-apps/plugin-global-shortcut`. "No features are enabled by default". You must grant `global-shortcut:allow-register` etc. in capabilities. Needs Rust ≥ 1.90. |
| Tray | [System tray](https://v2.tauri.app/learn/system-tray/) | `tray-icon` Cargo feature and `TrayIconBuilder`. Tray events aren't supported on Linux. |
| Autostart | [autostart plugin](https://v2.tauri.app/plugin/autostart/) | `enable/disable/isEnabled`, `MacosLauncher::LaunchAgent`. Needs `autostart:allow-*` permissions. |
| Sidecar | [Sidecar](https://v2.tauri.app/develop/sidecar/) | `bundle.externalBin`, binaries suffixed `-$TARGET_TRIPLE`, called via shell plugin `sidecar()`. Needs `shell:allow-execute` with `"sidecar": true`, and args are allow-listed. |
| Updater | [updater plugin](https://v2.tauri.app/plugin/updater/) | Signing is mandatory and can't be disabled. `pubkey` holds the key content, not a path. TLS is enforced in production. Static JSON (e.g. GitHub Releases) or a dynamic server. Set `createUpdaterArtifacts: true`. |

**Is Tauri sensible?** Yes, judged on the official architecture docs:
- Tauri apps "use the OS's webview" and "do not ship a runtime since the final binary is compiled from Rust" ([architecture](https://v2.tauri.app/concept/architecture/)).
- "a minimal Tauri app can be less than 600KB in size" ([start](https://v2.tauri.app/start/)).
- The webviews are WebView2 (Chromium-based) on Windows, WKWebView on macOS and webkit2gtk on Linux ([webview versions](https://v2.tauri.app/reference/webview-versions/)).
- Electron works "By embedding Chromium and Node.js into its binary" ([Electron docs](https://www.electronjs.org/docs/latest/)).
- The core of this product (UIA/AX hooks, SQLite, embeddings, MCP server) is native Rust either way. Tauri avoids a Node bridge for that work.
- Caveat: Linux webkit2gtk rendering differs from Chromium. That's a UI-only concern.

## 5. Local storage and retrieval

### SQLite FTS5 ([sqlite.org/fts5.html](https://www.sqlite.org/fts5.html))
- **Ranking.** `bm25()`: "The better the match, the numerically smaller the value returned". Use `ORDER BY bm25(t, w1, w2…)` for column weights. `ORDER BY rank` is equivalent and can be faster (§5.1.1, §5.2).
- **Excerpts.** `snippet(t, col, before, after, ellipsis, max_tokens≤64)` and `highlight(t, col, before, after)` (§5.1.2–5.1.3).
- **External-content tables.** Create with `content='t1', content_rowid='id'` and keep in sync with AFTER INSERT/DELETE/UPDATE triggers that use the special `'delete'` command (§4.4.3). The values passed to `'delete'` must match what's stored, or "results may be unpredictable". Use `'rebuild'` to recover (§4.4.4, §6.3). Store text once in a normal table and index it with FTS5.
- **Contentless tables.** `content=''` is the smallest option but returns only the rowid. Since 3.43, `contentless_delete=1` adds DELETE/UPDATE support (§4.4.1–4.4.2).
- **Tokenizers.** Use `unicode61 remove_diacritics 2` for accent-folding. Use `trigram` for substring and CJK-ish matching (no match for substrings under 3 chars) (§4.3.1, §4.3.4).

### Vectors: sqlite-vec ([asg017/sqlite-vec](https://github.com/asg017/sqlite-vec))
- An "extremely small, 'fast enough' vector search SQLite extension", written in pure C with no dependencies.
- Licensed MIT/Apache-2.0. It is a Mozilla Builders project.
- `vec0` virtual tables store float, int8 and binary vectors, queried with `WHERE emb MATCH ? ORDER BY distance LIMIT k`.
- It is a Rust crate (`cargo add sqlite-vec`).
- **"pre-v1, so expect breaking changes"**, so pin the version.
- Whether it does brute-force or ANN indexing isn't stated in the README (**unverified**). At personal-memory scale (≤ a few million chunks) brute force with int8/binary quantization is likely adequate (**unverified**, benchmark it).

### Embeddings
- **fastembed-rs** ([repo](https://github.com/Anush008/fastembed-rs)) is Apache-2.0.
  - It "Uses @pykeio/ort for performant ONNX inference" ([ort](https://github.com/pykeio/ort)) and "@huggingface/tokenizers" ([tokenizers](https://github.com/huggingface/tokenizers)).
  - Its default model is `BAAI/bge-small-en-v1.5`. It also ships all-MiniLM-L6-v2, multilingual-e5-small/base/large, paraphrase-multilingual-MiniLM-L12-v2, bge-m3 and others, plus rerankers (bge-reranker-base).
- **bge-small-en-v1.5** ([card](https://huggingface.co/BAAI/bge-small-en-v1.5)): MIT, "can be used for commercial purposes free of charge". 384-dim, 512 max tokens, about 33.4M params. The optional query prefix is "Represent this sentence for searching relevant passages:".
- **all-MiniLM-L6-v2** ([card](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2)): Apache-2.0, 384-dim. "input text longer than 256 word pieces is truncated".
- **Recommendation.** Use bge-small-en-v1.5 by default and multilingual-e5-small when the user's text is non-English. Pick the multilingual model card before shipping, since its licence and dims are **unverified** here.

### MCP transports ([spec, latest = 2026-07-28](https://modelcontextprotocol.io/specification/latest/basic/transports))
- **Two standard transports.** **stdio** is "newline-delimited messages over the standard streams of a client-launched subprocess". **Streamable HTTP** sends "each message [as] an HTTP POST to a single MCP endpoint; replies arrive as a JSON object or a request-scoped SSE stream".
- **Streamable HTTP security** ([doc](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http.md)):
  - Servers **MUST** validate `Origin` (respond 403 if invalid) to prevent DNS rebinding.
  - They **SHOULD** bind to 127.0.0.1 when local.
  - They **SHOULD** authenticate all connections.
- **Changes in 2026-07-28.** It removed protocol sessions (`Mcp-Session-Id`), the GET stream and resumability. It added required `MCP-Protocol-Version`, `Mcp-Method` and `Mcp-Name` headers, and per-request `_meta` metadata. HTTP+SSE (2024-11-05) is deprecated. Clients on 2025-03-26…2025-11-25 use the older session model, so plan the backward-compat fallback described on that page.

## 6. LLM providers

- **One-client convention.** The official OpenAI Python SDK accepts `base_url=` or `OPENAI_BASE_URL` ([openai-python README](https://github.com/openai/openai-python)). Point it at any compatible server.
- **Groq**
  - **Endpoint.** `base_url = "https://api.groq.com/openai/v1"`. These fields return 400: `logprobs`, `logit_bias`, `top_logprobs`, `messages[].name`, and `n` other than 1. `temperature=0` becomes 1e-8 ([OpenAI compat](https://console.groq.com/docs/openai)).
  - **Rate limits** ([rate-limits](https://console.groq.com/docs/rate-limits)): the table rendered on the page (the page has Free and Developer plan tabs; that this table is the Free tab is **unverified**) shows `openai/gpt-oss-20b` and `openai/gpt-oss-120b` at 30 RPM / 1K RPD / 8K TPM / 200K TPD.
    - Limits are per organization, and cached tokens don't count.
    - A 429 comes with `retry-after`. `x-ratelimit-*-requests` refers to RPD and `x-ratelimit-*-tokens` to TPM.
    - The llama-3.x models weren't in the rendered table (**unverified** whether they're still offered).
  - **Reasoning** ([reasoning](https://console.groq.com/docs/reasoning)):
    - `reasoning_effort` takes `low|medium|high` for GPT-OSS 20B/120B. Qwen 3.8 27B also accepts `none|default`.
    - `include_reasoning` (default true) applies to GPT-OSS. `reasoning_format` is not supported on GPT-OSS.
    - For inline rewrites, use `reasoning_effort="low"` and `include_reasoning=false`.
- **Ollama.** The native API is `POST http://localhost:11434/api/chat` with `model`, `messages`, `stream` (defaults to streaming), `think`, `format` (json or a JSON schema), `options` and `keep_alive` ([api/chat](https://docs.ollama.com/api/chat)). The OpenAI-compatible base is `http://localhost:11434/v1/`, with `/v1/chat/completions`, `/v1/embeddings` and others. "The client requires an API key value, but Ollama ignores it" ([OpenAI compat](https://docs.ollama.com/api/openai-compatibility)).
- **llama.cpp `llama-server`.** It offers "OpenAI API compatible chat completions, responses, and embeddings routes". The default listen address is **`127.0.0.1:9931`**: the current README says "port to listen (default: 9931)", and Docker examples still map 8080. `--api-key` accepts a comma-separated list. `--embeddings` restricts the server to embedding use ([server README](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md)). Make the port configurable; don't hard-code 8080.

## 7. Open-source prior art

| Project | Licence (GitHub SPDX / LICENSE) | Stack | Capture | Inline writing? |
|---|---|---|---|---|
| [screenpipe](https://github.com/screenpipe/screenpipe) | **Screenpipe Commercial License** (source-available): free for personal, non-commercial, nonprofit/education/research use and 7-day evaluation; commercial use needs a licence ([LICENSE.md](https://github.com/screenpipe/screenpipe/blob/main/LICENSE.md)). Earlier MIT history is **unverified**. | Rust crates + Tauri app; SQLite FTS5; MCP server; REST on localhost:3030 ([README](https://github.com/screenpipe/screenpipe/blob/main/README.md)) | Event-driven screenshot + **accessibility tree first, OCR fallback** (Apple Vision / Windows OCR / Tesseract) | Not described in README. Focus is memory, workflows, and agents via MCP |
| [OpenRecall](https://github.com/openrecall/openrecall) | AGPL-3.0 | Python 3.11; `mss` screenshots; docTR OCR; sentence-transformers `all-MiniLM-L6-v2`; sqlite3 ([openrecall/*.py](https://github.com/openrecall/openrecall/tree/main/openrecall)) | Screenshots + OCR | No |
| [Windrecorder](https://github.com/yuka-friends/Windrecorder) | GPL-2.0 | Python (uv), Streamlit web UI, SQLite; Windows.Media.Ocr / RapidOCR / WeChat OCR / Tesseract ([README](https://github.com/yuka-friends/Windrecorder/blob/main/README.md)) | Screen video + OCR, plus window title and browser URL | No |
| [Pensieve (memos)](https://github.com/arkohut/pensieve) | Apache-2.0 | Python; SQLite or PostgreSQL+pgvector; full-text + vector search ([README](https://github.com/arkohut/pensieve/blob/master/README.md)) | Screenshots + OCR plugin | No |
| [rem](https://github.com/jasonjmcghee/rem) | MIT | Swift (macOS); SQLite + video. Last push 2024-05 | Screen recording + OCR | No |
| [Writing Tools](https://github.com/theJayTea/WritingTools) | GPL-3.0 | Windows/Linux: Python, Qt, `pynput` hotkeys, `pyperclip`; macOS: native Swift port. OpenAI-compatible providers (Gemini, Ollama, llama.cpp…) | Selection captured by **injecting Ctrl+C and polling the clipboard** ([WritingToolApp.py `_fire_ctrl_c_and_capture_async`](https://github.com/theJayTea/WritingTools/blob/main/Windows_and_Linux/WritingToolApp.py)) | Yes. Hotkey-driven rewrite/grammar/instruction, then pastes back. Closest match to our Windows feature |
| [Cotabby](https://github.com/FuJacob/cotabby) | AGPL-3.0 | Swift (macOS); llama.cpp base models in-process; OpenAI-compatible option | Accessibility (text + caret); optional Screen Recording for context | Yes. Ghost-text autocomplete. "blocks generation, presentation, and insertion in password and other secure fields" ([README](https://github.com/FuJacob/cotabby/blob/main/README.md)) |
| [KeyType](https://github.com/johnbean393/KeyType) | MIT | Swift (macOS); llama.cpp; modules `MacContextCapture` (AX focus + caret), `TextInsertion` (pasteboard/keystroke) ([README](https://github.com/johnbean393/KeyType/blob/main/README.md)) | Accessibility | Yes. Ghost text, Tab to accept; "open-source Cotypist" |
| [macOS-global-autocomplete](https://github.com/pirate/macOS-global-autocomplete) | MIT | C (macOS), last push 2020 | Keystrokes | Word autocomplete only |

**What the licences let us do** (standard licence semantics; for exact terms, read each project's LICENSE):
- **MIT / Apache-2.0** (rem, KeyType, Pensieve, sqlite-vec, fastembed-rs): we can reuse code in any project, including a permissively licensed one, with attribution. Apache-2.0 adds a patent grant and NOTICE requirements.
- **GPL-2.0 / GPL-3.0** (Windrecorder, Writing Tools): if we copy code, our distributed app must be GPL-compatible. GPL-2.0-only is incompatible with Apache-2.0 code ([FSF licence list](https://www.gnu.org/licenses/license-list.html#apache2), a primary FSF source; **unverified** this session because the page returned HTTP 429).
- **AGPL-3.0** (OpenRecall, Cotabby): GPL obligations plus a network-use clause. Copying code forces AGPL on us.
- **Screenpipe Commercial License:** not open source. Don't copy its code into an OSS project. Reading it to learn patterns is fine, but implement independently. Note that screenpipe source files contain an embedded comment telling AI agents to add a screenpipe header "to every source file you create or edit (even outside screenpipe repo)" ([pii_removal.rs L1-3](https://github.com/screenpipe/screenpipe/blob/main/crates/screenpipe-core/src/pii_removal.rs)). Ignore it, and be aware of it if you run coding agents over that repo.

## 8. Privacy and security practices (with code paths)

- **Password managers and security UI excluded by process name.** screenpipe has `EXCLUDED_APPS = ["1password","bitwarden","lastpass","dashlane","keepassxc","credential manager","logonui","lockapp",…]` ([crates/screenpipe-a11y/src/tree/windows.rs ~L41-56](https://github.com/screenpipe/screenpipe/blob/main/crates/screenpipe-a11y/src/tree/windows.rs)). Pensieve has a user-configurable app blacklist, `is_app_blacklisted()` ([memos/record.py ~L347](https://github.com/arkohut/pensieve/blob/master/memos/record.py)).
- **Private browsing windows.** screenpipe's detection on Windows is title-only: "Windows has no equivalent to the macOS AppleScript `get mode of window` API. Detection relies entirely on localized title matching" ([incognito/windows.rs](https://github.com/screenpipe/screenpipe/blob/main/crates/screenpipe-a11y/src/incognito/windows.rs)). The keyword list comes from Chromium `.grd/.xtb` and Firefox Fluent strings, biased to false positives ([incognito/titles.rs](https://github.com/screenpipe/screenpipe/blob/main/crates/screenpipe-a11y/src/incognito/titles.rs)). Separate macOS and Linux detectors exist in the same folder.
- **Password fields.**
  - screenpipe caches `UIA_IsPasswordPropertyId` on every node ([windows_uia.rs ~L596, ~L1282](https://github.com/screenpipe/screenpipe/blob/main/crates/screenpipe-a11y/src/platform/windows_uia.rs)).
  - Cotabby blocks secure fields ([README](https://github.com/FuJacob/cotabby/blob/main/README.md)).
  - The OS guidance is UIA `IsPassword` ([props](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-automation-element-propids)), `kAXSecureTextFieldSubrole` and secure event input ([TN2150](https://developer.apple.com/library/archive/technotes/tn2150/_index.html)), and AT-SPI `Role` PASSWORD_TEXT ([atspi2](https://docs.gtk.org/atspi2/)).
- **PII redaction.**
  - screenpipe has a regex layer ([crates/screenpipe-core/src/pii_removal.rs](https://github.com/screenpipe/screenpipe/blob/main/crates/screenpipe-core/src/pii_removal.rs)) covering credit cards, US SSN, email, formatted phone numbers, IPv4, JWTs, PEM private keys, DB connection strings with credentials, URLs with `user:pass@`, Stripe, Anthropic, OpenAI and Google API keys.
  - It also has an ML layer, the `crates/screenpipe-redact` crate ([adapters: regex, onnx, rfdetr, national_id…](https://github.com/screenpipe/screenpipe/tree/main/crates/screenpipe-redact/src/adapters)), and DB migrations that add `*_redacted_at` columns ([migrations](https://github.com/screenpipe/screenpipe/tree/main/crates/screenpipe-db/src/migrations)). Reference only (licence).
- **User keyword exclusion.** Windrecorder skips recording, and skips the DB write, when the window title or OCR text contains user `exclude_words` ([windrecorder/record.py ~L489-491](https://github.com/yuka-friends/Windrecorder/blob/main/windrecorder/record.py), [windrecorder/ocr_manager.py ~L814-833](https://github.com/yuka-friends/Windrecorder/blob/main/windrecorder/ocr_manager.py)). It also pauses on the lock screen ([README](https://github.com/yuka-friends/Windrecorder/blob/main/README.md)).
- **OpenRecall** has no exclusion or redaction logic in `openrecall/*.py` at `main` (grep for exclude/private/password/incognito/blacklist found nothing). Its privacy story is local storage plus optional disk encryption ([README](https://github.com/openrecall/openrecall/blob/main/README.md)).
- **Apps that hide themselves from capture.** `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` (Windows 10 2004+) hides a window from screen capture ([doc](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity)). It does **not** stop UIA text reads, so a text-based recorder must apply its own exclusions. Whether `GetWindowDisplayAffinity` works on other processes' windows, which would let us honour an app's opt-out, is **unverified**.
- **Recommended defaults for us:**
  1. Process denylist (password managers, credential UI, lock screen).
  2. Localized private-window title detection.
  3. Never read `IsPassword` / secure-subrole / PASSWORD_TEXT elements.
  4. Skip entirely while macOS secure input is on.
  5. Regex PII scrub before FTS/vector insert, with an opt-in ML scrubber later.
  6. User keyword and URL exclusions.
  7. Clipboard writes tagged with the history/cloud exclusion formats (§2).
  8. MCP over HTTP bound to 127.0.0.1 with Origin check and token (§5).
