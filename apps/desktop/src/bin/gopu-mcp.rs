//! Gopu's MCP server: other AI apps launch this over stdio to read memory, read-only (memory-v1.1 ticket 13).

fn main() {
    gopu_core::mcp::serve();
}
