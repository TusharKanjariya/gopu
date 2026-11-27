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
