//! Related tests and related configuration: the files at a change's commit
//! that conventionally test or build its touched files, found from the
//! commit's paths alone, then sent whole as [`connections`] read from it.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::iter;

use whetstone_core::{
    Change, Connection, ConnectionGroup, Extent, FileContent, Relation, Tier, TouchedFile,
};

use super::{Failure, FileKind, Snapshot};

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
                    match snapshot.read(file.path()) {
                        Ok(Some(bytes)) => FileContent::from_bytes(bytes),
                        Ok(None) => return Err(Failure::NotInCommit),
                        Err(error) => return Err(Failure::Snapshot(Box::new(error))),
                    }
                }
            };
            let (group, relation): (_, fn(String) -> Relation) = match file.group() {
                RelatedGroup::Test => (ConnectionGroup::RelatedTest, |touched_file| {
                    Relation::TestFor { touched_file }
                }),
                RelatedGroup::Configuration => {
                    (ConnectionGroup::RelatedConfiguration, |touched_file| {
                        Relation::ManifestFor { touched_file }
                    })
                }
            };
            Ok(Connection::new(
                file.path().to_owned(),
                Extent::WholeFile,
                group,
                Tier::High,
                file.related_to().iter().cloned().map(relation).collect(),
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
    /// A related test: by its ecosystem's naming convention, it is a test
    /// for each touched file it relates to.
    Test,
    /// Related configuration: it is the nearest build manifest for each
    /// touched file it relates to.
    Configuration,
}

/// Finds the related tests and related configuration for a change's
/// `touched` files, from every file path in the `tree` at the change's
/// commit. Paths are relative to the repository's root and `/`-separated.
///
/// - **Related tests:** a touched file's tests by the naming convention of
///   the ecosystem its extension picks: Rust, Go, Python, JavaScript and
///   TypeScript, Java and Kotlin, Ruby, or Elixir. A touched file that is
///   itself a test, by the same conventions, has none.
/// - **Related configuration:** a touched file's nearest build manifest,
///   walking up to the repository's root. Of several manifests in one
///   directory, one that builds the touched file's ecosystem comes first,
///   then the first of `Cargo.toml`, `package.json`, `go.mod`,
///   `pyproject.toml`, `setup.cfg`, `pom.xml`, `build.gradle`,
///   `build.gradle.kts`, `Gemfile`, `mix.exs` and `composer.json`.
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
    let tree = Tree::new(tree);
    let mut found: BTreeMap<(String, RelatedGroup), BTreeSet<&str>> = BTreeMap::new();
    for file in touched.iter().filter(|file| file.after().is_some()) {
        let path = file.path();
        let source = Source::parse(path);
        for test in tree.tests_for(&source) {
            found
                .entry((test, RelatedGroup::Test))
                .or_default()
                .insert(path);
        }
        if let Some(manifest) = tree.nearest_manifest(&source) {
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
    ecosystem: Option<Ecosystem>,
}

impl<'a> Source<'a> {
    fn parse(path: &'a str) -> Self {
        let (dir, name) = split(path);
        let (stem, ecosystem) = match name.rsplit_once('.') {
            // A leading dot starts a hidden file's name, not an extension.
            Some((stem, extension)) if !stem.is_empty() => {
                (stem, Ecosystem::by_extension(extension))
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

/// A language's ecosystem, which decides a source file's test conventions
/// and which manifest it prefers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ecosystem {
    Rust,
    Go,
    Python,
    /// JavaScript and TypeScript.
    Script,
    /// Java and Kotlin.
    Jvm,
    Ruby,
    Elixir,
    /// PHP, which has a manifest but no test convention here.
    Php,
}

impl Ecosystem {
    fn by_extension(extension: &str) -> Option<Self> {
        match extension {
            "rs" => Some(Self::Rust),
            "go" => Some(Self::Go),
            "py" => Some(Self::Python),
            "java" | "kt" => Some(Self::Jvm),
            "rb" => Some(Self::Ruby),
            "ex" => Some(Self::Elixir),
            "php" => Some(Self::Php),
            _ if SCRIPT_EXTENSIONS.contains(&extension) => Some(Self::Script),
            _ => None,
        }
    }
}

/// The build manifests, by file name, with the ecosystem each builds, in
/// order of preference when one directory has several.
const MANIFESTS: [(&str, Ecosystem); 11] = [
    ("Cargo.toml", Ecosystem::Rust),
    ("package.json", Ecosystem::Script),
    ("go.mod", Ecosystem::Go),
    ("pyproject.toml", Ecosystem::Python),
    ("setup.cfg", Ecosystem::Python),
    ("pom.xml", Ecosystem::Jvm),
    ("build.gradle", Ecosystem::Jvm),
    ("build.gradle.kts", Ecosystem::Jvm),
    ("Gemfile", Ecosystem::Ruby),
    ("mix.exs", Ecosystem::Elixir),
    ("composer.json", Ecosystem::Php),
];

/// The extensions of JavaScript and TypeScript files.
const SCRIPT_EXTENSIONS: [&str; 8] = ["js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts"];

/// The file paths in the tree at the change's commit.
#[derive(Debug)]
struct Tree<'a> {
    paths: BTreeSet<&'a str>,
    /// The same paths, by file name.
    by_name: HashMap<&'a str, Vec<&'a str>>,
}

impl<'a> Tree<'a> {
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

    /// The manifest in the nearest directory to `source` that has one,
    /// walking up to the repository's root. Of several in that directory,
    /// one that builds the source's ecosystem comes first, then the first
    /// in [`MANIFESTS`].
    fn nearest_manifest(&self, source: &Source<'_>) -> Option<String> {
        ancestors(source.dir).find_map(|dir| {
            MANIFESTS
                .iter()
                .filter_map(|&(name, builds)| Some((self.file(dir, name)?, builds)))
                // The first of the smallest keys wins: the ecosystem's own,
                // else the first listed.
                .min_by_key(|&(_, builds)| Some(builds) != source.ecosystem)
                .map(|(manifest, _)| manifest)
        })
    }

    /// The related tests for `source`, by its ecosystem's conventions. A
    /// source that is itself a test, by the same conventions, has none.
    fn tests_for(&self, source: &Source<'_>) -> Vec<String> {
        let Source {
            dir,
            stem,
            ecosystem,
        } = *source;
        match ecosystem {
            Some(Ecosystem::Rust) => self.rust_tests(dir, stem),
            Some(Ecosystem::Go) => self.go_tests(dir, stem),
            Some(Ecosystem::Python) => self.python_tests(dir, stem),
            Some(Ecosystem::Script) => self.script_tests(dir, stem),
            Some(Ecosystem::Jvm) => self.jvm_tests(dir, stem),
            Some(Ecosystem::Ruby) => self.ruby_tests(dir, stem),
            Some(Ecosystem::Elixir) => self.elixir_tests(dir, stem),
            Some(Ecosystem::Php) | None => Vec::new(),
        }
    }

    /// `tests/s.rs` beside the nearest `Cargo.toml`. Everything in that
    /// `tests/` directory is a test itself.
    fn rust_tests(&self, dir: &str, stem: &str) -> Vec<String> {
        let Some(package) =
            ancestors(dir).find(|package| self.file(package, "Cargo.toml").is_some())
        else {
            return Vec::new();
        };
        let tests_dir = join(package, "tests");
        if ancestors(dir).any(|ancestor| ancestor == tests_dir) {
            return Vec::new();
        }
        self.file(&tests_dir, &format!("{stem}.rs"))
            .into_iter()
            .collect()
    }

    /// `s_test.go` in the same directory.
    fn go_tests(&self, dir: &str, stem: &str) -> Vec<String> {
        if stem.ends_with("_test") {
            return Vec::new();
        }
        self.file(dir, &format!("{stem}_test.go"))
            .into_iter()
            .collect()
    }

    /// `test_s.py` or `s_test.py`, in the same directory or anywhere under a
    /// `tests/` or `test/` directory of an ancestor.
    fn python_tests(&self, dir: &str, stem: &str) -> Vec<String> {
        if stem.starts_with("test_") || stem.ends_with("_test") {
            return Vec::new();
        }
        [format!("test_{stem}.py"), format!("{stem}_test.py")]
            .iter()
            .flat_map(|name| {
                let mut tests = self.under_test_dirs(dir, name, &["tests", "test"]);
                tests.extend(self.file(dir, name));
                tests
            })
            .collect()
    }

    /// `s.test.*` or `s.spec.*` in the same directory, or `__tests__/s.*`,
    /// with any script extension. Everything under a `__tests__` directory
    /// is a test itself.
    fn script_tests(&self, dir: &str, stem: &str) -> Vec<String> {
        if matches!(stem.rsplit_once('.'), Some((_, "test" | "spec")))
            || ancestors(dir).any(|ancestor| split(ancestor).1 == "__tests__")
        {
            return Vec::new();
        }
        let tests_dir = join(dir, "__tests__");
        SCRIPT_EXTENSIONS
            .iter()
            .flat_map(|extension| {
                [
                    self.file(dir, &format!("{stem}.test.{extension}")),
                    self.file(dir, &format!("{stem}.spec.{extension}")),
                    self.file(&tests_dir, &format!("{stem}.{extension}")),
                ]
            })
            .flatten()
            .collect()
    }

    /// `STest`, `STests`, `TestS` or `SIT`, in Java or Kotlin, at the path
    /// under `src/test/` that matches the source's path under `src/main/`.
    /// Only a source under `src/main/` has tests, so a test, under
    /// `src/test/`, has none.
    fn jvm_tests(&self, dir: &str, stem: &str) -> Vec<String> {
        let mut parts: Vec<&str> = dir.split('/').collect();
        let Some(source_set) = parts.windows(2).rposition(|pair| pair == ["src", "main"]) else {
            return Vec::new();
        };
        parts[source_set + 1] = "test";
        let tests_dir = parts.join("/");
        [
            format!("{stem}Test"),
            format!("{stem}Tests"),
            format!("Test{stem}"),
            format!("{stem}IT"),
        ]
        .iter()
        .flat_map(|name| {
            ["java", "kt"].map(|extension| self.file(&tests_dir, &format!("{name}.{extension}")))
        })
        .flatten()
        .collect()
    }

    /// `s_spec.rb` anywhere under a `spec/` directory of an ancestor, or
    /// `s_test.rb` anywhere under a `test/` one.
    fn ruby_tests(&self, dir: &str, stem: &str) -> Vec<String> {
        if stem.ends_with("_spec") || stem.ends_with("_test") {
            return Vec::new();
        }
        let mut tests = self.under_test_dirs(dir, &format!("{stem}_spec.rb"), &["spec"]);
        tests.extend(self.under_test_dirs(dir, &format!("{stem}_test.rb"), &["test"]));
        tests
    }

    /// `s_test.exs` anywhere under a `test/` directory of an ancestor. A
    /// test is a script, `.exs`, so it is never a source.
    fn elixir_tests(&self, dir: &str, stem: &str) -> Vec<String> {
        self.under_test_dirs(dir, &format!("{stem}_test.exs"), &["test"])
    }

    /// The files called `name` anywhere under a directory called one of
    /// `test_dirs` that is in `dir` or in a directory above it.
    fn under_test_dirs(&self, dir: &str, name: &str, test_dirs: &[&str]) -> Vec<String> {
        self.by_name
            .get(name)
            .into_iter()
            .flatten()
            .filter(|path| {
                ancestors(split(path).0).any(|found_in| {
                    let (parent, dir_name) = split(found_in);
                    test_dirs.contains(&dir_name)
                        && ancestors(dir).any(|ancestor| ancestor == parent)
                })
            })
            .map(|path| (*path).to_owned())
            .collect()
    }
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
