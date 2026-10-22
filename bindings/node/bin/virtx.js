#!/usr/bin/env node
// The `virtx` command: what `npx @brekkylab/virtx` and a global install run.
const { cliMain } = require('../index.js')

cliMain(process.argv.slice(2)).then((code) => {
  process.exitCode = code
})
