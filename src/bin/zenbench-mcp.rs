#![forbid(unsafe_code)]

//! MCP server binary for zenbench.
//!
//! Run with: `zenbench-mcp [--project <dir>]`
//! Or configure in your MCP client's settings.

use std::path::PathBuf;

fn main() {
    let project = std::env::args()
        .nth(1)
        .filter(|a| a == "--project")
        .and_then(|_| std::env::args().nth(2))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    zenbench::mcp::run_server(project);
}
