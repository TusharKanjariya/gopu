# When does capture read the screen? (Windows)

Research for `.scratch/memory-v1.1/issues/02-capture-trigger.md`, 2026-11-23. Bench: `crates/gopu-core/examples/capture_bench.rs`.

## Recommendation

**Trigger: both, with the poll as the backbone.** A WinEvent hook says *when to read soon*; a slow poll guarantees we never miss a change the app didn't announce.

| Decision | Answer |
|---|---|
| Event trigger | `SetWinEventHook` out-of-context for `EVENT_SYSTEM_FOREGROUND` only (plus `EVENT_OBJECT_NAMECHANGE` on the foreground hwnd's root, for title changes such as switching tabs or chats). Each one schedules a read **1 s later** (debounce: a newer event pushes the read back; at most one read per 1 s). Do not subscribe to UIA text, structure or property events. |
| Poll | Read the foreground window every **10 s** while the user is active. recall.py's 5 s is fine on CPU but doubles the cost for little gain once window switches are caught by the event. |
| Idle rule | Skip reads when `GetLastInputInfo` says no input for **60 s**; the first input after idle schedules an immediate read. Also skip when the foreground window is Gopu itself, a skip-listed app (password managers, `LockApp`), or `IsHungAppWindow` is true. |
| Dedup | Hash `app + title + visible leaf text` (SHA-1 or std `DefaultHasher`; collisions don't matter here). Same hash as the last stored row for that window: bump `last_seen` only. Different: insert. This is recall.py's `record()` behaviour. Hashing costs 0.1–3 ms. |
| Hang guard | Use `IUIAutomation2::put_TransactionTimeout` (default 20 s) and `put_ConnectionTimeout` to cap a read at about 2 s, so one hung app can't stall capture. |
| Process model | **A dedicated thread inside the Tauri app**, COM MTA, owning no windows. It holds the WinEvent hook and its message loop and does the reads, so the Gopu key thread and the UI thread are never blocked. The **MCP server is a separate small binary** (`gopu-mcp`, stdio) that MCP clients launch themselves. It opens the same SQLite store read-only (WAL mode, so a reader never blocks the writer). It does not need the reader. While the app isn't running, memory is still searchable; it just isn't growing. |

Why not UIA events as the main trigger:
- Microsoft says events beat polling for efficiency ([Subscribing to UI Automation Events][events]), but also: "Do not assume that all possible events are raised by a UI Automation provider" (same page). A capture that relies on text-changed events misses whatever the app doesn't announce. Chromium only builds its tree once a client asks for it (ADR 0001 spike), and Firefox gave us an empty tree (below).
- Subscribing to UIA text, structure or property events across the desktop makes every app marshal events to us cross-process. Even with the machine idle, the cheap WinEvent `OBJECT_LOCATIONCHANGE` ran at 18/s (table below). Structure and text events in a busy chat or editor are far higher. `IUIAutomation6::CoalesceEvents` ([IUIAutomation6][uia6]) helps, but only on 1809+ and only with duplicate events.
- UIA event handlers bring threading rules: register and remove on one MTA thread, never from a UI thread ([Understanding Threading Issues][threading]). A plain foreground WinEvent hook only needs a message loop on its thread ([SetWinEventHook][swh]).

## Measurements

Windows 10 22H2, this dev machine, `--release`, 10 reads per window, two runs. Each read is one UIA cache request (`BuildUpdatedCache`, subtree, control view) on the app's main window: the same request as `cached_tree` in `platform/windows.rs`. "Our cpu" / "their cpu" = process CPU time per read (`GetProcessTimes`). Another UIA client was also running, so this machine was already being read by it. No Chrome window was open.

| App (main window) | Nodes | Visible leaf chars | Read ms (median, run 1 / 2) | Read ms max | Our CPU ms/read | Target CPU ms/read |
|---|---|---|---|---|---|---|
| VS Code | 159 | 1,614 | 46 / 69 | 107 | 14–20 | 19–36 |
| Discord | 471 | 2,193 | 76 / 156 | 323 | 16–19 | 56–80 |
| Firefox | 17 | **0** | 11 / 23 | 34 | 6–8 | 3–17 |
| Sublime Text | 26 | 6 | 32 / 81 | 134 | 23–38 | 6–22 |

- `on_screen_context` (the real slice-2 path, foreground = Firefox): 276 / 452 ms, 0 chars. The extra time is Firefox's slow first answer.
- Text hashing over a whole read: 0.1–3 ms.
- Repeat reads of an unchanged window gave one distinct hash each time, so dedup by hash works (no jitter in the visible-leaf text).
- Cost of a 10 s poll, worst case (Discord): about 100 ms CPU per read, both sides together. That's about 1% of one core while active and 0 while idle. A 5 s poll would be about 2%.
- WinEvents over 15 s, idle desktop, all processes, out of context: FOREGROUND 0, FOCUS 3, NAMECHANGE 20, VALUECHANGE 6, REORDER 1, SHOW 15, LOCATIONCHANGE 278 (18.5/s). Subscribe only to what's needed, and filter on hwnd in the callback.

## Pitfalls to build around

1. **Hung apps.** A cross-process UIA call waits on the target. The default transaction timeout is 20 s ("some operations require the provider to process hundreds of elements") ([put_TransactionTimeout][tt]). Lower it, and check `IsHungAppWindow` first. It's true once an app hasn't pumped messages for 5 s ([IsHungAppWindow][hung]). Microsoft marks it "not intended for general use", so treat it as a hint.
2. **Threading.** Make UIA calls from a thread that owns no windows and is COM MTA. Calls from a UI thread "can lead to very slow performance, or even cause the application to stop responding" ([Understanding Threading Issues][threading]). The existing reader already calls `CoInitializeEx(COINIT_MULTITHREADED)`, so a dedicated capture thread fits. Don't share elements across apartments ([same page][threading]).
3. **The hook thread needs a message loop.** Out-of-context events arrive queued and in order, on the thread that called `SetWinEventHook` ([SetWinEventHook][swh]). Keep the callback trivial (set a deadline) so it can't reenter.
4. **Idle is per session.** `GetLastInputInfo` only reports the calling session, and its tick count can go backwards ([GetLastInputInfo][gli]). Use wrapping subtraction (recall.py already does).
5. **Chromium/Electron trees are lazy.** The first query builds the tree. The spike's 150 ms retry covers the Gopu key. For capture, the next poll picks up the full tree.
6. **The slice-2 reader isn't capture-shaped yet.** `on_screen_context` needs a `Field`, splits text around it, and keeps only the last 6,000 chars (`MAX_BEFORE`). Capture needs the same walk with no field and its own cap. This is a small sibling function next to `cached_tree`, not a rewrite.

## Open questions for the user

- **Firefox returns no page text** through this reader (17 nodes, 0 chars, foreground or not). If you use Firefox daily, memory will be blind there until someone looks into it (Gecko's UIA/MSAA exposure, or a fallback). Worth a ticket?
- The 10 s poll and 60 s idle are guesses. Shorter catches more short-lived screens but costs more.

## Sources

- [events]: https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-eventsforclients
- [threading]: https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-threading
- [uia6]: https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nn-uiautomationclient-iuiautomation6
- [uia2]: https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nn-uiautomationclient-iuiautomation2
- [tt]: https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation2-put_transactiontimeout
- [swh]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook
- [gli]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getlastinputinfo
- [hung]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-ishungappwindow
- ADR 0001 § "Spike results: Windows" (Chromium lazy tree, caret findings), `docs/adr/0001-tech-stack.md`
- `prototype/recall.py` (`record()` dedup, `idle_seconds()`, 5 s / 120 s constants)

[events]: https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-eventsforclients
[threading]: https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-threading
[uia6]: https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nn-uiautomationclient-iuiautomation6
[tt]: https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation2-put_transactiontimeout
[swh]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook
[gli]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getlastinputinfo
[hung]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-ishungappwindow
