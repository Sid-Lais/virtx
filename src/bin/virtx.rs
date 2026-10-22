//! The `virtx` binary: [`virtx::cli::run`] on this process's arguments.

use std::process::ExitCode;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let code = virtx::cli::run(std::env::args_os().skip(1)).await;
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
