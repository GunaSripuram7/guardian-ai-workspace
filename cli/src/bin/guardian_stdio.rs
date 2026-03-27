// cli/src/bin/guardian_stdio.rs
// Thin stdio ↔ HTTP bridge. OpenClaw spawns this as a child process.
// It reads JSON-RPC lines from stdin, POSTs to Guardian's HTTP server,
// writes the response to stdout. Guardian HTTP server must be running first.

use std::io::{self, BufRead, Write};

#[tokio::main]
async fn main() {
    let client = reqwest::Client::new();
    let guardian_url = "http://127.0.0.1:3000/";
    let stdin  = io::stdin();
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) if !l.trim().is_empty() => l,
            _ => continue,
        };

        // Forward the JSON-RPC request to Guardian's HTTP server
        match client
            .post(guardian_url)
            .header("Content-Type", "application/json")
            .body(line)
            .send()
            .await
        {
            Ok(resp) => {
                if let Ok(text) = resp.text().await {
                    let _ = writeln!(out, "{}", text.trim());
                    let _ = out.flush();
                }
            }
            Err(e) => {
                // Write a JSON-RPC error back so OpenClaw doesn't hang
                let err = format!(
                    r#"{{"jsonrpc":"2.0","id":null,"error":{{"code":-32000,"message":"Guardian unreachable: {}"}}}}"#,
                    e
                );
                let _ = writeln!(out, "{}", err);
                let _ = out.flush();
            }
        }
    }
}
