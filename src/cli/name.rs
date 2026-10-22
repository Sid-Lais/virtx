//! How an argument names an image: a ref as the user or the server spells it, an image ID,
//! or a digest.

use crate::image::{ImageEntry, ImageSource};

/// A digest is `algorithm:hex`, as `build` prints one; anything else is a ref.
///
/// A ref is `name:tag`, which a digest also matches, so the algorithm is what tells them
/// apart: a tag of hex digits under a name that is not a digest algorithm stays a ref.
pub(super) fn source(image: &str) -> ImageSource {
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

/// What `name` names among `listed`.
///
/// A hex string, with or without `sha256:`, that begins exactly one image's digest is that
/// image's ID, as `images` shows one; one that begins several is refused rather than guessed.
/// Anything else is the digest or ref as spelled.
pub(super) fn resolve(name: &str, listed: &[ImageEntry]) -> anyhow::Result<ImageSource> {
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
pub(super) fn names_image(wanted: &ImageSource, image: &ImageEntry) -> bool {
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

/// The first 12 hex digits of a digest: what `docker images` shows, and enough to name one.
pub(super) fn image_id(digest: &str) -> &str {
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
    &hex[..hex.len().min(12)]
}

/// Every ref as a user spells it, which is how `docker` shows the same names.
pub(super) fn familiar(mut images: Vec<ImageEntry>) -> Vec<ImageEntry> {
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
pub(super) fn familiar_ref(reference: &str) -> String {
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
pub(super) fn canonical_ref(reference: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
