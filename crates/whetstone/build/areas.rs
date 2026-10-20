//! The build's checks on areas: Whetstone's default area files must be
//! valid, and every `Areas:` trailer in the showcase project's history must
//! name one of them by ID. `build.rs` fails the build when either fails, and
//! the integration tests include this file to cover both.

use whetstone_core::{Area, AreaId, InvalidAreaFile};

/// An `Areas:` trailer names something that is not a default area's ID.
#[derive(Debug, thiserror::Error)]
#[error(
    "the `Areas:` trailer names {named:?}, which is not the ID of a default area: {}",
    ids(defaults)
)]
pub(crate) struct UnknownTrailerArea {
    /// What the trailer names.
    named: String,
    /// The default areas' IDs.
    defaults: Vec<AreaId>,
}

/// The IDs of the areas in Whetstone's default area `files`, each a file
/// name and its contents, in file-name order, or the first file that is not
/// a valid area file. `whetstone areas init` copies these files into a
/// repository, so each must load as that repository's area file would.
pub(crate) fn default_ids(files: &[(&str, &[u8])]) -> Result<Vec<AreaId>, InvalidAreaFile> {
    Area::offered(
        files
            .iter()
            .map(|&(file_name, contents)| (file_name, Ok(contents))),
    )
    .into_iter()
    .map(|area| area.map(|area| area.id().clone()))
    .collect()
}

/// Fails unless every area an `Areas:` trailer `named` is the ID of one of
/// the `defaults`.
pub(crate) fn check_trailer(named: &[&str], defaults: &[AreaId]) -> Result<(), UnknownTrailerArea> {
    match named.iter().find(|area| {
        !area
            .parse::<AreaId>()
            .is_ok_and(|id| defaults.contains(&id))
    }) {
        Some(area) => Err(UnknownTrailerArea {
            named: (*area).to_owned(),
            defaults: defaults.to_vec(),
        }),
        None => Ok(()),
    }
}

/// `ids`, separated by commas.
fn ids(ids: &[AreaId]) -> String {
    ids.iter()
        .map(AreaId::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}
