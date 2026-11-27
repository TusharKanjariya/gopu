# Gopu

An open-source (Apache-2.0), local-first desktop companion with two jobs:
1. **Memory:** reads on-screen text through OS accessibility APIs (no screenshots) into a local searchable store.
2. **The Gopu key:** tap it in any text box. An empty box gets a reply drafted from the on-screen context, a draft gets polished, and an instruction gets carried out.

Status: early. The only working code is `prototype/recall.py` (a single Windows file, written in Python). Treat it as a reference, not as the architecture.

## Read before working
- `docs/brand/decisions.md`: the name, mascot (a gold baby elephant that writes with its trunk), palette, tagline and licence. All settled; don't reopen them.
- `docs/research/inline-assist-platform.md`: platform APIs and prior art, with sources.
- `docs/adr/0001-tech-stack.md`: the stack (Tauri v2 + Rust core, Windows + macOS in v1, Gopu key first, memory in v1.1).
- `prototype/gopu-key.html`: the chosen Gopu-key animation ("Gopu Walk"), the reference for the overlay.

Private notes, kept outside the public repo (present only on the owner's PC): @.scratch/CLAUDE.private.md

## Hard rules
- **Never copy another app's prompt text, art or names.**
- The mascot is a cartoon character, never based on a real person's likeness or photos, and public material never says where the name comes from. No Ganesha imagery.
- The LLM runs on Groq's free tier (`openai/gpt-oss-120b`), called through an OpenAI-compatible `base_url` so Ollama and llama.cpp work too. The key comes from the `GROQ_API_KEY` user environment variable. Never commit keys.
- Mascot and media assets are made with the VectorCraft MCP, which is limited to reading and writing inside `C:/projects/gopu`.

## Agent skills

### Issue tracker

Issues live as local markdown files under `.scratch/<feature>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

The default five labels: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one root `GLOSSARY.md` plus `docs/adr/`. See `docs/agents/domain.md`.
