//! The connected code a change links to and names: the repository files
//! that touched Markdown links to and that changed lines name, each sent
//! whole, merged with the callers and callees found in the same files.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use whetstone_core::{Change, Connection, ConnectionGroup, Extent, FileContent, Relation, Tier};

use super::lines::added_lines;
use super::links::linked_files;
use super::mentions::named_files;
use super::touched_text::{Target, TouchedText};
use super::{Failure, FileKind, Snapshot, read_file};

/// `callers_and_callees`, those of `change`'s callables, with the
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
    callers_and_callees: Vec<Connection>,
) -> Result<Vec<Connection>, Failure> {
    let files = snapshot
        .files()
        .map_err(|error| logged(Failure::Snapshot(Box::new(error))))?;
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
    let sent_whole: BTreeSet<&str> = linked.keys().chain(named.keys()).copied().collect();
    let (merging, mut connections): (Vec<Connection>, Vec<Connection>) =
        callers_and_callees.into_iter().partition(|connection| {
            connection.group() == ConnectionGroup::ConnectedCode
                && sent_whole.contains(connection.id().path())
        });
    tracing::info!(
        linked = linked.len(),
        named = sent_whole.len() - linked.len(),
        merged_ranges = merging.len(),
        "found the files a change links to and names"
    );
    for path in sent_whole {
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
        let kind = *kinds
            .get(path)
            .expect("a file linked to or named is in the commit's tree");
        connections.push(Connection::new(
            path.to_owned(),
            Extent::WholeFile,
            ConnectionGroup::ConnectedCode,
            tier,
            relations,
            whole_file(snapshot, kind, path).map_err(logged)?,
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
    kind: FileKind,
    path: &str,
) -> Result<FileContent, Failure> {
    match kind {
        FileKind::Symlink => Ok(FileContent::Symlink),
        FileKind::Submodule => Ok(FileContent::Submodule),
        FileKind::Blob | FileKind::Executable => read_file(snapshot, path),
    }
}

/// `failure`, once recorded in the log.
fn logged(failure: Failure) -> Failure {
    tracing::warn!(error = %failure, "cannot read the files a change links to and names");
    failure
}

#[cfg(test)]
mod tests {
    use whetstone_core::{CommitId, LineRange};

    use super::*;
    use crate::repository::test_repo::TestRepo;

    /// The connected code of the commit `id` of `repo`, given its
    /// `callers_and_callees`, each piece as `path extent tier: relations`.
    fn sent(repo: &TestRepo, id: CommitId, callers_and_callees: Vec<Connection>) -> Vec<String> {
        let repository = repo.open();
        let change = repository.load(id).expect("load the change");
        repository
            .handle()
            .with_tree(id, |tree| {
                with_linked_and_named(tree, &change, callers_and_callees)
            })
            .expect("read the commit's tree")
            .expect("read what the change links to and names")
            .iter()
            .map(|connection| {
                let tier = match connection.tier() {
                    Tier::High => "high",
                    Tier::Medium => "medium",
                };
                let relations: Vec<String> = connection
                    .relations()
                    .iter()
                    .map(ToString::to_string)
                    .collect();
                format!(
                    "{} {} {tier}: {}",
                    connection.id().path(),
                    connection.id().extent(),
                    relations.join(", ")
                )
            })
            .collect()
    }

    /// A caller of `charge` in `src/pay.rs`, at lines `first` to `last` of
    /// `path`, in `tier`.
    fn caller(path: &str, first: usize, last: usize, tier: Tier) -> Connection {
        Connection::new(
            path.to_owned(),
            Extent::Lines(LineRange::new(first, last).expect("a line range")),
            ConnectionGroup::ConnectedCode,
            tier,
            vec![Relation::Calls {
                callable: "charge".to_owned(),
                touched_file: "src/pay.rs".to_owned(),
            }],
            FileContent::Text("fn checkout() {}\n".repeat(8)),
        )
    }

    #[test]
    fn a_linked_file_is_in_the_high_tier_and_a_file_only_named_in_the_medium_tier() {
        let repo = TestRepo::new();
        repo.write("README.md", "# Shop\n")
            .write("docs/guide.md", "# Guide\n")
            .write("docs/faq.md", "# FAQ\n");
        repo.commit("Start the shop");
        repo.write(
            "README.md",
            "# Shop\n\nRead [the guide](docs/guide.md), then docs/faq.md.\n",
        );
        let id = repo.commit("Point to the docs");

        assert_eq!(
            sent(&repo, id, Vec::new()),
            [
                "docs/faq.md whole file medium: named in `README.md`",
                "docs/guide.md whole file high: linked from `README.md`",
            ]
        );
    }

    #[test]
    fn callers_in_a_file_sent_whole_merge_into_it_and_lift_it_to_their_tier() {
        let repo = TestRepo::new();
        repo.write("README.md", "# Shop\n")
            .write("src/cart.rs", "fn cart() {}\n")
            .write("src/check.rs", "fn check() {}\n")
            .write("src/shop.rs", "fn shop() {}\n");
        repo.commit("Start the shop");
        repo.write(
            "README.md",
            "# Shop\n\nCheckout is in src/cart.rs and src/shop.rs.\n",
        );
        let id = repo.commit("Say where checkout is");

        let found = sent(
            &repo,
            id,
            vec![
                caller("src/cart.rs", 1, 2, Tier::Medium),
                caller("src/check.rs", 1, 3, Tier::Medium),
                caller("src/shop.rs", 3, 5, Tier::Medium),
                caller("src/shop.rs", 7, 8, Tier::High),
            ],
        );

        assert_eq!(
            found,
            [
                "src/cart.rs whole file medium: calls `charge`, named in `README.md`",
                "src/check.rs lines 1–3 medium: calls `charge`",
                "src/shop.rs whole file high: calls `charge`, named in `README.md`",
            ]
        );
    }
}
