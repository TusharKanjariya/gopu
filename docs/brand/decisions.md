# Gopu brand decisions (2026-11-19)

Source kit: `assets/mascot/GOPU-BRAND-KIT.md`. Research: `docs/research/`.

| # | Decision |
|---|---|
| 1 | Name **Gopu**, wordmark `gopu.` GitHub org `gopu-app` (the `gopu` username is taken). Target domain `gopu.app`. npm, PyPI and crates.io are free. |
| 2 | No written pronunciation ("GO-poo") and no story of where the name comes from in any public material. |
| 3 | Mascot: a **gold baby elephant** (memory animal: "little buddy with a big memory"). The earlier bear concept is retired. |
| 4 | The mascot is also the in-app helper: it must read at **16 px**. States: idle, thinking/writing, done, listening, error. |
| 5 | Tagline "Your little buddy with a big memory.", subtitle "Remembers what you saw. Writes what you mean." |
| 6 | The hotkey is called **the Gopu key**. Default: Right Ctrl. |
| 7 | Licence: **Apache-2.0** for the code. The Gopu name, mascot and logo are reserved (`TRADEMARKS.md`), 2026-11-25. Outside contributions need the CLA (`CLA.md`), so the owner can relicense or sell Gopu, 2026-11-26. |
| 8 | Project lives at `C:\projects\gopu`. VectorCraft MCP has read/write access only inside it. |
| 9 | Elephant confirmed. |
| 10 | A cartoon character with its own personality. **Never** a real person's likeness or photos. |
| 11 | ~~Signature move: writes with the trunk (pencil).~~ Dropped 2026-11-19: no pencil or typing gesture. The "working" signature is walking along the text with a status bubble (see 15). |
| 12 | Mascot colour **Burnt Saffron** (2026-11-19, replaces the kit gold): body `#E8891C`, shade `#B2600B`, outline `#2B170A`, light/toes `#FFD29A`, cheek `#F06A5E`. Never grey. App UI: see 16. |
| 13 | Pose: **side view**, standing, trunk **hanging down naturally** (user call, 2026-11-19; it curls up only as a reaction). No hair tuft, no seated front-facing pose (both read as Ganesha). Next to the caret Gopu is the full side-view body at ~50 px; the tray/app icon is the side-view head only. |
| 14 | Animations are **our own**. No borrowed motions from other apps. **Changed 2026-11-26 (user call):** Ask Gopu may sit in a tab on the screen edge (the default), drawn as Gopu's head in saffron; the other choice is Gopu standing in the taskbar. |
| 15 | Gopu-key animation: **Gopu Walk** (2026-11-19). Gopu drops onto the field's top border by the caret, trunk hanging down. Thinking = looks up, ear flick, speech bubble "Thinking…"; writing = walks along the border with the text, bubble says what it's doing ("Writing a reply…" / "Polishing…" / "On it…"); done = one ~10px hop + "Done!"; error = ears droop, head tilts, "Oops. Your draft is safe." No pencil, no typing gesture, no separate status panel: the bubble is the status. Reference: `prototype/gopu-key.html`; rejected concepts in `prototype/gopu-key-PROTOTYPE.html`. |
| 16 | App UI (2026-11-20): **OS-native look with the brand colour as the accent**. Each platform follows its own system look (Windows 11 settings style, macOS and Linux their own) and light/dark mode; saffron `#E8891C` / `#B2600B` is the only brand colour, used for selection, focus and the primary button. Reason: one look per OS ports cleanly to macOS and Linux. Reference: option C in `prototype/settings-mockups.png`. |

## Guardrails
- A cartoon baby elephant only. No Ganesha cues: no crown, tilak, mouse, multiple arms or lotus.
- Don't copy other apps' prompts, art or names.

## Mascot build order
1. 16 px silhouette test (head, ears and trunk must read as an elephant)
2. Hero mascot (512 px SVG)
3. Five in-app states
4. App icon and tray icon (16, 32, 256), wordmark lockup
