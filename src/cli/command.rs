use std::ffi::OsString;

use clap::{Parser, Subcommand};

use super::{
    build::{self, BuildArgs},
    inspect::{self, InspectArgs},
    ls::{self, LsArgs},
    pull::{self, PullArgs},
    rm::{self, RmArgs},
};
use crate::{ensure_virtx, image::ImageClient};

#[derive(Debug, Parser)]
#[command(
    name = "virtx",
    version,
    about = "Build and manage images for virtx sessions"
)]
pub(super) struct Cli {
    #[command(subcommand)]
    command: Command,
}

// What each command does, said once for both spellings.
const BUILD: &str = "Build an image from a Dockerfile";
const LS: &str = "List built images";
const INSPECT: &str =
    "Print images as JSON, each by its ref (`name:tag`), image ID or digest (`sha256:hex`)";
const RM: &str = "Remove builds, each by a ref (`name:tag`), image ID or digest (`sha256:hex`). \
                  A build goes with every ref on it, so one with several asks first";
const PULL: &str =
    "Pull an image from a registry: a build with no steps, stored under the same name";

#[derive(Debug, Subcommand)]
enum Command {
    /// Manage images: the commands below, as `image <COMMAND>`.
    Image {
        #[command(subcommand)]
        command: ImageCommand,
    },

    #[command(about = BUILD)]
    Build(BuildArgs),

    #[command(about = LS)]
    Images(LsArgs),

    #[command(about = INSPECT)]
    Inspect(InspectArgs),

    #[command(about = RM)]
    Rmi(RmArgs),

    #[command(about = PULL)]
    Pull(PullArgs),
}

impl Command {
    fn into_image(self) -> ImageCommand {
        match self {
            Command::Image { command } => command,
            Command::Build(args) => ImageCommand::Build(args),
            Command::Images(args) => ImageCommand::Ls(args),
            Command::Inspect(args) => ImageCommand::Inspect(args),
            Command::Rmi(args) => ImageCommand::Rm(args),
            Command::Pull(args) => ImageCommand::Pull(args),
        }
    }
}

#[derive(Debug, PartialEq, Subcommand)]
pub(super) enum ImageCommand {
    #[command(about = BUILD)]
    Build(BuildArgs),

    #[command(about = LS)]
    Ls(LsArgs),

    #[command(about = INSPECT)]
    Inspect(InspectArgs),

    #[command(about = RM)]
    Rm(RmArgs),

    #[command(about = PULL)]
    Pull(PullArgs),
}

/// Run the command line `args` spell, without the program name, and return the exit code: 0
/// when done, 1 when a command failed, and clap's own for an argument it could not read or
/// a `--help`. Everything is said on stdout and stderr, as a shell tool does.
///
/// On unix this resets `SIGPIPE` to its default action first: Rust (and Python) start with
/// it ignored, so `virtx images | head` would fail on the write after `head` exits, where a
/// shell tool ends quietly.
pub async fn run(args: impl IntoIterator<Item = impl Into<OsString>>) -> i32 {
    #[cfg(unix)]
    // SAFETY: `signal` is process-wide and may be called from any thread at any time.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let argv = std::iter::once(OsString::from("virtx")).chain(args.into_iter().map(Into::into));
    let cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            return error.exit_code();
        }
    };
    match execute(cli).await {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("virtx: {}", report(&error));
            1
        }
    }
}

async fn execute(cli: Cli) -> anyhow::Result<()> {
    let server = ensure_virtx().await?.join("virtx-uvm");
    let mut images = ImageClient::try_from_cmd(&[&server]).await?;
    match cli.command.into_image() {
        ImageCommand::Build(args) => build::build(&mut images, args).await,
        ImageCommand::Ls(args) => ls::ls(&mut images, args).await,
        ImageCommand::Inspect(args) => inspect::inspect(&mut images, args).await,
        ImageCommand::Rm(args) => rm::rm(&mut images, args).await,
        ImageCommand::Pull(args) => pull::pull(&mut images, args).await,
    }
}

/// The error and its causes, as `cause: cause`.
///
/// A cause its parent already spells (a `Failure` names the server's error in its own
/// message and exposes it as its source) is skipped, so nothing is read twice.
pub(super) fn report(error: &anyhow::Error) -> String {
    let mut text = String::new();
    for cause in error.chain().map(ToString::to_string) {
        if text.ends_with(&cause) {
            continue;
        }
        if !text.is_empty() {
            text.push_str(": ");
        }
        text.push_str(&cause);
    }
    text
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    fn parse(args: &[&str]) -> ImageCommand {
        let mut argv = vec!["virtx"];
        argv.extend_from_slice(args);
        Cli::try_parse_from(argv).unwrap().command.into_image()
    }

    #[test]
    fn build_takes_tag_file_and_context() {
        let parsed = parse(&[
            "image",
            "build",
            "-t",
            "jq:latest",
            "-f",
            "ci/Dockerfile",
            "app",
        ]);
        assert_eq!(
            parsed,
            ImageCommand::Build(BuildArgs {
                tag: Some("jq:latest".into()),
                file: Some("ci/Dockerfile".into()),
                path: "app".into(),
            })
        );
    }

    #[test]
    fn build_defaults_to_the_working_directory() {
        assert_eq!(
            parse(&["image", "build"]),
            ImageCommand::Build(BuildArgs {
                tag: None,
                file: None,
                path: Path::new(".").into(),
            })
        );
    }

    #[test]
    fn ls_takes_digests_and_quiet() {
        assert_eq!(
            parse(&["image", "ls", "-q", "--digests"]),
            ImageCommand::Ls(LsArgs {
                digests: true,
                quiet: true
            })
        );
        assert_eq!(
            parse(&["image", "ls"]),
            ImageCommand::Ls(LsArgs {
                digests: false,
                quiet: false
            })
        );
    }

    #[test]
    fn rm_needs_at_least_one_image() {
        assert!(Cli::try_parse_from(["virtx", "image", "rm"]).is_err());
        assert_eq!(
            parse(&["image", "rm", "-f", "a:1", "sha256:ab"]),
            ImageCommand::Rm(RmArgs {
                images: vec!["a:1".into(), "sha256:ab".into()],
                force: true
            })
        );
    }

    #[test]
    fn both_spellings_are_the_same_command() {
        let pairs: [(&[&str], &[&str]); 5] = [
            (
                &["build", "-t", "a:1", "dir"],
                &["image", "build", "-t", "a:1", "dir"],
            ),
            (&["images", "-q"], &["image", "ls", "-q"]),
            (&["inspect", "a:1"], &["image", "inspect", "a:1"]),
            (&["rmi", "-f", "a:1"], &["image", "rm", "-f", "a:1"]),
            (&["pull", "alpine:3.20"], &["image", "pull", "alpine:3.20"]),
        ];
        for (alias, command) in pairs {
            assert_eq!(parse(alias), parse(command), "{alias:?}");
        }
    }

    #[test]
    fn a_report_spells_each_cause_once() {
        let refused = crate::protocol::Failure::Refused(crate::protocol::Error {
            code: -1,
            message: "no such image".into(),
            data: None,
        });
        let error = anyhow::Error::from(refused).context("removing jq:latest");
        assert_eq!(
            report(&error),
            "removing jq:latest: the console server refused: no such image (-1)"
        );

        let plain = anyhow::anyhow!("inner").context("outer");
        assert_eq!(report(&plain), "outer: inner");
    }
}
