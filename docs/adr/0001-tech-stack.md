# 0001 · Tech stack

Date: 2026-11-19 · Status: accepted

## Context

v1 is **the Gopu key** on **Windows and macOS** at the same time: tap Right Ctrl in any text field, read the on-screen context through accessibility APIs, then reply, polish or follow the instruction, with Gopu walking along the field's top border (brand decision 15). Background memory capture comes next, in v1.1.

To do that, the app has to:
- read other apps' text (UI Automation on Windows, AXUIElement on macOS);
- catch a lone Right Ctrl tap system-wide;
- paste text back;
- draw a transparent, click-through mascot over any app;
- call an OpenAI-compatible LLM.

Research: `docs/research/inline-assist-platform.md`.

## Decision

| Layer | Choice |
|---|---|
| Shell | **Tauri v2** (system webview: WebView2 / WKWebView). Official plugins for tray, autostart, signed updater. |
| Core language | **Rust** for everything except UI: field reading, hotkey, paste-back, context building, prompts, LLM client, storage. |
| Platform layer | One `Platform` trait with two implementations: Windows (`windows` crate: UIA, `SetWindowsHookEx` low-level keyboard hook, `SendInput` paste) and macOS (AXUIElement + `AXIsProcessTrustedWithOptions`, CGEventTap for the key, `IsSecureEventInputEnabled` → do nothing). |
| Gopu overlay | A transparent, always-on-top, click-through Tauri window rendering the Gopu Walk HTML/SVG from `prototype/gopu-key.html`. The caret and field rectangle come from the platform layer, not from the DOM mirror. |
| Settings / search UI | TypeScript + Vite, no framework until the UI grows enough to need one. |
| Storage | SQLite via `rusqlite` (bundled) with FTS5, one local file (`%LOCALAPPDATA%\Gopu\memory.db`). Embeddings are not in v1.1: FTS5 + bm25 is enough (memory-v1.1 ticket 08). `sqlite-vec` and a local embedding model come only when a real search fails that FTS5 can't fix. |
| LLM | One OpenAI-compatible client with configurable `base_url`. Default Groq `openai/gpt-oss-120b` with key from `GROQ_API_KEY`; Ollama and llama.cpp work by changing `base_url`. |
| MCP | stdio server, with memory (v1.1). |

Layout (created when the code starts, not before):

```
apps/desktop/        Tauri app: windows (overlay, settings), tray, wiring
crates/gopu-core/    platform-neutral: field cases, context, prompts, LLM, storage
crates/gopu-core/src/platform/{windows,macos}.rs   the Platform trait's two implementations
```

## Consequences

- One codebase and one language for the core. Small installers with no bundled runtime.
- Both platforms in v1 roughly doubles the native work: two accessibility walkers, two key listeners, two paste paths, two overlay setups. CI must build and smoke-test both.
- macOS needs the user to grant **Accessibility** (and likely **Input Monitoring** for the key tap), and needs signing and notarization for distribution. The onboarding flow must explain why.
- To verify early with a spike, not assumed:
  - Tauri transparent click-through overlay on macOS (transparency needs `macOSPrivateApi`, which blocks App Store distribution);
  - reliable lone-Right-Ctrl detection on macOS;
  - caret rectangle quality in Chromium/Electron apps on both OSes.
- `prototype/recall.py` stays a reference only. Its logic gets ported, not wrapped.

## Rejected

- **Electron**: bundles Chromium and Node; too heavy for an always-on companion.
- **Python + sidecar**: two runtimes, bigger installs, painful hook and overlay packaging.
- **C#/WPF**: best UIA story, but Windows-only.

## Spike results: Windows (2026-11-19)

From `.scratch/v1-platform-spike/` tickets 02–04:
- **Overlay:** Tauri v2 config (`transparent`, `alwaysOnTop`, `focus:false`, `focusable:false`, no decorations or shadow) plus `set_ignore_cursor_events(true)` gives a click-through window that never takes focus and keeps animating. Add `WS_EX_TOOLWINDOW` by hand to keep it out of Alt+Tab.
- **Caret:** UIA TextPattern2 → TextPattern selection → Win32 `GetGUIThreadInfo` caret → field start. Chromium/Electron have no TextPattern2. An empty Chromium field reports the whole field as its caret, so treat it as "caret at the field's text start". Don't detect a field by control type (contenteditables are `GroupControl`).
- **Gopu key:** `WH_KEYBOARD_LL` + `WH_MOUSE_LL` on a dedicated thread, pass-through only, skip `LLKHF_INJECTED`. A lone Right Ctrl under 500ms is a tap. Another key, mouse button or wheel while it's held cancels it.

macOS (ticket 05) still needs to be checked.
