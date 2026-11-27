# Why did the capture bench read no Firefox page text? (Windows)

Research for memory v1.1 ticket 10, 2026-11-23. Probe: `crates/gopu-core/examples/firefox_probe.rs` (throwaway; prints counts only). Firefox Developer Edition 161 on Windows 10 22H2. Another UIA client was running too.

## Recommendation

**The slice-2 reader already reads Firefox page text. It needs no change for the Gopu key.** For background capture, add one guard and follow one rule:

1. **No field, no anchor.** `Walk::visit` treats any element whose rectangle equals `field.rect` as the field and skips that element's subtree. With `Field::default()` (a zero rect), it anchors on the first zero-size Gecko element and loses the page. The capture sibling of `on_screen_context` (capture-trigger.md, pitfall 6) must skip the anchor check when there's no field. The smallest form is one condition in `Walk::visit`: `self.field.w > 0 && …` before the rectangle compare. The Gopu key never hits this, because `focused_field` rejects zero-size fields.
2. **Read only the foreground window**, which capture-trigger.md already settles. Firefox marks the whole page `IsOffscreen` once its window is occluded or minimized, so a background read returns the tree with no visible text.

Not needed:
- **The raw view.** It returned about 33% more visible leaf text in one real window (2,321 vs 1,750 chars), because Gecko marks some text leaves as non-control elements. It's worth a look if Firefox context feels thin. It would change every app, so it isn't part of this fix.
- **A lazy-activation retry like Chromium's.** A fresh Firefox process gave its full tree on our first read (see the evidence below, with its caveat).
- **Changes to the UIA pref.** Native UIA is on by default and was the provider serving the tree.

The bench's own numbers are misleading. Its `on_screen_context` line used `Field::default()` and printed only `before`, and its per-app table read Firefox while the window was in the background. If the bench is reused, pass a field rect that can't match (as the probe does) and count `after` as well.

## Evidence

### What each read gave (probe, control view unless stated)

| Case | Nodes | Visible leaf chars | `on_screen_context` |
|---|---|---|---|
| Real window (82 tabs), not foreground (occluded by other windows, not minimized) | 5,120 | 26 (2,366 of 2,374 leaves `IsOffscreen`; 0 of 82 documents onscreen) | not run |
| Same window, foreground | 5,120 | 1,750 (raw view 2,321; one onscreen document whose TextPattern `DocumentRange` gives 1,581) | **871 chars**, 236–275 ms |
| Same window, foreground, bench's `Field::default()` | — | — | before 0, after 871, `anchored = true` |
| Fresh process, local test page, foreground | 491 | 2,195 | **1,969 chars**, 36–37 ms |
| Fresh process, `Field::default()` | — | — | before 0, after 1,500, `anchored = true` |
| Fresh process, timeline after the first query | 491 at +44 ms, unchanged to +3.4 s | 2,195 throughout | — |

The tree was complete on the probe's first read every time, and in all views. No document was missing from the control view. The visible document sits at Window > Pane > Pane > Pane > Document, so it is outside the reader's "quiet" chrome types (ToolBar, Tab and so on). Only the address bar text is dropped as chrome, which is intended.

### Candidate by candidate

- **The bench read a background window: yes, this is the cause of the 0 chars in the table.** Gecko's `RemoteAccessible::State()` adds `states::OFFSCREEN` to everything in a document whose browsing context is inactive ("we're in a background tab and inherently offscreen") ([RemoteAccessible.cpp][remote]). Firefox deactivates the selected tab's docshell when `document.hidden` becomes true. `AsyncTabSwitcher.onVisibilityChange` handles "minimized or occluded" windows ([AsyncTabSwitcher.sys.mjs][switcher]), and Windows occlusion tracking is on by default (`widget.windows.window_occlusion_tracking.enabled: true`) ([StaticPrefList.yaml][prefs]). Gecko's UIA provider maps `IsOffscreen` straight from that state ([uiaRawElmProvider.cpp][uiaprov]). Microsoft's definition says IsOffscreen "is not affected by occlusion by other windows" ([Automation Element Property Identifiers][propids]), so Firefox departs from the spec here, and we have to work around it rather than expect it to change.
- **The bench's `on_screen_context` line used a zero field rect: yes, this is the cause of its "0 chars kept".** Reproduced on both windows (table above). A UIA bounding rectangle defaults to `[0,0,0,0]` for elements that aren't displaying ([same page][propids]), so a zero rect is a common value, not a sentinel.
- **Lazy activation: real, but not what we hit.** Firefox answers `WM_GETOBJECT` for `UiaRootObjectId` through `LazyInstantiator::GetRootUia` ([nsWindow.cpp][nswindow]). It builds the real tree only after `ShouldInstantiate`, which refuses blocklisted client executables ([LazyInstantiator.cpp][lazy]). The parent also requests cache domains on demand: until the `State` domain arrives, `State()` returns 0 ([DocAccessibleParent.cpp][dap], [RemoteAccessible.cpp][remote]). That can only make a first read over-include, never come back empty. Caveat: another UIA client was also querying windows, so "our first query" may not have been the process's first query. Firefox ignored a minimized launch, so a clean isolation wasn't possible. The bench's **17 nodes** was not reproduced. It may have been a pre-instantiation stub, or another Firefox window: the bench takes the first enumerated visible, titled window, not the biggest as its comment says. Either way, the reader works on the next read.
- **Native UIA off, served through the MSAA/IA2 proxy: no.** `accessibility.uia.enable` defaults to `2` ("Enable unless incompatible accessibility clients are detected") ([StaticPrefList.yaml][prefs]). `ProviderDescription` on the root shows Gecko's own provider ("Main(parent link): … unmanaged:xul.dll"), and `FrameworkId` is "Gecko".
- **The cache request or filter misses the content-process subtree: no.** The control view gave all 82 tab documents, with depth up to 31. Raw and content views gave the same document count. Firefox's parent process holds the cached content tree, so one cache request from the top-level HWND covers it.

### Why the Gopu key already worked

`focused_field` asks for the focused element of the foreground window, which is never occluded, and slice 4 rebuilds rich-editor text from its children. `on_screen_context` reads the same foreground window with a real, non-zero field rect. Neither path meets the background-offscreen or zero-rect problems.

## Sources

- [remote]: https://github.com/mozilla-firefox/firefox/blob/main/accessible/ipc/RemoteAccessible.cpp (`RemoteAccessible::State`, OFFSCREEN for inactive browsing contexts and the viewport cache)
- [switcher]: https://github.com/mozilla-firefox/firefox/blob/main/browser/components/tabbrowser/AsyncTabSwitcher.sys.mjs (`windowHidden` = `document.hidden`, `onVisibilityChange` deactivates docshells when minimized or occluded)
- [prefs]: https://github.com/mozilla-firefox/firefox/blob/main/modules/libpref/init/StaticPrefList.yaml (`accessibility.uia.enable: 2`, `widget.windows.window_occlusion_tracking.enabled: true`)
- [uiaprov]: https://github.com/mozilla-firefox/firefox/blob/main/accessible/windows/uia/uiaRawElmProvider.cpp (`UIA_IsOffscreenPropertyId` from `states::OFFSCREEN`; `IsControlElement`/`IsContentElement` from `IsControl()`)
- [nswindow]: https://github.com/mozilla-firefox/firefox/blob/main/widget/windows/nsWindow.cpp (`WM_GETOBJECT` → `LazyInstantiator::GetRootUia`)
- [lazy]: https://github.com/mozilla-firefox/firefox/blob/main/accessible/windows/msaa/LazyInstantiator.cpp (`ShouldInstantiate`, client blocklist, UIA client detection)
- [dap]: https://github.com/mozilla-firefox/firefox/blob/main/accessible/ipc/DocAccessibleParent.cpp (`RequestDomainsIfInactive`)
- [propids]: https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-automation-element-propids (IsOffscreen not affected by occlusion; BoundingRectangle default `[0,0,0,0]`)
- `crates/gopu-core/src/platform/windows.rs` (`on_screen_context`, `cached_tree`, `Walk::visit`), `crates/gopu-core/examples/capture_bench.rs`, `docs/research/capture-trigger.md`

[remote]: https://github.com/mozilla-firefox/firefox/blob/main/accessible/ipc/RemoteAccessible.cpp
[switcher]: https://github.com/mozilla-firefox/firefox/blob/main/browser/components/tabbrowser/AsyncTabSwitcher.sys.mjs
[prefs]: https://github.com/mozilla-firefox/firefox/blob/main/modules/libpref/init/StaticPrefList.yaml
[uiaprov]: https://github.com/mozilla-firefox/firefox/blob/main/accessible/windows/uia/uiaRawElmProvider.cpp
[nswindow]: https://github.com/mozilla-firefox/firefox/blob/main/widget/windows/nsWindow.cpp
[lazy]: https://github.com/mozilla-firefox/firefox/blob/main/accessible/windows/msaa/LazyInstantiator.cpp
[dap]: https://github.com/mozilla-firefox/firefox/blob/main/accessible/ipc/DocAccessibleParent.cpp
[propids]: https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-automation-element-propids
