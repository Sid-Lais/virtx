//! `build`: a Dockerfile into an image.

use std::{
    io::Read as _,
    path::{Path, PathBuf},
};

use anyhow::Context as _;
use clap::Args;

use crate::image::{ImageClient, Recipe, Step};

#[derive(Debug, PartialEq, Args)]
pub(super) struct BuildArgs {
    /// The ref to store the build under, as `name:tag`; the server picks one if absent.
    #[arg(short = 't', long = "tag", value_name = "NAME:TAG")]
    pub(super) tag: Option<String>,

    /// The Dockerfile to build; `PATH/Dockerfile` if absent, stdin if `-`.
    #[arg(short = 'f', long = "file", value_name = "FILE")]
    pub(super) file: Option<PathBuf>,

    /// The directory the Dockerfile is read from; `-` reads the Dockerfile from stdin.
    #[arg(default_value = ".", value_name = "PATH")]
    pub(super) path: PathBuf,
}

/// Build a Dockerfile, read from a file or stdin, and say where the build went.
pub(super) async fn build(
    images: &mut ImageClient,
    BuildArgs { tag, file, path }: BuildArgs,
) -> anyhow::Result<()> {
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
    let recipe = Recipe::from_dockerfile(content).with_context(|| format!("in {source}"))?;
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
    Ok(())
}

/// Where the Dockerfile is: `file` if given, else `Dockerfile` under `path`; `None` is stdin,
/// which `-` names in either place, as `docker build -f -` and `docker build -` do.
pub(super) fn dockerfile(file: Option<&Path>, path: &Path) -> Option<PathBuf> {
    let stdin = Path::new("-");
    match file {
        Some(file) if file == stdin => None,
        Some(file) => Some(file.to_path_buf()),
        None if path == stdin => None,
        None => Some(path.join("Dockerfile")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
