//! The connected code a change links to and names: the repository files
//! that touched Markdown links to and that changed lines name, each sent
//! whole, merged with the callers and callees found in the same files.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use whetstone_core::{Change, Connection, ConnectionGroup, Extent, FileContent, Relation, Tier};

use super::lines::added_lines;
use super::links::linked_files;
use super::mentions::named_files;
use super::touched_text::{Target, TouchedText};
use super::{Failure, FileKind, Snapshot};

/// `code`, the callers and callees of `change`'s callables, with the
/// repository files that `change` links to and names in the commit
/// `snapshot` shows, in path and line order.
///
/// Each file it links to or names is sent whole, with one relation for
/// each touched file that does. A file that a touched Markdown file links
/// to, as [`linked_files`] finds them, is in the high tier; one that is
/// only named, as [`named_files`] finds them, is in the medium tier. A
/// touched file that links to a file is not also said to name it. Callers
/// and callees in a file that is sent whole merge into it: their relations
/// come first, and it takes the higher of their tiers.
///
/// If the commit has more paths than one search for mentions can hold, it
/// logs that and carries on with the links. It fails if the commit's tree
/// or a file in it cannot be read.
pub(super) fn with_linked_and_named<S: Snapshot>(
    snapshot: &S,
    change: &Change,
    code: Vec<Connection>,
) -> Result<Vec<Connection>, Failure> {
    let files = snapshot
        .files()
        .map_err(|error| Failure::Snapshot(Box::new(error)))?;
    let kinds: HashMap<&str, FileKind> = files
        .iter()
        .map(|file| (file.path(), file.kind()))
        .collect();
    let paths: BTreeSet<String> = kinds.keys().map(|&path| path.to_owned()).collect();
    let touched: Vec<TouchedText<'_>> = change
        .files()
        .iter()
        .zip(change.file_diffs())
        .map(|(file, diff)| TouchedText::new(file, added_lines(diff)))
        .collect();
    let links = linked_files(&paths, &touched);
    let mentions = named_files(&paths, &touched).unwrap_or_else(|error| {
        tracing::warn!(error = %error, "cannot search a commit's paths for mentions");
        Vec::new()
    });
    let linked = by_path(&links);
    let named = by_path(&mentions);
    tracing::info!(
        linked = linked.len(),
        named = named.len(),
        "found the files a change links to and names"
    );
    let whole: BTreeSet<&str> = linked.keys().chain(named.keys()).copied().collect();
    let (merging, mut connections): (Vec<Connection>, Vec<Connection>) =
        code.into_iter().partition(|connection| {
            connection.group() == ConnectionGroup::ConnectedCode
                && whole.contains(connection.id().path())
        });
    for path in whole {
        let in_file: Vec<&Connection> = merging
            .iter()
            .filter(|connection| connection.id().path() == path)
            .collect();
        let linked_from = linked.get(path).copied().unwrap_or_default();
        let named_in = named.get(path).copied().unwrap_or_default();
        let mut relations: Vec<Relation> = Vec::new();
        for relation in in_file.iter().flat_map(|connection| connection.relations()) {
            if !relations.contains(relation) {
                relations.push(relation.clone());
            }
        }
        relations.extend(linked_from.iter().map(|touched_file| Relation::LinkedFrom {
            touched_file: touched_file.clone(),
        }));
        relations.extend(
            named_in
                .iter()
                .filter(|touched_file| !linked_from.contains(touched_file))
                .map(|touched_file| Relation::NamedIn {
                    touched_file: touched_file.clone(),
                }),
        );
        let tier = if !linked_from.is_empty()
            || in_file
                .iter()
                .any(|connection| connection.tier() == Tier::High)
        {
            Tier::High
        } else {
            Tier::Medium
        };
        connections.push(Connection::new(
            path.to_owned(),
            Extent::WholeFile,
            ConnectionGroup::ConnectedCode,
            tier,
            relations,
            whole_file(snapshot, kinds.get(path).copied(), path)?,
        ));
    }
    connections.sort_by(|a, b| a.id().cmp(b.id()));
    Ok(connections)
}

/// Each of `targets`' paths, with the touched files it was found in.
fn by_path(targets: &[Target]) -> BTreeMap<&str, &[String]> {
    targets
        .iter()
        .map(|target| (target.path.as_str(), target.found_in.as_slice()))
        .collect()
}

/// The whole of the file at `path` in `snapshot`, which is of `kind`. A
/// symlink or a submodule is never read.
fn whole_file<S: Snapshot>(
    snapshot: &S,
    kind: Option<FileKind>,
    path: &str,
) -> Result<FileContent, Failure> {
    match kind {
        Some(FileKind::Symlink) => Ok(FileContent::Symlink),
        Some(FileKind::Submodule) => Ok(FileContent::Submodule),
        Some(FileKind::Blob | FileKind::Executable) | None => match snapshot.read(path) {
            Ok(Some(bytes)) => Ok(FileContent::from_bytes(bytes)),
            Ok(None) => Err(Failure::NotInCommit),
            Err(error) => Err(Failure::Snapshot(Box::new(error))),
        },
    }
}
