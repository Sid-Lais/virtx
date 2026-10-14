//! A browser in a console: Chromium in the VM, driven from the command line with
//! `agent-browser` and watched and used from your own browser through `viewer`, both in `/abin`.
//!
//! ```sh
//! cargo run --example browser
//! ```
//!
//! It opens example.com and follows its link, then runs the `agent-browser` commands you type
//! (`snapshot -i`, `click @e1`, `open <url>`, ...) until an empty line. All the while, the page
//! it prints shows the browser live and takes your mouse and keyboard.

use std::io::{BufRead as _, Write as _};

use virtx::{console::ConsoleClient, ensure_virtx, image::Recipe, protocol::Port};

/// The host port the viewer is published on.
const VIEWER_PORT: u16 = 4849;

/// Run `cmd` in the console, and return what it printed.
async fn sh(console: &mut ConsoleClient, cmd: &str) -> anyhow::Result<String> {
    let result = console.exec(["sh", "-c", cmd], None).await?;
    let out = String::from_utf8_lossy(&result.stdout) + String::from_utf8_lossy(&result.stderr);
    Ok(out.trim_end().to_string())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ensure_virtx().await?;
    let mut console = ConsoleClient::builder()
        .image(Recipe::new("debian:trixie-slim").step(
            "apt-get update \
             && apt-get install -y --no-install-recommends chromium fonts-noto-cjk ca-certificates \
             && rm -rf /var/lib/apt/lists/* /var/log/journal",
        ))
        .network(true)
        .ports([format!("{VIEWER_PORT}:4849").parse::<Port>()?])
        .build()
        .await?;

    // Headless: what you see is the viewer's, from the browser's DevTools port. Chromium logs
    // once that port is open.
    sh(
        &mut console,
        "setsid nohup chromium --headless --no-sandbox --user-data-dir=/root/chromium \
         --remote-debugging-port=9222 about:blank >/tmp/chromium.log 2>&1 & \
         until grep -q \"DevTools listening\" /tmp/chromium.log; do sleep 0.2; done",
    )
    .await?;
    // The viewer lets in only a page that presents this token.
    let token = sh(
        &mut console,
        "head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \\n'",
    )
    .await?;
    sh(
        &mut console,
        &format!(
            "setsid nohup viewer --listen 127.0.0.1:4849 --token {token} >/tmp/viewer.log 2>&1 &"
        ),
    )
    .await?;
    println!("Watch and use the browser at http://localhost:{VIEWER_PORT}/#t={token}\n");

    for command in [
        "open https://example.com",
        "get title",
        "snapshot -i",
        "click @e1",
        "get url",
    ] {
        println!("$ agent-browser {command}");
        println!(
            "{}\n",
            sh(&mut console, &format!("agent-browser --cdp 9222 {command}")).await?
        );
    }

    println!("Type agent-browser commands, or an empty line to end.");
    let mut stdin = std::io::stdin().lock();
    loop {
        print!("$ agent-browser ");
        std::io::stdout().flush()?;
        let mut line = Vec::new();
        stdin.read_until(b'\n', &mut line)?;
        let command = String::from_utf8_lossy(&line);
        let command = command.trim();
        if command.is_empty() {
            break;
        }
        println!(
            "{}\n",
            sh(&mut console, &format!("agent-browser --cdp 9222 {command}")).await?
        );
    }

    // Dropping the console says `quit`, and the server tears the session down.
    Ok(())
}
