"""A browser in a console: Chromium in the VM, driven from the command line with `agent-browser`
and watched and used from your own browser through `viewer`, both in `/abin`.

    uv run python examples/browser.py

It opens example.com and follows its link, then runs the `agent-browser` commands you type
(`snapshot -i`, `click @e1`, `open <url>`, ...) until an empty line. All the while, the page it
prints shows the browser live and takes your mouse and keyboard.
"""

import asyncio
import sys

from virtx import ConsoleClient, Recipe, ensure_virtx

# The host port the viewer is published on.
VIEWER_PORT = 4849


async def sh(console: ConsoleClient, cmd: str) -> str:
    """Run `cmd` in the console, and return what it printed."""
    result = await console.exec(["sh", "-c", cmd])
    return (result.stdout + result.stderr).decode(errors="replace").rstrip()


async def read(prompt: str) -> str:
    """A line from the terminal, or an empty one once it closes."""
    try:
        return (await asyncio.to_thread(input, prompt)).strip()
    except EOFError:
        return ""


async def main() -> None:
    await ensure_virtx()
    console = await (
        ConsoleClient.builder()
        .image(
            Recipe("debian:trixie-slim").step(
                "apt-get update"
                " && apt-get install -y --no-install-recommends chromium fonts-noto-cjk ca-certificates"
                " && rm -rf /var/lib/apt/lists/* /var/log/journal"
            )
        )
        .network(True)
        .ports([f"{VIEWER_PORT}:4849"])
        .build()
    )

    # Leaving the block says `quit`, and the server tears the session down.
    async with console:
        # Headless: what you see is the viewer's, from the browser's DevTools port. Chromium logs
        # once that port is open.
        await sh(
            console,
            "setsid nohup chromium --headless --no-sandbox --user-data-dir=/root/chromium"
            " --remote-debugging-port=9222 about:blank >/tmp/chromium.log 2>&1 &"
            " until grep -q 'DevTools listening' /tmp/chromium.log; do sleep 0.2; done",
        )
        # The viewer lets in only a page that presents this token.
        token = await sh(console, "head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \\n'")
        await sh(console, f"setsid nohup viewer --listen 127.0.0.1:4849 --token {token} >/tmp/viewer.log 2>&1 &")
        print(f"Watch and use the browser at http://localhost:{VIEWER_PORT}/#t={token}\n")

        for command in ["open https://example.com", "get title", "snapshot -i", "click @e1", "get url"]:
            print(f"$ agent-browser {command}")
            print(await sh(console, f"agent-browser --cdp 9222 {command}"), end="\n\n")

        print("Type agent-browser commands, or an empty line to end.")
        # Bytes that are not UTF-8, as a broken IME composition can leave, become U+FFFD.
        sys.stdin.reconfigure(errors="replace")
        while command := await read("$ agent-browser "):
            print(await sh(console, f"agent-browser --cdp 9222 {command}"), end="\n\n")


if __name__ == "__main__":
    asyncio.run(main())
