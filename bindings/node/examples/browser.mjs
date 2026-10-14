// A browser in a console: Chromium in the VM, driven from the command line with `agent-browser`
// and watched and used from your own browser through `viewer`, both in `/abin`.
//
//     node examples/browser.mjs
//
// It opens example.com and follows its link, then runs the `agent-browser` commands you type
// (`snapshot -i`, `click @e1`, `open <url>`, ...) until an empty line. All the while, the page it
// prints shows the browser live and takes your mouse and keyboard.

import { createRequire } from 'node:module'
import { createInterface } from 'node:readline'

const { ConsoleClient, Recipe, ensureVirtx } = createRequire(import.meta.url)('../index.js')

// The host port the viewer is published on.
const VIEWER_PORT = 4849

/** Run `cmd` in the console, and return what it printed. */
const sh = async (console_, cmd) => {
  const result = await console_.exec(['sh', '-c', cmd])
  return Buffer.concat([result.stdout, result.stderr]).toString().trimEnd()
}

await ensureVirtx()
const console_ = await ConsoleClient.builder()
  .image(
    new Recipe('debian:trixie-slim').step(
      'apt-get update' +
        ' && apt-get install -y --no-install-recommends chromium fonts-noto-cjk ca-certificates' +
        ' && rm -rf /var/lib/apt/lists/* /var/log/journal',
    ),
  )
  .network(true)
  .ports([`${VIEWER_PORT}:4849`])
  .build()

try {
  // Headless: what you see is the viewer's, from the browser's DevTools port. Chromium logs
  // once that port is open.
  await sh(
    console_,
    'setsid nohup chromium --headless --no-sandbox --user-data-dir=/root/chromium' +
      ' --remote-debugging-port=9222 about:blank >/tmp/chromium.log 2>&1 &' +
      ' until grep -q "DevTools listening" /tmp/chromium.log; do sleep 0.2; done',
  )
  // The viewer lets in only a page that presents this token.
  const token = await sh(console_, "head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \\n'")
  await sh(console_, `setsid nohup viewer --listen 127.0.0.1:4849 --token ${token} >/tmp/viewer.log 2>&1 &`)
  console.log(`Watch and use the browser at http://localhost:${VIEWER_PORT}/#t=${token}\n`)

  for (const command of ['open https://example.com', 'get title', 'snapshot -i', 'click @e1', 'get url']) {
    console.log(`$ agent-browser ${command}`)
    console.log(`${await sh(console_, `agent-browser --cdp 9222 ${command}`)}\n`)
  }

  console.log('Type agent-browser commands, or an empty line to end.')
  const prompt = () => process.stdout.write('$ agent-browser ')
  prompt()
  for await (const line of createInterface({ input: process.stdin })) {
    const command = line.trim()
    if (!command) break
    console.log(`${await sh(console_, `agent-browser --cdp 9222 ${command}`)}\n`)
    prompt()
  }
} finally {
  // Closing says `quit`, and the server tears the session down.
  await console_.close()
}
