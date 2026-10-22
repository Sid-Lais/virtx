//! `pull`: a base from a registry, as a build with no steps.

use clap::Args;

use crate::image::{ImageClient, Recipe};

#[derive(Debug, PartialEq, Args)]
pub(super) struct PullArgs {
    #[arg(value_name = "NAME[:TAG]")]
    pub(super) image: String,
}

/// Pull a base from a registry, stored under the name given.
pub(super) async fn pull(
    images: &mut ImageClient,
    PullArgs { image }: PullArgs,
) -> anyhow::Result<()> {
    let built = images.build(Recipe::new(&image), Some(&image)).await?;
    println!("{} {}", built.reference, built.digest);
    Ok(())
}
