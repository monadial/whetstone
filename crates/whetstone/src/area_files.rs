//! Each area file's version, for provenance: the area it defines, where the
//! file comes from, and a SHA-256 hash of its content exactly as read, in
//! lowercase hexadecimal, as `shasum -a 256` prints it. `Area` keeps no
//! file content, so the hash is taken where Whetstone reads area files:
//! its defaults, embedded at build time, and the repository's, read once at
//! startup.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};
use whetstone_core::{Area, AreaFileVersion, AreaId, AreaSource, ContentHash};

/// The SHA-256 hash of an area file's `contents`.
pub(crate) fn content_hash(contents: &[u8]) -> ContentHash {
    let hex = Sha256::digest(contents)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            write!(hex, "{byte:02x}").expect("writing to a String never fails");
            hex
        });
    ContentHash::new(hex).expect("a SHA-256 digest in lowercase hex is a content hash")
}

/// The version of the area file called `file_name`, holding `contents`,
/// from `source`, if the name is an area ID followed by `.md`.
pub(crate) fn version(
    file_name: &str,
    contents: &[u8],
    source: AreaSource,
) -> Option<AreaFileVersion> {
    let area: AreaId = file_name.strip_suffix(".md")?.parse().ok()?;
    Some(AreaFileVersion::new(area, source, content_hash(contents)))
}

/// The versions of the files that define the chosen `areas`, in their
/// order: an area from the repository from `repository`, the versions of
/// the repository's area files, and a default from Whetstone's own.
pub(crate) fn of(areas: &[Area], repository: &[AreaFileVersion]) -> Vec<AreaFileVersion> {
    let defaults: Vec<AreaFileVersion> = Area::default_files()
        .iter()
        .filter_map(|&(file_name, contents)| version(file_name, contents, AreaSource::Default))
        .collect();
    areas
        .iter()
        .filter_map(|area| {
            let files = match area.source() {
                AreaSource::Repository => repository,
                AreaSource::Default => &defaults,
            };
            files
                .iter()
                .find(|file| file.area() == area.id() && file.source() == area.source())
                .cloned()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_files_hash_is_its_sha_256_in_lowercase_hex() {
        assert_eq!(
            content_hash(b"abc").as_str(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_default_area_has_the_version_of_whetstones_own_file() {
        let areas = Area::defaults();
        let (file_name, contents) = Area::default_files()[0];

        let versions = of(&areas[..1], &[]);

        assert_eq!(
            versions,
            [AreaFileVersion::new(
                areas[0].id().clone(),
                AreaSource::Default,
                content_hash(contents),
            )],
            "{file_name}"
        );
    }

    #[test]
    fn an_area_from_the_repository_has_the_version_of_the_repositorys_file() {
        let security = "+++\nname = \"Security\"\ndescription = \"Who can do what\"\n+++\nLook.\n";
        let area = Area::offered([("security.md", Ok(security.as_bytes()))])
            .remove(0)
            .expect("a valid area file");
        let repository = [
            version("security.md", security.as_bytes(), AreaSource::Repository)
                .expect("an area file name"),
        ];

        assert_eq!(of(&[area], &repository), repository);
    }

    #[test]
    fn a_file_name_that_is_not_an_area_id_has_no_version() {
        assert_eq!(version("Security.md", b"", AreaSource::Repository), None);
        assert_eq!(version("security.txt", b"", AreaSource::Repository), None);
    }
}
