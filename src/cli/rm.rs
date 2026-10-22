//! `rm`: builds removed, with every ref on them.

use std::io::IsTerminal as _;

use anyhow::Context as _;
use clap::Args;

use super::{
    command::report,
    name::{canonical_ref, familiar, image_id, names_image, resolve},
};
use crate::image::{ImageClient, ImageSource};

#[derive(Debug, PartialEq, Args)]
pub(super) struct RmArgs {
    #[arg(required = true, value_name = "IMAGE")]
    pub(super) images: Vec<String>,

    /// Remove a build with several refs without asking.
    #[arg(short, long)]
    pub(super) force: bool,
}

/// Remove the named builds, asking first for one that several refs share.
pub(super) async fn rm(
    images: &mut ImageClient,
    RmArgs {
        images: names,
        force,
    }: RmArgs,
) -> anyhow::Result<()> {
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
    Ok(())
}

/// Ask on stderr and read the answer from stdin: `y` is yes, and anything else, or no
/// answer at all, is no.
pub(super) fn confirm(question: &str) -> anyhow::Result<bool> {
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

/// `text` in bold when `on`, for a terminal that shows it: not through a pipe, and not
/// under `NO_COLOR`, which tools take as "no styling".
pub(super) fn emphasis(text: &str, on: bool) -> String {
    let shown = std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    if on && shown {
        format!("\x1b[1m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}
