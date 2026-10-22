//! `cliMain`: the `virtx` command line, for the package's `bin` (`npx @brekkylab/virtx`).

use napi::{Env, bindgen_prelude::PromiseRaw};
use napi_derive::napi;

use crate::console::promise;

/// Run the `virtx` command line on `args` (without the program name); settles with its exit
/// code. Output goes to this process's stdout and stderr.
#[napi(ts_return_type = "Promise<number>")]
pub fn cli_main(env: &Env, args: Vec<String>) -> napi::Result<PromiseRaw<'_, i32>> {
    promise(env, async move { Ok(virtx::cli::run(args).await) })
}
