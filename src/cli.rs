//! The `virtx` command line: build, list, inspect, remove and pull images from a shell.
//!
//! [`run`] is the whole of it, and the `virtx` binary, the Python console script and the Node
//! `bin` each hand it their arguments. Each command runs on the console server
//! [`ensure_virtx`](crate::ensure_virtx) fetches, over [`ImageClient`], and does what that
//! client's method does. The spelling follows `docker`'s, in both of its forms: `virtx image
//! <command>`, and the top-level `build`, `images`, `inspect`, `rmi` and `pull` most hands
//! know, which are the same commands.

use std::{
    ffi::OsString,
    fmt::Write as _,
    io::{IsTerminal as _, Read as _},
    path::{Path, PathBuf},
};

use crate::{
    ensure_virtx,
    image::{ImageClient, ImageEntry, ImageSource, Recipe, Step},
};
use anyhow::Context as _;
use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "virtx",
    version,
    about = "Build and manage images for virtx sessions"
)]
struct Cli {
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
enum ImageCommand {
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

#[derive(Debug, PartialEq, Args)]
struct BuildArgs {
    /// The ref to store the build under, as `name:tag`; the server picks one if absent.
    #[arg(short = 't', long = "tag", value_name = "NAME:TAG")]
    tag: Option<String>,

    /// The Dockerfile to build; `PATH/Dockerfile` if absent, stdin if `-`.
    #[arg(short = 'f', long = "file", value_name = "FILE")]
    file: Option<PathBuf>,

    /// The directory the Dockerfile is read from; `-` reads the Dockerfile from stdin.
    #[arg(default_value = ".", value_name = "PATH")]
    path: PathBuf,
}

#[derive(Debug, PartialEq, Args)]
struct LsArgs {
    /// Show each image's digest beside its ID.
    #[arg(long)]
    digests: bool,

    /// Only show image IDs, one per build; with --digests, the digests.
    #[arg(short, long)]
    quiet: bool,
}

#[derive(Debug, PartialEq, Args)]
struct InspectArgs {
    #[arg(required = true, value_name = "IMAGE")]
    images: Vec<String>,
}

#[derive(Debug, PartialEq, Args)]
struct RmArgs {
    #[arg(required = true, value_name = "IMAGE")]
    images: Vec<String>,

    /// Remove a build with several refs without asking.
    #[arg(short, long)]
    force: bool,
}

#[derive(Debug, PartialEq, Args)]
struct PullArgs {
    #[arg(value_name = "NAME[:TAG]")]
    image: String,
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
        ImageCommand::Build(BuildArgs { tag, file, path }) => {
            let (content, source) = match dockerfile(file.as_deref(), &path) {
                None => {
                    let mut text = String::new();
                    std::io::stdin()
                        .read_to_string(&mut text)
                        .context("reading the Dockerfile from stdin")?;
                    (text, "stdin".to_string())
                }
                Some(file) => (
                    std::fs::read_to_string(&file)
                        .with_context(|| format!("reading {}", file.display()))?,
                    file.display().to_string(),
                ),
            };
            let recipe =
                Recipe::from_dockerfile(content).with_context(|| format!("in {source}"))?;
            // `build_image` carries a recipe and a ref, no build context, so a COPY has
            // nothing to read from. Refused here, naming the file, rather than by the server.
            anyhow::ensure!(
                !recipe
                    .steps
                    .iter()
                    .any(|step| matches!(step, Step::Copy { .. })),
                "{source} has a COPY, and a build sends no context to copy from: only FROM, \
                 RUN, ENV and WORKDIR build"
            );

            let built = images.build(recipe, tag.as_deref()).await?;
            println!("{} {}", built.reference, built.digest);
        }
        ImageCommand::Ls(LsArgs { digests, quiet }) => {
            let listed = familiar(images.list().await?);
            if quiet {
                for image in &listed {
                    let id = if digests {
                        image.digest.as_str()
                    } else {
                        image_id(&image.digest)
                    };
                    println!("{id}");
                }
            } else {
                print!("{}", table(&listed, digests));
            }
        }
        ImageCommand::Inspect(InspectArgs { images: names }) => {
            let listed = familiar(images.list().await?);
            // Every image is looked up, as `docker inspect` does: the found ones print, each
            // missing one is named, and one missing fails the command.
            let mut failed = false;
            let mut found: Vec<&ImageEntry> = Vec::new();
            for name in &names {
                let image = resolve(name, &listed)
                    .map(|wanted| listed.iter().find(|image| names_image(&wanted, image)));
                match image {
                    Ok(Some(image)) => found.push(image),
                    Ok(None) => {
                        failed = true;
                        eprintln!("virtx: {name} is not a built image");
                    }
                    Err(error) => {
                        failed = true;
                        eprintln!("virtx: {name}: {}", report(&error));
                    }
                }
            }
            println!("{}", serde_json::to_string_pretty(&found)?);
            anyhow::ensure!(!failed, "not every image was found");
        }
        ImageCommand::Rm(RmArgs {
            images: names,
            force,
        }) => {
            let listed = familiar(images.list().await?);
            // Every image is tried, as `docker rmi` does; one refusal fails the command at
            // the end rather than leaving the rest unremoved. A declined one is not a
            // refusal.
            let mut failed = false;
            for name in names {
                let wanted = match resolve(&name, &listed) {
                    Ok(wanted) => wanted,
                    Err(error) => {
                        failed = true;
                        eprintln!("virtx: {name}: {}", report(&error));
                        continue;
                    }
                };
                let Some(image) = listed.iter().find(|image| names_image(&wanted, image)) else {
                    failed = true;
                    eprintln!("virtx: {name} is not a built image");
                    continue;
                };
                // A ref cannot be taken off a build on its own, so the build goes from
                // under every ref; said before it is done, with the one named in bold.
                if image.refs.len() > 1 && !force {
                    let id = image_id(&image.digest);
                    let by_id = matches!(wanted, ImageSource::Digest { .. });
                    eprintln!("{} has {} refs:", emphasis(id, by_id), image.refs.len());
                    for reference in &image.refs {
                        let named = matches!(&wanted, ImageSource::Ref { reference: given }
                            if canonical_ref(given) == canonical_ref(reference));
                        eprintln!("  {}", emphasis(reference, named));
                    }
                    if !confirm("remove the build and every ref?")? {
                        continue;
                    }
                }
                match images.remove(ImageSource::digest(&image.digest)).await {
                    Ok(()) => {
                        for reference in &image.refs {
                            println!("Untagged: {reference}");
                        }
                        println!("Deleted: {}", image_id(&image.digest));
                    }
                    Err(error) => {
                        failed = true;
                        eprintln!("virtx: {name}: {}", report(&error.into()));
                    }
                }
            }
            anyhow::ensure!(!failed, "not every image was removed");
        }
        ImageCommand::Pull(PullArgs { image }) => {
            let built = images.build(Recipe::new(&image), Some(&image)).await?;
            println!("{} {}", built.reference, built.digest);
        }
    }
    Ok(())
}

/// The error and its causes, as `cause: cause`.
///
/// A cause its parent already spells (a `Failure` names the server's error in its own
/// message and exposes it as its source) is skipped, so nothing is read twice.
fn report(error: &anyhow::Error) -> String {
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

/// A digest is `algorithm:hex`, as `build` prints one; anything else is a ref.
///
/// A ref is `name:tag`, which a digest also matches, so the algorithm is what tells them
/// apart: a tag of hex digits under a name that is not a digest algorithm stays a ref.
fn source(image: &str) -> ImageSource {
    match image.split_once(':') {
        Some((algorithm, hex))
            if matches!(algorithm, "sha256" | "sha512")
                && !hex.is_empty()
                && hex.chars().all(|c| c.is_ascii_hexdigit()) =>
        {
            ImageSource::digest(image)
        }
        _ => ImageSource::reference(image),
    }
}

/// Where the Dockerfile is: `file` if given, else `Dockerfile` under `path`; `None` is stdin,
/// which `-` names in either place, as `docker build -f -` and `docker build -` do.
fn dockerfile(file: Option<&Path>, path: &Path) -> Option<PathBuf> {
    let stdin = Path::new("-");
    match file {
        Some(file) if file == stdin => None,
        Some(file) => Some(file.to_path_buf()),
        None if path == stdin => None,
        None => Some(path.join("Dockerfile")),
    }
}

/// What `name` names among `listed`.
///
/// A hex string, with or without `sha256:`, that begins exactly one image's digest is that
/// image's ID, as `images` shows one; one that begins several is refused rather than guessed.
/// Anything else is the digest or ref as spelled.
fn resolve(name: &str, listed: &[ImageEntry]) -> anyhow::Result<ImageSource> {
    let prefix = name.strip_prefix("sha256:").unwrap_or(name);
    if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_hexdigit()) {
        let matched: Vec<&str> = listed
            .iter()
            .map(|image| image.digest.as_str())
            .filter(|digest| {
                digest
                    .strip_prefix("sha256:")
                    .is_some_and(|hex| hex.starts_with(prefix))
            })
            .collect();
        match matched.as_slice() {
            [digest] => return Ok(ImageSource::digest(*digest)),
            [] => {}
            _ => anyhow::bail!("{name} begins {} image IDs", matched.len()),
        }
    }
    Ok(source(name))
}

/// Whether `wanted` is `image`'s digest, or one of its refs in any spelling.
fn names_image(wanted: &ImageSource, image: &ImageEntry) -> bool {
    match wanted {
        ImageSource::Digest { digest } => image.digest == *digest,
        ImageSource::Ref { reference } => {
            let canonical = canonical_ref(reference);
            image
                .refs
                .iter()
                .any(|stored| stored == reference || canonical_ref(stored) == canonical)
        }
        ImageSource::Recipe { .. } => false,
    }
}

/// `text` in bold when `on`, for a terminal that shows it: not through a pipe, and not
/// under `NO_COLOR`, which tools take as "no styling".
fn emphasis(text: &str, on: bool) -> String {
    let shown = std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    if on && shown {
        format!("\x1b[1m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

/// Ask on stderr and read the answer from stdin: `y` is yes, and anything else, or no
/// answer at all, is no.
fn confirm(question: &str) -> anyhow::Result<bool> {
    eprint!("{question} [y/N] ");
    let mut answer = String::new();
    let read = std::io::stdin()
        .read_line(&mut answer)
        .context("reading an answer")?;
    // No answer leaves the prompt's line open, with no Enter to close it.
    if read == 0 {
        eprintln!();
    }
    Ok(matches!(answer.trim(), "y" | "Y" | "yes"))
}

/// The first 12 hex digits of a digest: what `docker images` shows, and enough to name one.
fn image_id(digest: &str) -> &str {
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
    &hex[..hex.len().min(12)]
}

/// Every ref as a user spells it, which is how `docker` shows the same names.
fn familiar(mut images: Vec<ImageEntry>) -> Vec<ImageEntry> {
    for image in &mut images {
        for reference in &mut image.refs {
            *reference = familiar_ref(reference);
        }
    }
    images
}

/// A ref without what [`canonical_ref`] fills in: `docker.io/library/alpine:3.20` is
/// `alpine:3.20`, and `docker.io/user/app:1` is `user/app:1`. The tag stays, so a ref is still
/// `name:tag`.
fn familiar_ref(reference: &str) -> String {
    let Some(rest) = reference.strip_prefix("docker.io/") else {
        return reference.to_string();
    };
    match rest.strip_prefix("library/") {
        Some(name) if !name.contains('/') => name.to_string(),
        _ => rest.to_string(),
    }
}

/// A ref as the server lists it: `registry/path:tag`, filled in as a registry client does.
///
/// A name without a tag is `latest`; without a registry it is on `docker.io`, where a lone
/// name is under `library/`. The first component is a registry when it has a `.` or a `:`
/// or is `localhost`, as `docker` tells `ghcr.io/x` from `user/x`.
fn canonical_ref(reference: &str) -> String {
    let (name, tag) = match reference.rsplit_once(':') {
        Some((name, tag)) if !tag.contains('/') => (name, tag),
        _ => (reference, "latest"),
    };
    let path = match name.split_once('/') {
        Some((registry, rest))
            if registry.contains('.') || registry.contains(':') || registry == "localhost" =>
        {
            if registry == "docker.io" && !rest.contains('/') {
                format!("docker.io/library/{rest}")
            } else {
                name.to_string()
            }
        }
        Some(_) => format!("docker.io/{name}"),
        None => format!("docker.io/library/{name}"),
    };
    format!("{path}:{tag}")
}

/// One line per ref, and one `<none>` line for a build no ref points at any more. The
/// digest column is there on request, as the ID names the image already.
fn table(images: &[ImageEntry], digests: bool) -> String {
    const NONE: &str = "<none>";
    let rows: Vec<(&str, &ImageEntry)> = images
        .iter()
        .flat_map(|image| {
            let refs: Vec<&str> = if image.refs.is_empty() {
                vec![NONE]
            } else {
                image.refs.iter().map(String::as_str).collect()
            };
            refs.into_iter().map(move |reference| (reference, image))
        })
        .collect();

    let width = rows
        .iter()
        .map(|(reference, _)| reference.len())
        .chain(std::iter::once("REF".len()))
        .max()
        .unwrap_or_default();

    let mut out = String::new();
    let heading = if digests {
        "IMAGE ID      DIGEST"
    } else {
        "IMAGE ID"
    };
    writeln!(out, "{:width$}  {heading}", "REF").unwrap();
    for (reference, image) in rows {
        let id = image_id(&image.digest);
        if digests {
            writeln!(out, "{reference:width$}  {id:12}  {}", image.digest).unwrap();
        } else {
            writeln!(out, "{reference:width$}  {id}").unwrap();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::Path;

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
    fn a_dash_reads_the_dockerfile_from_stdin() {
        assert_eq!(
            dockerfile(None, Path::new("app")),
            Some(PathBuf::from("app/Dockerfile"))
        );
        assert_eq!(
            dockerfile(Some(Path::new("ci/Dockerfile")), Path::new("app")),
            Some(PathBuf::from("ci/Dockerfile"))
        );
        assert_eq!(dockerfile(Some(Path::new("-")), Path::new("app")), None);
        assert_eq!(dockerfile(None, Path::new("-")), None);
        // A file named outright wins over a `-` path, as in docker.
        assert_eq!(
            dockerfile(Some(Path::new("ci/Dockerfile")), Path::new("-")),
            Some(PathBuf::from("ci/Dockerfile"))
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

    #[test]
    fn a_digest_is_told_from_a_ref_by_its_algorithm() {
        assert_eq!(
            source("sha256:0123abcd"),
            ImageSource::digest("sha256:0123abcd")
        );
        assert_eq!(source("sha512:ff"), ImageSource::digest("sha512:ff"));
        assert_eq!(source("jq:latest"), ImageSource::reference("jq:latest"));
        // Hex digits under a name that is not an algorithm, as a git-sha tag is.
        assert_eq!(
            source("app:3f2a1b4c"),
            ImageSource::reference("app:3f2a1b4c")
        );
        assert_eq!(source("sha256:"), ImageSource::reference("sha256:"));
        assert_eq!(
            source("sha256:latest"),
            ImageSource::reference("sha256:latest")
        );
        assert_eq!(source("alpine"), ImageSource::reference("alpine"));
    }

    #[test]
    fn a_ref_is_filled_in_as_the_server_stores_it() {
        assert_eq!(canonical_ref("alpine"), "docker.io/library/alpine:latest");
        assert_eq!(
            canonical_ref("alpine:3.20"),
            "docker.io/library/alpine:3.20"
        );
        assert_eq!(
            canonical_ref("docker.io/alpine:3.20"),
            "docker.io/library/alpine:3.20"
        );
        assert_eq!(canonical_ref("user/app:1"), "docker.io/user/app:1");
        assert_eq!(canonical_ref("ghcr.io/user/app"), "ghcr.io/user/app:latest");
        assert_eq!(
            canonical_ref("localhost:5000/app"),
            "localhost:5000/app:latest"
        );
        assert_eq!(
            canonical_ref("localhost:5000/app:1"),
            "localhost:5000/app:1"
        );
        assert_eq!(
            canonical_ref("docker.io/library/alpine:3.20"),
            "docker.io/library/alpine:3.20"
        );
    }

    #[test]
    fn a_ref_is_shown_as_a_user_spells_it() {
        assert_eq!(familiar_ref("docker.io/library/alpine:3.20"), "alpine:3.20");
        assert_eq!(familiar_ref("docker.io/user/app:1"), "user/app:1");
        assert_eq!(
            familiar_ref("docker.io/library/deep/app:1"),
            "library/deep/app:1"
        );
        assert_eq!(
            familiar_ref("ghcr.io/user/app:latest"),
            "ghcr.io/user/app:latest"
        );
        assert_eq!(familiar_ref("mine:1"), "mine:1");
        // Round trip, both ways.
        for reference in ["alpine:3.20", "user/app:1", "ghcr.io/user/app:latest"] {
            assert_eq!(familiar_ref(&canonical_ref(reference)), reference);
        }
        let stored = "docker.io/library/alpine:3.20";
        assert_eq!(canonical_ref(&familiar_ref(stored)), stored);
    }

    #[test]
    fn an_image_is_named_by_its_digest_or_any_spelling_of_a_ref() {
        for stored in ["docker.io/library/alpine:3.20", "alpine:3.20"] {
            let image = ImageEntry {
                digest: "sha256:aa".into(),
                refs: vec![stored.into()],
            };
            assert!(names_image(&ImageSource::digest("sha256:aa"), &image));
            assert!(names_image(&ImageSource::reference("alpine:3.20"), &image));
            assert!(names_image(
                &ImageSource::reference("docker.io/library/alpine:3.20"),
                &image
            ));
            assert!(!names_image(&ImageSource::reference("alpine"), &image));
            assert!(!names_image(&ImageSource::digest("sha256:bb"), &image));
        }
        // A ref the server stored as spelled, outside the canonical form.
        let odd = ImageEntry {
            digest: "sha256:cc".into(),
            refs: vec!["mine:1".into()],
        };
        assert!(names_image(&ImageSource::reference("mine:1"), &odd));
    }

    #[test]
    fn an_image_id_is_a_prefix_that_begins_one_digest() {
        let listed = vec![
            ImageEntry {
                digest: "sha256:45e09956dc667c5eff".into(),
                refs: vec!["alpine:3.20".into()],
            },
            ImageEntry {
                digest: "sha256:45e0aaaa".into(),
                refs: vec![],
            },
            ImageEntry {
                digest: "sha256:ab96ea8ff122".into(),
                refs: vec!["abc:1".into()],
            },
        ];
        let digest = |hex: &str| ImageSource::digest(format!("sha256:{hex}"));
        assert_eq!(resolve("ab96", &listed).unwrap(), digest("ab96ea8ff122"));
        assert_eq!(
            resolve("sha256:ab96", &listed).unwrap(),
            digest("ab96ea8ff122")
        );
        assert_eq!(
            resolve("45e09956dc66", &listed).unwrap(),
            digest("45e09956dc667c5eff")
        );
        // A full digest is itself.
        assert_eq!(
            resolve("sha256:45e0aaaa", &listed).unwrap(),
            digest("45e0aaaa")
        );
        // Begins two digests.
        assert!(resolve("45e0", &listed).is_err());
        // Hex, but begins none: a ref.
        assert_eq!(
            resolve("abc", &listed).unwrap(),
            ImageSource::reference("abc")
        );
        assert_eq!(
            resolve("abc:1", &listed).unwrap(),
            ImageSource::reference("abc:1")
        );
        assert_eq!(resolve("", &listed).unwrap(), ImageSource::reference(""));
    }

    #[test]
    fn an_image_id_is_the_first_twelve_hex_digits() {
        assert_eq!(image_id("sha256:45e09956dc667c5eff3583c9"), "45e09956dc66");
        assert_eq!(image_id("sha256:45e0"), "45e0");
        assert_eq!(image_id("45e09956dc667c5eff"), "45e09956dc66");
    }

    #[test]
    fn the_table_lists_every_ref_and_names_an_unreferenced_build() {
        let images = vec![
            ImageEntry {
                digest: "sha256:aaaaaaaaaaaaaaaa".into(),
                refs: vec!["jq:latest".into(), "jq:1".into()],
            },
            ImageEntry {
                digest: "sha256:bb".into(),
                refs: vec![],
            },
        ];
        assert_eq!(
            table(&images, false),
            "REF        IMAGE ID
\
             jq:latest  aaaaaaaaaaaa
\
             jq:1       aaaaaaaaaaaa
\
             <none>     bb
"
        );
        assert_eq!(
            table(&images, true),
            "REF        IMAGE ID      DIGEST
\
             jq:latest  aaaaaaaaaaaa  sha256:aaaaaaaaaaaaaaaa
\
             jq:1       aaaaaaaaaaaa  sha256:aaaaaaaaaaaaaaaa
\
             <none>     bb            sha256:bb
"
        );
        assert_eq!(
            table(&[], false),
            "REF  IMAGE ID
"
        );
    }
}
