# Telling that a Gopu draft was sent (Windows)

Research for `.scratch/memory-v1.1/issues/07-send-detection.md`, 2026-11-23. Probe: `crates/gopu-core/examples/send_watch.rs` (prints lengths and timings only). **Inferred** marks anything no source states and the probe didn't measure.

## Recommendation

**After every successful write-back, a short-lived send watcher holds the composer element, polls it once a second for up to 15 minutes, and remembers the last non-empty text. When the composer clears, one window read checks that the text now shows in the thread.** The 10 s capture poll isn't used for this.

| Decision | Answer |
|---|---|
| Who starts it | `write_back` returning `Ok` in `apps/desktop/src/main.rs::tap`. It hands over the field's `IUIAutomationElement` (the one `focused_field` already gets) plus the window `HWND`, app, title and the text Gopu wrote. One watcher per run. A newer write-back in the same field replaces the old watcher, and the old pair becomes `unseen`. |
| Watch loop | Every **1 s**, read the held element's text using the same order as `focused_field`: TextPattern `DocumentRange().GetText`, falling back to `ValuePattern.CurrentValue`. Keep `last` = the most recent non-empty text. Gopu having focus isn't required, because the held element is read directly. The loop runs on its own MTA thread, the same way capture does. |
| Why not the capture poll | With a 10 s capture poll the user can edit and send between two reads, so the final text is lost. Capture also reads window leaves, not the field's value. The composer's last value is **the raw text the user sent** (markdown, `:shortcodes:`, before any rendering), which is what the style learner wants. |
| Cost | Measured: a held-field read costs **median 1.2 ms, p95 4–8 ms, max 8 ms** (Discord composer and Chrome omnibox, 72 reads). 900 reads over 15 min is about 1–2 s of wall time in total, so cost doesn't matter. |
| "Empty" | Squash the text the way `crate::landed` does (drop whitespace and U+200B–U+200F, U+2060, U+FEFF). Measured: **Discord's empty composer reads as U+FEFF U+000A, 2 chars, not 0**, so a plain `is_empty()` check never fires there. |
| Clear → commit | When `squash(text)` becomes empty while `last` wasn't: read the window once at **+1 s** (and again at **+4 s** if the first read misses) with the existing `cached_tree` walk. Normalize both sides (below) and check whether the window text holds `last`, using `landed(window, last, None)`'s head-and-tail rule on the normalized text. If it does, that's a **send_commit**. If neither read finds it, the result is `cleared` (the user deleted it or the thread didn't render), and it's **not** a send. |
| Element goes stale | Any call returning `UIA_E_ELEMENTNOTAVAILABLE` means the element "no longer exists, usually because it has been destroyed" ([errors]). Each tick, try to re-attach: if the foreground window is the same `HWND` and `GetFocusedElement()`'s squashed text equals squashed `last`, keep watching that element instead. This covers React/Slate re-mounting the composer (**inferred** to happen on Discord channel switches and Slack re-renders). Don't compare `RuntimeId` across re-mounts: it's "opaque … used only for comparison" and "can be reused over time" ([propids], [compare]). |
| Compose window gone | When the element is stale and the watcher can't re-attach, **and** the host is on the compose-closes-on-send list (Gmail in Chrome or Edge, see table), record **send_probable** with `sent = last`. For every other app, record `unseen`. A stale element alone never counts as a send in other apps, which avoids false sends in Explorer, 7-Zip and Notepad without an exclusion list (an Explorer rename box disappears too). |
| Timeout | After 15 min with no clear, record `unseen`. |
| Hang guard | Use the same ~2 s `IUIAutomation2` transaction and connection timeouts that capture uses (`capture-trigger.md`). |
| Enter key | Not used. The tap hook already sees keys, but the clear-plus-thread check is enough, and it also covers send buttons. |
| UIA events | Not used. Microsoft: "Do not assume that all possible events are raised by a UI Automation provider" ([events]). Polling one element costs 1 ms, so events aren't worth it. |

### Normalizing for comparison and grading

`norm(s)`: NFC isn't available without a dependency, so skip it. Lowercase, map curly quotes and apostrophes to straight ones, drop zero-width and BOM characters, collapse whitespace runs to one space, and trim. For the **thread match only**, also drop the markdown markers `* _ ~ \` | >` and the leading `#`/`-` of each line, because Discord and Slack render them away, so the thread never shows them.

### Similarity and grades

- **Measure:** normalized char-level Levenshtein, `1 - dist / max(len_a, len_b)`, over `norm(draft)` and `norm(sent)`. It's about 15 lines with two rows of `usize`, O(n·m) with n, m ≤ ~2,000 chars (≤ 4 M steps, well under 10 ms). No new dependency. (`strsim` is already in `Cargo.lock` through another crate, and `strsim::normalized_levenshtein` is the drop-in if we'd rather not own the code.) It needs one `assert` self-check.
- **Thresholds (starting values, tune on real pairs):**
  - `verbatim`: sim ≥ **0.97** (case, a trailing period, an emoji added)
  - `edited`: **0.50 ≤ sim < 0.97**
  - `replaced`: sim < **0.50**
- **Verdict**, derived from how the watch ended:

| Outcome | Verdict |
|---|---|
| send_commit or send_probable, grade verbatim | `kept` |
| … grade edited | `edited` |
| … grade replaced | `replaced` |
| timeout, cleared without a thread match, stale outside the compose list, superseded by a newer tap | `unseen` |

### What a pair holds

One row per Gopu write-back. It extends the tap's own record rather than adding a separate table (the shape is up to ticket 04).

| Field | Notes |
|---|---|
| `draft` | The text Gopu wrote (the `write_back` input) |
| `sent` | The composer's last non-empty value before the clear (raw, unrendered). Null when unseen |
| `similarity` | 0..1, as above. Null when unseen |
| `grade` | verbatim / edited / replaced |
| `verdict` | kept / edited / replaced / unseen |
| `source` | `commit` (thread matched) / `probable` (compose closed) / `none` |
| `case` | The field case of the tap (empty / draft / instruction / selection), because a reply to an empty box and a polish teach different things |
| `app`, `host`, `title` | Process name, browser site taken from the title suffix (e.g. "Gmail", "WhatsApp", "LinkedIn"), and the window title at write-back |
| `recipient` | Best effort, nullable: the conversation title for Discord and Slack (the window title), the chat header for WhatsApp, the To-chip names for Gmail (read from the same `cached_tree` walk at write-back). Used to link to people pages (09) |
| `written_at`, `sent_at` | Time-to-send gives survival buckets (sent within 1 m, 10 m, …) for free |

Masks (ticket 03) apply to `draft` and `sent` before storage, the same as captured text.

### Which pairs the style learner gets

- **Learn from:** `source = commit` with verdict `edited` or `replaced`. These are the user's corrections of Gopu, which is the signal. Also use Gmail `probable` pairs that are `edited`, because closing the compose window is Gmail's normal send path.
- **Count but don't learn the text:** `kept` pairs. They show Gopu was already right, so use them as a weight that reinforces the current rules.
- **Never:** `unseen`; pairs where `sent` is under ~10 characters ("ok", "👍"); `replaced` pairs with sim < 0.15 when the tap's case was `draft` or `instruction` (most likely the user sent something unrelated in the same box, not a correction). **Inferred**, tune later.

## Per app (Chrome/Edge for the web apps)

Measured on this machine: Discord's composer is an Electron (`FrameworkId = Chrome`) Slate editor with TextPattern and ValuePattern. A cached walk of the whole window finds it in about 31–34 ms. Chrome's open window had no web composer, so the browser rows rely on the existing slice-4 write-back working in those apps (TextPattern on the contenteditable) and are **inferred** for send behaviour.

| App | Clears on send? | Thread shows | Pitfalls | Expected source |
|---|---|---|---|---|
| Discord (desktop) | Yes, instantly, to U+FEFF U+000A (measured empty value) | The message, with markdown rendered (`**b**` shows as bold, no asterisks), `:shortcode:` emoji as images, and custom emoji as images whose name is **inferred** to be `:name:` | Squash U+FEFF. Strip markdown for the thread match. A channel switch keeps the draft per channel, so a stale element plus a re-attach matters. A reply chip above the composer isn't part of the text | commit |
| Slack (desktop, Electron) | Yes | WYSIWYG composer: formatting is applied in the box itself ([slack]), so the field value is already plain text. Links unfurl below the message | `:shortcodes:` turn into emoji as you type (**inferred**), so `sent` holds the emoji. A "draft" that's still being edited in a thread side panel is a separate composer element, so key the watcher on the element, not the window | commit |
| WhatsApp desktop / web | Yes | The bubble, with `*bold*` and `_italic_` rendered and a time plus tick marks next to it | Its own status leaves ("✓✓", the time) sit next to the text. The head-and-tail containment match ignores them. The window title is just "WhatsApp", so the recipient comes from the chat header leaf | commit |
| Gmail (Chrome/Edge) | **The compose window closes on send** (pop-out or inline reply). With Undo Send there's a 5/10/20/30 s cancel window ([gmail]) | The sent mail isn't in view (inline replies do show in the thread after the cancel window) | Discarding a draft also closes the compose window, so `probable` stays probable. The signature and quoted thread below the draft are kept text: compare only `split_kept(last).0`. Inline replies: the +4 s thread read can miss Undo Send's 10 s+ window, so add a third read at +35 s for Gmail only | probable (popout), commit or probable (inline) |
| LinkedIn messaging (Chrome/Edge) | Yes, in the message overlay or the /messaging page | The message bubble | The overlay can be minimized or closed. That's a stale element **without** a send, so LinkedIn isn't on the compose-closes list. Feed post and comment boxes behave differently (the post dialog closes on Post): **inferred** commit for comments, `unseen` for posts in v1.1 | commit |
| Firefox | Ticket 10 | | | |

Compose-closes-on-send list for v1.1: **Gmail only** (the host taken from the window title suffix "- Gmail"). Outlook can join when someone checks it.

## Throwaway probe

`cargo run -p gopu-core --example send_watch --release [secs]` lists the editable fields in Discord, Chrome and Edge, times reads on held elements, and logs length changes and clears. To see a real clear, run it while you send a message yourself (Gopu never types or sends). Results from this session: the numbers above. No send was observed, because the probe doesn't send anything.

## Sources

- [propids]: UIA_RuntimeIdPropertyId ("Identifiers can be reused over time … treated as an opaque value"), https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-automation-element-propids
- [errors]: UIA_E_ELEMENTNOTAVAILABLE 0x80040201, https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-error-codes
- [compare]: IUIAutomation::CompareElements (compares runtime IDs), https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation-compareelements
- [events]: Subscribing to UI Automation Events, https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-eventsforclients
- ValuePattern, https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nn-uiautomationclient-iuiautomationvaluepattern ; TextRange GetText, https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationtextrange-gettext
- [gmail]: Google, "Unsend an email in Gmail" (5/10/20/30 s), https://support.google.com/mail/answer/2819488
- [slack]: Slack, "Format your messages" (WYSIWYG toolbar), https://slack.com/help/articles/202288908-Format-your-messages
- Discord markdown: https://support.discord.com/hc/en-us/articles/210298617 (returned 403 to the fetcher, so the rendering claims above come from the app's known behaviour and are **inferred**)
- Existing code reused: `platform::windows::{focused_field, cached_tree, text_pattern}`, `crate::{landed, split_kept}`
