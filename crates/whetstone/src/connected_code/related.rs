//! Related tests and related configuration: the files at a change's commit
//! that conventionally test or build its touched files, found from the
//! commit's paths alone, then sent whole as [`connections`] read from it.
//!
//! The conventions are data, not code: `conventions.toml`, embedded, says
//! which files are which language ecosystem's, which manifests build them
//! and where their tests are. This module applies them, so adding a
//! language means editing only that file.

mod conventions;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::iter;

use whetstone_core::{
    Change, Connection, ConnectionGroup, Extent, FileContent, Relation, Tier, TouchedFile,
};

use super::{Failure, FileKind, Snapshot, read_file};

use conventions::{Conventions, Ecosystem, Location};

/// The related tests and related configuration of `change` at the commit
/// `snapshot` shows, as [`find`] finds them: each a whole-file connection
/// in the high tier, in the related test or related configuration group,
/// there as the test or the manifest for each touched file it relates to,
/// with its file's content at the commit. A symbolic link or a submodule is
/// never read. It fails if the commit's tree or one of the files cannot be
/// read.
pub(super) fn connections<S: Snapshot>(
    snapshot: &S,
    change: &Change,
) -> Result<Vec<Connection>, Failure> {
    let files = snapshot
        .files()
        .map_err(|error| Failure::Snapshot(Box::new(error)))?;
    let kinds: HashMap<&str, FileKind> = files
        .iter()
        .map(|file| (file.path(), file.kind()))
        .collect();
    let related = find(kinds.keys().copied(), change.files());
    tracing::info!(
        tests = related
            .iter()
            .filter(|file| file.group() == RelatedGroup::Test)
            .count(),
        manifests = related
            .iter()
            .filter(|file| file.group() == RelatedGroup::Configuration)
            .count(),
        "found a change's related tests and configuration"
    );
    related
        .into_iter()
        .map(|file| {
            let content = match kinds.get(file.path()) {
                Some(FileKind::Symlink) => FileContent::Symlink,
                Some(FileKind::Submodule) => FileContent::Submodule,
                Some(FileKind::Blob | FileKind::Executable) | None => {
                    read_file(snapshot, file.path())?
                }
            };
            let group = file.group();
            Ok(Connection::new(
                file.path().to_owned(),
                Extent::WholeFile,
                group.connection_group(),
                Tier::High,
                file.related_to()
                    .iter()
                    .map(|touched_file| group.relation(touched_file.clone()))
                    .collect(),
                content,
            ))
        })
        .collect::<Result<_, _>>()
        .inspect_err(|failure| {
            tracing::warn!(error = %failure, "cannot read a change's related tests and configuration");
        })
}

/// A file at the commit that tests or builds one or more touched files.
/// It is sent whole, in the high tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RelatedFile {
    path: String,
    group: RelatedGroup,
    related_to: Vec<String>,
}

impl RelatedFile {
    /// Where the file is, relative to the repository's root.
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    /// Whether it is a related test or related configuration.
    pub(crate) fn group(&self) -> RelatedGroup {
        self.group
    }

    /// The paths of the touched files it relates to, in path order: it is a
    /// test for each, or the manifest for each, by its group.
    pub(crate) fn related_to(&self) -> &[String] {
        &self.related_to
    }
}

/// Why a [`RelatedFile`] is related to the touched files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RelatedGroup {
    /// A related test: by its ecosystem's test rules, it is a test for each
    /// touched file it relates to.
    Test,
    /// Related configuration: it is the nearest build manifest for each
    /// touched file it relates to.
    Configuration,
}

impl RelatedGroup {
    /// The evidence's group for it.
    fn connection_group(self) -> ConnectionGroup {
        match self {
            Self::Test => ConnectionGroup::RelatedTest,
            Self::Configuration => ConnectionGroup::RelatedConfiguration,
        }
    }

    /// Why a file in it is related to `touched_file`: it is its test, or
    /// its manifest.
    fn relation(self, touched_file: String) -> Relation {
        match self {
            Self::Test => Relation::TestFor { touched_file },
            Self::Configuration => Relation::ManifestFor { touched_file },
        }
    }
}

/// Finds the related tests and related configuration for a change's
/// `touched` files, from every file path in the `tree` at the change's
/// commit, by the conventions Whetstone embeds, which `conventions.toml`
/// sets out by language ecosystem. Paths are relative to the repository's
/// root and `/`-separated.
///
/// - **Related tests:** a touched file's tests by the test rules of the
///   ecosystem its extension picks. A touched file that is itself a test,
///   by the same ecosystem's conventions, has none.
/// - **Related configuration:** a touched file's nearest build manifest,
///   walking up to the repository's root. Of several manifests in one
///   directory, one of the touched file's own ecosystem comes first, then
///   the first in the conventions' order.
/// - A file related to several touched files is one entry that relates to
///   all of them. The entries are in path order.
/// - A touched file is never related: it is in the evidence already. A
///   touched manifest is still the nearest, so none further up replaces it.
/// - A deleted touched file has neither: it is not at the commit, so nothing
///   there tests or builds it, as a deleted function has no connected code.
pub(crate) fn find<'a>(
    tree: impl IntoIterator<Item = &'a str>,
    touched: &[TouchedFile],
) -> Vec<RelatedFile> {
    find_with(Conventions::embedded(), tree, touched)
}

/// [`find`], with `conventions` instead of the embedded ones.
fn find_with<'a>(
    conventions: &Conventions,
    tree: impl IntoIterator<Item = &'a str>,
    touched: &[TouchedFile],
) -> Vec<RelatedFile> {
    let tree = Tree::new(tree);
    let mut found: BTreeMap<(String, RelatedGroup), BTreeSet<&str>> = BTreeMap::new();
    for file in touched.iter().filter(|file| file.after().is_some()) {
        let path = file.path();
        let source = Source::parse(path, conventions);
        for test in tree.tests_for(&source) {
            found
                .entry((test, RelatedGroup::Test))
                .or_default()
                .insert(path);
        }
        if let Some(manifest) = tree.nearest_manifest(conventions, &source) {
            found
                .entry((manifest, RelatedGroup::Configuration))
                .or_default()
                .insert(path);
        }
    }
    let touched: BTreeSet<&str> = touched.iter().map(TouchedFile::path).collect();
    found
        .into_iter()
        .filter(|((path, _), _)| !touched.contains(path.as_str()))
        .map(|((path, group), related_to)| RelatedFile {
            path,
            group,
            related_to: related_to.into_iter().map(str::to_owned).collect(),
        })
        .collect()
}

/// A touched file's path, taken apart for matching conventions.
#[derive(Debug)]
struct Source<'a> {
    /// The directory it is in, `""` at the repository's root.
    dir: &'a str,
    /// Its file name without the last extension.
    stem: &'a str,
    /// The ecosystem its extension picks, if any.
    ecosystem: Option<&'a Ecosystem>,
}

impl<'a> Source<'a> {
    /// The touched file at `path`, taken apart, with the ecosystem its
    /// extension picks in `conventions`.
    fn parse(path: &'a str, conventions: &'a Conventions) -> Self {
        let (dir, name) = split(path);
        let (stem, ecosystem) = match name.rsplit_once('.') {
            // A leading dot starts a hidden file's name, not an extension.
            Some((stem, extension)) if !stem.is_empty() => {
                (stem, conventions.by_extension(extension))
            }
            _ => (name, None),
        };
        Self {
            dir,
            stem,
            ecosystem,
        }
    }
}

/// The file paths in the tree at the change's commit.
#[derive(Debug)]
struct Tree<'a> {
    /// Every path, in order.
    paths: BTreeSet<&'a str>,
    /// The same paths, by file name.
    by_name: HashMap<&'a str, Vec<&'a str>>,
}

impl<'a> Tree<'a> {
    /// The tree of `paths`.
    fn new(paths: impl IntoIterator<Item = &'a str>) -> Self {
        let paths: BTreeSet<&str> = paths.into_iter().collect();
        let mut by_name: HashMap<&str, Vec<&str>> = HashMap::new();
        for path in &paths {
            by_name.entry(split(path).1).or_default().push(path);
        }
        Self { paths, by_name }
    }

    /// The file `name` in `dir`, if the tree has it.
    fn file(&self, dir: &str, name: &str) -> Option<String> {
        let path = join(dir, name);
        self.paths.contains(path.as_str()).then_some(path)
    }

    /// The manifest in the nearest directory to `source` that has one of
    /// the manifests in `conventions`, walking up to the repository's root.
    /// Of several in that directory, one of the source's own ecosystem comes
    /// first, then the first in the conventions' order.
    fn nearest_manifest(&self, conventions: &Conventions, source: &Source<'_>) -> Option<String> {
        ancestors(source.dir).find_map(|dir| {
            conventions
                .manifests()
                .filter_map(|name| Some((self.file(dir, name)?, name)))
                // The first of the smallest keys wins: the ecosystem's own,
                // else the first listed.
                .min_by_key(|&(_, name)| {
                    !source
                        .ecosystem
                        .is_some_and(|ecosystem| ecosystem.builds_with(name))
                })
                .map(|(manifest, _)| manifest)
        })
    }

    /// The related tests for `source`, by its ecosystem's test rules. A
    /// source that is itself a test, by the same ecosystem, has none.
    fn tests_for(&self, source: &Source<'_>) -> Vec<String> {
        let Some(ecosystem) = source.ecosystem else {
            return Vec::new();
        };
        if self.is_test(ecosystem, source) {
            return Vec::new();
        }
        ecosystem
            .tests
            .iter()
            .flat_map(|rule| {
                rule.names
                    .iter()
                    .flat_map(|name| name.expand(source.stem, &ecosystem.extensions))
                    .flat_map(|name| self.files_in(&rule.look_in, ecosystem, source.dir, &name))
            })
            .collect()
    }

    /// Whether `source`, a file of `ecosystem`, is a test itself.
    fn is_test(&self, ecosystem: &Ecosystem, source: &Source<'_>) -> bool {
        let Source { dir, stem, .. } = *source;
        let is_test = &ecosystem.is_test;
        let beside_manifest = || {
            self.package(ecosystem, dir).is_some_and(|package| {
                is_test.under_dir_beside_manifest.iter().any(|tests| {
                    let tests_dir = join(package, tests);
                    ancestors(dir).any(|ancestor| ancestor == tests_dir)
                })
            })
        };
        is_test
            .stem_starts_with
            .iter()
            .any(|start| stem.starts_with(start.as_str()))
            || is_test
                .stem_ends_with
                .iter()
                .any(|end| stem.ends_with(end.as_str()))
            || ancestors(dir).any(|ancestor| {
                is_test
                    .under_dir
                    .iter()
                    .any(|name| name == split(ancestor).1)
            })
            || beside_manifest()
    }

    /// The files called `name` in `location`, for a file of `ecosystem` in
    /// `dir`.
    fn files_in(
        &self,
        location: &Location,
        ecosystem: &Ecosystem,
        dir: &str,
        name: &str,
    ) -> Vec<String> {
        let in_dir = match location {
            Location::SameDir => Some(dir.to_owned()),
            Location::Subdir(subdir) => Some(join(dir, subdir)),
            Location::BesideManifest(beside) => self
                .package(ecosystem, dir)
                .map(|package| join(package, beside)),
            Location::UnderAncestorDir(test_dirs) => {
                return self.under_test_dirs(dir, name, test_dirs);
            }
            Location::Mirror { from, to } => mirror(dir, from, to),
        };
        in_dir
            .and_then(|in_dir| self.file(&in_dir, name))
            .into_iter()
            .collect()
    }

    /// The nearest directory to `dir`, walking up to the repository's root,
    /// that has one of `ecosystem`'s manifests.
    fn package<'d>(&self, ecosystem: &Ecosystem, dir: &'d str) -> Option<&'d str> {
        ancestors(dir).find(|package| {
            ecosystem
                .manifests
                .iter()
                .any(|manifest| self.file(package, manifest).is_some())
        })
    }

    /// The files called `name` anywhere under a directory called one of
    /// `test_dirs` that is in `dir` or in a directory above it.
    fn under_test_dirs(&self, dir: &str, name: &str, test_dirs: &[String]) -> Vec<String> {
        self.by_name
            .get(name)
            .into_iter()
            .flatten()
            .filter(|path| {
                ancestors(split(path).0).any(|found_in| {
                    let (parent, dir_name) = split(found_in);
                    test_dirs.iter().any(|test_dir| test_dir == dir_name)
                        && ancestors(dir).any(|ancestor| ancestor == parent)
                })
            })
            .map(|path| (*path).to_owned())
            .collect()
    }
}

/// `dir` with the last `from` in its path replaced by `to`, if it has one.
/// Both are `/`-separated and matched by whole segments.
fn mirror(dir: &str, from: &str, to: &str) -> Option<String> {
    let parts: Vec<&str> = dir.split('/').collect();
    let from: Vec<&str> = from.split('/').collect();
    let at = parts
        .windows(from.len())
        .rposition(|window| window == from.as_slice())?;
    let (before, after) = parts.split_at(at);
    Some(
        before
            .iter()
            .copied()
            .chain(to.split('/'))
            .chain(after.iter().skip(from.len()).copied())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// `dir`, then each directory above it, up to the repository's root, `""`.
fn ancestors(dir: &str) -> impl Iterator<Item = &str> {
    iter::successors(Some(dir), |dir| {
        (!dir.is_empty()).then(|| dir.rsplit_once('/').map_or("", |(parent, _)| parent))
    })
}

/// A path's directory, `""` at the repository's root, and its file name.
fn split(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

/// The path of `name` in `dir`.
fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

#[cfg(test)]
mod tests {
    use whetstone_core::FileContent;

    use super::*;

    /// Touched files the change creates or edits, so each is at the commit.
    fn changed(paths: &[&str]) -> Vec<TouchedFile> {
        paths
            .iter()
            .map(|path| TouchedFile::created((*path).to_owned(), FileContent::Text(String::new())))
            .collect()
    }

    /// Everything `find` reports, as (path, group, related touched files).
    fn found(tree: &[&str], touched: &[TouchedFile]) -> Vec<(String, RelatedGroup, Vec<String>)> {
        find(tree.iter().copied(), touched)
            .iter()
            .map(|file| {
                (
                    file.path().to_owned(),
                    file.group(),
                    file.related_to().to_vec(),
                )
            })
            .collect()
    }

    /// The related tests `find` reports, as (path, related touched files).
    fn tests_found(tree: &[&str], touched: &[TouchedFile]) -> Vec<(String, Vec<String>)> {
        in_group(found(tree, touched), RelatedGroup::Test)
    }

    /// The related configuration `find` reports, as (path, related touched
    /// files).
    fn manifests_found(tree: &[&str], touched: &[TouchedFile]) -> Vec<(String, Vec<String>)> {
        in_group(found(tree, touched), RelatedGroup::Configuration)
    }

    /// The entries of `found` in the group `wanted`, without their group.
    fn in_group(
        found: Vec<(String, RelatedGroup, Vec<String>)>,
        wanted: RelatedGroup,
    ) -> Vec<(String, Vec<String>)> {
        found
            .into_iter()
            .filter(|(_, group, _)| *group == wanted)
            .map(|(path, _, related_to)| (path, related_to))
            .collect()
    }

    /// A related file and the touched files it relates to.
    fn related(path: &str, touched: &[&str]) -> (String, Vec<String>) {
        (
            path.to_owned(),
            touched.iter().map(|path| (*path).to_owned()).collect(),
        )
    }

    #[test]
    fn a_go_file_is_tested_by_its_test_file_in_the_same_directory() {
        let tree = ["store/store.go", "store/store_test.go", "store_test.go"];

        assert_eq!(
            tests_found(&tree, &changed(&["store/store.go"])),
            [related("store/store_test.go", &["store/store.go"])]
        );
    }

    #[test]
    fn a_rust_file_is_tested_by_the_test_of_its_name_beside_the_nearest_cargo_toml() {
        let tree = [
            "Cargo.toml",
            "tests/store.rs",
            "crates/shop/Cargo.toml",
            "crates/shop/src/store.rs",
            "crates/shop/src/tests/store.rs",
            "crates/shop/tests/store.rs",
        ];

        assert_eq!(
            tests_found(&tree, &changed(&["crates/shop/src/store.rs"])),
            [related(
                "crates/shop/tests/store.rs",
                &["crates/shop/src/store.rs"]
            )]
        );
    }

    #[test]
    fn a_python_file_is_tested_by_either_test_name_in_the_same_directory() {
        let tree = [
            "shop/store.py",
            "shop/test_store.py",
            "shop/store_test.py",
            "shop/teststore.py",
        ];

        assert_eq!(
            tests_found(&tree, &changed(&["shop/store.py"])),
            [
                related("shop/store_test.py", &["shop/store.py"]),
                related("shop/test_store.py", &["shop/store.py"]),
            ]
        );
    }

    #[test]
    fn a_python_file_is_tested_by_a_test_under_a_tests_or_test_directory_of_an_ancestor() {
        let tree = [
            "src/shop/store.py",
            "tests/unit/test_store.py",
            "src/test/store_test.py",
            "src/shop/tests/test_store.py",
            "other/tests/test_store.py",
            "src/shop/fixtures/test_store.py",
        ];

        assert_eq!(
            tests_found(&tree, &changed(&["src/shop/store.py"])),
            [
                related("src/shop/tests/test_store.py", &["src/shop/store.py"]),
                related("src/test/store_test.py", &["src/shop/store.py"]),
                related("tests/unit/test_store.py", &["src/shop/store.py"]),
            ]
        );
    }

    #[test]
    fn a_script_is_tested_by_a_test_or_spec_beside_it_or_in_its_tests_directory() {
        let tree = [
            "web/cart.ts",
            "web/cart.test.ts",
            "web/cart.spec.js",
            "web/__tests__/cart.tsx",
            "web/__tests__/cart.json",
            "web/cart.stories.ts",
            "lib/cart.test.ts",
            "__tests__/cart.ts",
        ];

        assert_eq!(
            tests_found(&tree, &changed(&["web/cart.ts"])),
            [
                related("web/__tests__/cart.tsx", &["web/cart.ts"]),
                related("web/cart.spec.js", &["web/cart.ts"]),
                related("web/cart.test.ts", &["web/cart.ts"]),
            ]
        );
    }

    #[test]
    fn a_java_class_is_tested_by_a_test_class_at_the_matching_src_test_path() {
        let tree = [
            "shop/src/main/java/com/acme/Store.java",
            "shop/src/test/java/com/acme/StoreTest.java",
            "shop/src/test/java/com/acme/StoreTests.java",
            "shop/src/test/java/com/acme/TestStore.java",
            "shop/src/test/java/com/acme/StoreIT.java",
            "shop/src/test/java/com/acme/StoreSpec.java",
            "shop/src/test/java/com/other/StoreTest.java",
            "src/test/java/com/acme/StoreTest.java",
        ];

        assert_eq!(
            tests_found(&tree, &changed(&["shop/src/main/java/com/acme/Store.java"])),
            [
                related(
                    "shop/src/test/java/com/acme/StoreIT.java",
                    &["shop/src/main/java/com/acme/Store.java"]
                ),
                related(
                    "shop/src/test/java/com/acme/StoreTest.java",
                    &["shop/src/main/java/com/acme/Store.java"]
                ),
                related(
                    "shop/src/test/java/com/acme/StoreTests.java",
                    &["shop/src/main/java/com/acme/Store.java"]
                ),
                related(
                    "shop/src/test/java/com/acme/TestStore.java",
                    &["shop/src/main/java/com/acme/Store.java"]
                ),
            ]
        );
    }

    #[test]
    fn a_kotlin_class_is_tested_by_a_test_class_at_the_matching_src_test_path() {
        let tree = [
            "src/main/kotlin/acme/Store.kt",
            "src/test/kotlin/acme/StoreTest.kt",
        ];

        assert_eq!(
            tests_found(&tree, &changed(&["src/main/kotlin/acme/Store.kt"])),
            [related(
                "src/test/kotlin/acme/StoreTest.kt",
                &["src/main/kotlin/acme/Store.kt"]
            )]
        );
    }

    #[test]
    fn a_ruby_file_is_tested_by_a_spec_under_spec_or_a_test_under_test() {
        let tree = [
            "app/models/user.rb",
            "spec/models/user_spec.rb",
            "test/models/user_test.rb",
            "app/models/user_spec.rb",
            "spec/models/user_test.rb",
            "test/models/user_spec.rb",
            "engines/billing/spec/user_spec.rb",
        ];

        assert_eq!(
            tests_found(&tree, &changed(&["app/models/user.rb"])),
            [
                related("spec/models/user_spec.rb", &["app/models/user.rb"]),
                related("test/models/user_test.rb", &["app/models/user.rb"]),
            ]
        );
    }

    #[test]
    fn an_elixir_module_is_tested_by_a_test_script_under_test() {
        let tree = [
            "lib/shop/cart.ex",
            "test/shop/cart_test.exs",
            "lib/shop/cart_test.exs",
            "test/shop/cart_test.ex",
        ];

        assert_eq!(
            tests_found(&tree, &changed(&["lib/shop/cart.ex"])),
            [related("test/shop/cart_test.exs", &["lib/shop/cart.ex"])]
        );
    }

    #[test]
    fn a_touched_test_has_no_related_tests_of_its_own() {
        // Each touched file is a test, beside a file its stem would match
        // if it were a source.
        let tree = [
            "crates/shop/Cargo.toml",
            "crates/shop/tests/it/store.rs",
            "crates/shop/tests/store.rs",
            "store_test.go",
            "store_test_test.go",
            "tests/test_store.py",
            "tests/test_test_store.py",
            "web/cart.test.ts",
            "web/cart.test.test.ts",
            "web/__tests__/cart.ts",
            "web/__tests__/cart.test.ts",
            "src/test/java/acme/StoreTest.java",
            "src/test/java/acme/StoreTestTest.java",
            "spec/models/user_spec.rb",
            "spec/models/user_spec_spec.rb",
            "test/cart_test.exs",
            "test/cart_test_test.exs",
        ];

        assert_eq!(
            tests_found(
                &tree,
                &changed(&[
                    "crates/shop/tests/it/store.rs",
                    "store_test.go",
                    "tests/test_store.py",
                    "web/cart.test.ts",
                    "web/__tests__/cart.ts",
                    "src/test/java/acme/StoreTest.java",
                    "spec/models/user_spec.rb",
                    "test/cart_test.exs",
                ])
            ),
            []
        );
    }

    #[test]
    fn a_touched_file_is_built_by_the_nearest_manifest_above_it() {
        let tree = [
            "Cargo.toml",
            "crates/shop/Cargo.toml",
            "crates/shop/src/store.rs",
            "crates/shop/src/catalog/item.rs",
            "docs/guide.md",
        ];

        assert_eq!(
            manifests_found(
                &tree,
                &changed(&["crates/shop/src/catalog/item.rs", "docs/guide.md"])
            ),
            [
                related("Cargo.toml", &["docs/guide.md"]),
                related(
                    "crates/shop/Cargo.toml",
                    &["crates/shop/src/catalog/item.rs"]
                ),
            ]
        );
    }

    #[test]
    fn of_manifests_in_one_directory_a_file_gets_its_own_ecosystems_then_the_first_listed() {
        let tree = [
            "composer.json",
            "pyproject.toml",
            "package.json",
            "Cargo.toml",
            "setup.cfg",
        ];

        assert_eq!(
            manifests_found(
                &tree,
                &changed(&[
                    "src/lib.rs",
                    "web/index.ts",
                    "tools/gen.py",
                    "api/index.php",
                    "README.md"
                ])
            ),
            [
                related("Cargo.toml", &["README.md", "src/lib.rs"]),
                related("composer.json", &["api/index.php"]),
                related("package.json", &["web/index.ts"]),
                related("pyproject.toml", &["tools/gen.py"]),
            ]
        );
    }

    #[test]
    fn a_touched_manifest_or_test_is_never_related_material() {
        let tree = [
            "Cargo.toml",
            "crates/shop/Cargo.toml",
            "crates/shop/src/store.rs",
            "crates/shop/tests/store.rs",
        ];

        assert_eq!(
            found(
                &tree,
                &changed(&[
                    "crates/shop/Cargo.toml",
                    "crates/shop/src/store.rs",
                    "crates/shop/tests/store.rs",
                ])
            ),
            []
        );
    }

    #[test]
    fn a_file_related_to_several_touched_files_is_one_entry_listing_them_all_in_path_order() {
        let tree = [
            "pyproject.toml",
            "src/store.py",
            "src/cart.py",
            "src/test_cart.py",
            "lib/store.py",
            "tests/test_store.py",
        ];

        assert_eq!(
            found(
                &tree,
                &changed(&["src/store.py", "src/cart.py", "lib/store.py"])
            ),
            [
                (
                    "pyproject.toml".to_owned(),
                    RelatedGroup::Configuration,
                    vec![
                        "lib/store.py".to_owned(),
                        "src/cart.py".to_owned(),
                        "src/store.py".to_owned(),
                    ]
                ),
                (
                    "src/test_cart.py".to_owned(),
                    RelatedGroup::Test,
                    vec!["src/cart.py".to_owned()]
                ),
                (
                    "tests/test_store.py".to_owned(),
                    RelatedGroup::Test,
                    vec!["lib/store.py".to_owned(), "src/store.py".to_owned()]
                ),
            ]
        );
    }

    #[test]
    fn a_language_added_to_the_conventions_needs_no_code() {
        let conventions = Conventions::parse(
            r#"
            [[ecosystem]]
            name = "C#"
            extensions = ["cs"]
            manifests = ["Directory.Build.props"]

            [[ecosystem.tests]]
            names = ["{stem}Tests.cs"]
            look_in = { mirror = { from = "src", to = "tests/Unit" } }
            "#,
        )
        .expect("the conventions are valid");
        let tree = [
            "Directory.Build.props",
            "src/Shop/Store.cs",
            "tests/Unit/Shop/StoreTests.cs",
        ];

        assert_eq!(
            find_with(&conventions, tree, &changed(&["src/Shop/Store.cs"])),
            [
                RelatedFile {
                    path: "Directory.Build.props".to_owned(),
                    group: RelatedGroup::Configuration,
                    related_to: vec!["src/Shop/Store.cs".to_owned()],
                },
                RelatedFile {
                    path: "tests/Unit/Shop/StoreTests.cs".to_owned(),
                    group: RelatedGroup::Test,
                    related_to: vec!["src/Shop/Store.cs".to_owned()],
                },
            ]
        );
    }

    #[test]
    fn a_deleted_file_has_no_related_tests_or_configuration() {
        let tree = ["go.mod", "store/store_test.go", "store/cache.go"];
        let touched = [
            TouchedFile::deleted(
                "store/store.go".to_owned(),
                FileContent::Text(String::new()),
            ),
            TouchedFile::modified(
                "store/cache.go".to_owned(),
                FileContent::Text(String::new()),
                FileContent::Text(String::new()),
            ),
        ];

        assert_eq!(
            found(&tree, &touched),
            [(
                "go.mod".to_owned(),
                RelatedGroup::Configuration,
                vec!["store/cache.go".to_owned()]
            )]
        );
    }
}
