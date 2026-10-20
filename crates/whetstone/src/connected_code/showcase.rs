//! The showcase project's tree at one of its changes, as finding connected
//! code reads it: rebuilt by replaying the embedded changes up to that one,
//! so the showcase project's connected code is found at run time exactly as
//! a commit's is in the learner's repository, and nothing about it is
//! worked out when Whetstone is built.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the AI Practice spec's Preparing questions finds the showcase project's connected code"
    )
)]

use std::collections::BTreeMap;
use std::time::Instant;

use whetstone_core::{Change, Connections, ExcludePatterns, FileContent, ShowcaseProject};

use super::{
    AnalysisDirs, FileKind, GraphCache, GraphOrigin, Snapshot, SnapshotFile, SnapshotId,
    connections_at, state,
};

/// The showcase project's tree just after one of its changes, with that
/// change: a [`Snapshot`] of the showcase change, as a commit's tree is one
/// of the learner's commit. It owns what it holds, so a background job can
/// take it.
#[derive(Debug)]
pub(crate) struct ShowcaseTree {
    /// The change's position among the showcase project's changes.
    position: usize,
    change: Change,
    files: BTreeMap<String, FileContent>,
}

/// The showcase project keeps a binary file's size, never its contents, so
/// there is nothing to read.
#[derive(Debug, thiserror::Error)]
#[error("the showcase project keeps no contents for a binary file")]
pub(crate) struct BinaryFile;

impl ShowcaseTree {
    /// The tree of `showcase` at its change at `position`, as
    /// [`ShowcaseProject::files_at`] replays it, or `None` if there is no
    /// change there.
    pub(crate) fn at(showcase: &ShowcaseProject, position: usize) -> Option<Self> {
        let files = showcase.files_at(position)?;
        Some(Self {
            position,
            change: showcase.changes()[position].clone(),
            files: files
                .into_iter()
                .map(|(path, content)| (path.to_owned(), content.clone()))
                .collect(),
        })
    }

    /// Finds the change's connected code, blocking until it is found, as
    /// [`connections_at`] finds a learner's commit's. The showcase project's
    /// config sets nothing, so only the default exclusions apply. Its graph
    /// is cached under the change's position and this Whetstone version,
    /// which ships the showcase project.
    pub(crate) fn find(
        &self,
        dirs: &AnalysisDirs,
        cache: &GraphCache,
    ) -> (Connections, GraphOrigin) {
        let started = Instant::now();
        let (connections, origin) =
            connections_at(self, &self.change, &ExcludePatterns::default(), dirs, cache);
        tracing::info!(
            position = self.position,
            state = state(&connections),
            origin = ?origin,
            ms = started.elapsed().as_millis(),
            "found a showcase change's connected code"
        );
        (connections, origin)
    }
}

impl Snapshot for ShowcaseTree {
    type Error = BinaryFile;

    fn id(&self) -> SnapshotId {
        SnapshotId::Showcase(self.position)
    }

    fn files(&self) -> Result<Vec<SnapshotFile>, BinaryFile> {
        Ok(self
            .files
            .iter()
            .map(|(path, content)| {
                let kind = match content {
                    FileContent::Text(_) | FileContent::Binary { .. } => FileKind::Blob,
                    FileContent::Submodule => FileKind::Submodule,
                    FileContent::Symlink => FileKind::Symlink,
                };
                SnapshotFile::new(path.clone(), kind)
            })
            .collect())
    }

    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, BinaryFile> {
        match self.files.get(path) {
            Some(FileContent::Text(text)) => Ok(Some(text.as_bytes().to_vec())),
            Some(FileContent::Binary { .. }) => Err(BinaryFile),
            Some(FileContent::Submodule | FileContent::Symlink) | None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use whetstone_core::{Connection, ConnectionGroup, Relation};

    use super::*;
    use crate::connected_code::analysis::ENGINE_VERSION;

    /// The expected table's header, before the engine version and the
    /// changes it numbers.
    const TABLE_HEADER: &str = "\
# The connected code of every curated showcase change, as Whetstone finds it
# through the connected-code module, the same way as a learner's commit.
# Each line is one connection: change | path | extent | group | relations.
# A change with none has one \"none found\" line.
#
# Review this table whenever cgg is upgraded (ADR 0002): an upgrade can change
# what is found. The engine version below follows the cgg pin, so an upgrade
# fails this check until the table has been reviewed. A failing run writes the
# table it found beside this one, ending in `.snap.new`, to compare and accept.";

    /// The showcase project's expected connected code, kept with the tests:
    /// each curated change's connections, one per line, in change, path and
    /// extent order, after a header that numbers the changes as their
    /// patches in `showcase/payments-history/` are.
    #[test]
    fn every_curated_showcase_change_has_the_connected_code_its_table_expects() {
        let showcase = crate::embedded_showcase();
        let temp = TempDir::new().expect("create a temporary directory");
        let cache = TempDir::new().expect("create a cache folder");
        let cache = GraphCache::open(cache.path(), GraphCache::CAP);
        let dirs = AnalysisDirs::new(temp.path().to_path_buf());

        let mut table = vec![
            TABLE_HEADER.to_owned(),
            format!("# Found with cgg {ENGINE_VERSION}."),
            "#".to_owned(),
        ];
        for (position, change) in showcase.changes().iter().enumerate() {
            table.push(format!("# {:02} {}", position + 1, change.summary()));
        }
        table.push(String::new());
        for position in 0..showcase.changes().len() {
            let number = position + 1;
            let (connections, _) = ShowcaseTree::at(&showcase, position)
                .expect("the showcase has a change there")
                .find(&dirs, &cache);
            match connections {
                Connections::Found(mut connections) => {
                    connections.sort_by(|a, b| a.id().cmp(b.id()));
                    table.extend(connections.iter().map(|connection| row(number, connection)));
                }
                Connections::NoneFound => table.push(format!("{number:02} none found")),
                other => panic!("change {number} has no connected code: {other:?}"),
            }
        }

        insta::assert_snapshot!("showcase_connected_code", table.join("\n"));
    }

    /// `connection` of the change numbered `number` as a row of the table.
    fn row(number: usize, connection: &Connection) -> String {
        let group = match connection.group() {
            ConnectionGroup::ConnectedCode => "connected code",
            ConnectionGroup::RelatedTest => "related test",
            ConnectionGroup::RelatedConfiguration => "related configuration",
        };
        let relations: Vec<String> = connection
            .relations()
            .iter()
            .map(|relation| match relation {
                Relation::Calls { touched_file, .. } | Relation::CalledBy { touched_file, .. } => {
                    format!("{relation} in {touched_file}")
                }
                Relation::LinkedFrom { .. }
                | Relation::NamedIn { .. }
                | Relation::TestFor { .. }
                | Relation::ManifestFor { .. } => relation.to_string(),
            })
            .collect();
        format!(
            "{number:02} | {} | {} | {group} | {}",
            connection.id().path(),
            connection.id().extent(),
            relations.join("; ")
        )
    }

    #[test]
    fn a_showcase_change_found_again_comes_from_the_cache_and_another_change_does_not() {
        let showcase = crate::embedded_showcase();
        let temp = TempDir::new().expect("create a temporary directory");
        let cache = TempDir::new().expect("create a cache folder");
        let cache = GraphCache::open(cache.path(), GraphCache::CAP);
        let dirs = AnalysisDirs::new(temp.path().to_path_buf());
        let find = |position| {
            ShowcaseTree::at(&showcase, position)
                .expect("the showcase has a change there")
                .find(&dirs, &cache)
        };

        let (first, first_origin) = find(3);
        let (again, again_origin) = find(3);
        let (_, other_origin) = find(4);

        assert!(matches!(first, Connections::Found(_)), "{first:?}");
        assert_eq!(again, first);
        assert_eq!(
            [first_origin, again_origin, other_origin],
            [
                GraphOrigin::Analysed,
                GraphOrigin::Cached,
                GraphOrigin::Analysed
            ]
        );
    }
}
