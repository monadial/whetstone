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

    use super::*;

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
