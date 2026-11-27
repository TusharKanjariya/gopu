# <img src="assets/mascot/gopu-glyph-256.png" width="36" alt=""> Gopu

**A writing helper that remembers what you saw, and keeps it on your PC.**

Tap the **Gopu key** (Right Ctrl) in any text box. Empty: Gopu drafts a reply from the conversation on screen. A
draft: it polishes it in your style. An instruction like *"turn this into a polite no"*: it does it. Tap outside a
text box to ask your memory: *"what did I do today?"*

<i>[ demo: a tap of the Gopu key, Gopu walks along the field and writes the reply ]</i>

**Windows 10 / 11 · free and open source (Apache-2.0) · 0.1.0 preview** ·
[Download](https://github.com/TusharKanjariya/gopu/releases/latest)

## What stays on your PC

| Stays on your PC | Leaves your PC |
|---|---|
| Everything memory captures, in one local file | A Gopu-key tap or a question: sent to **your** model, only when you ask |
| Your people pages, style rules and chats | The daily update check, which carries nothing about you |
| Your model key, in the Windows credential store | Nothing else. No account, no analytics |

Memory never reads passwords, sign-in or payment pages, banks, health sites, password managers or private windows.
Card numbers, keys and ID numbers are masked before anything is saved. Pause it, add apps to "Never capture", or
turn it off in **Settings → Privacy**.

## Why Gopu

- **You can read the code** of the app that reads your screen.
- **Any model, your key.** Groq's free tier by default; OpenAI, Anthropic, OpenRouter, or Ollama for fully local.
- **It knows who you're writing to.** People pages and style rules learned from your edits make replies sound like
  you, and you can read and edit every one.
- **It only speaks when asked.** No tours, no nudges, no model calls in the background.

## Install

Windows 10 or 11 (64-bit). No admin rights needed.

1. Download **`Gopu_0.1.0_x64-setup.exe`** from the [latest release](https://github.com/TusharKanjariya/gopu/releases/latest).
2. Windows may say **"Windows protected your PC"**. Click **More info → Run anyway**. This preview isn't code-signed
   yet; to check the download is ours, compare `Get-FileHash .\Gopu_0.1.0_x64-setup.exe` with `SHA256SUMS.txt` on
   the release page.
3. The welcome asks for a model key. A free [Groq key](https://console.groq.com/keys) takes a minute, or pick
   Ollama to keep everything on your PC.
4. Click into any text box and tap **Right Ctrl**.

Gopu starts with Windows and updates itself when you say so. Uninstalling keeps your memory unless you tick the box
to delete it.

## Use your memory in other AI apps

Turn on **Settings → Privacy → Let other AI apps read my memory**, then **Copy setup** for Claude Code, Claude Desktop,
Cursor or any MCP client. It's read-only and masked, and nothing is added to other apps for you.

## Build from source

```sh
cargo test -p gopu-core
cargo build -p gopu-desktop            # Gopu.exe and gopu-mcp.exe
cd apps/desktop && npx @tauri-apps/cli@2 build   # the installer
```

## Status

0.1.0 is a **preview**: Windows only, English interface, and settings or memory formats may change between 0.x
versions. See [known issues](https://github.com/TusharKanjariya/gopu/issues). macOS is next.

## Contributing

Issues and pull requests are welcome; see [CONTRIBUTING.md](CONTRIBUTING.md). Security problems go through
[SECURITY.md](SECURITY.md).

## Licence

Code: [Apache-2.0](LICENSE). The Gopu name, mascot and logo are reserved: [TRADEMARKS.md](TRADEMARKS.md).
