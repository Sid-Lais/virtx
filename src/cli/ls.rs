//! `ls`: every build, one line per ref.

use std::fmt::Write as _;

use clap::Args;

use super::name::{familiar, image_id};
use crate::image::{ImageClient, ImageEntry};

#[derive(Debug, PartialEq, Args)]
pub(super) struct LsArgs {
    /// Show each image's digest beside its ID.
    #[arg(long)]
    pub(super) digests: bool,

    /// Only show image IDs, one per build; with --digests, the digests.
    #[arg(short, long)]
    pub(super) quiet: bool,
}

/// List every build, as a table or as bare IDs.
pub(super) async fn ls(
    images: &mut ImageClient,
    LsArgs { digests, quiet }: LsArgs,
) -> anyhow::Result<()> {
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
    Ok(())
}

/// One line per ref, and one `<none>` line for a build no ref points at any more. The
/// digest column is there on request, as the ID names the image already.
pub(super) fn table(images: &[ImageEntry], digests: bool) -> String {
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
    use super::*;

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
