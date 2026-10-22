//! `inspect`: images as JSON.

use clap::Args;

use super::{
    command::report,
    name::{familiar, names_image, resolve},
};
use crate::image::{ImageClient, ImageEntry};

#[derive(Debug, PartialEq, Args)]
pub(super) struct InspectArgs {
    #[arg(required = true, value_name = "IMAGE")]
    pub(super) images: Vec<String>,
}

/// Print the named images as JSON, and fail if one is not there.
pub(super) async fn inspect(
    images: &mut ImageClient,
    InspectArgs { images: names }: InspectArgs,
) -> anyhow::Result<()> {
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
    Ok(())
}
