//! The conventions that find related tests and related configuration, by
//! language ecosystem, read from `conventions.toml`, which Whetstone embeds
//! and which documents them. Reading them validates them, so the parent
//! module can apply them without checking them again.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

/// The embedded conventions file.
const EMBEDDED: &str = include_str!("conventions.toml");

/// The embedded conventions, read the first time they are needed.
static CONVENTIONS: LazyLock<Conventions> = LazyLock::new(|| {
    Conventions::parse(EMBEDDED)
        .expect("the embedded conventions file is valid, which a unit test checks")
});

/// Every ecosystem's conventions.
#[derive(Debug)]
pub(super) struct Conventions {
    /// The ecosystems, in the file's order, which is the order of preference
    /// of their manifests.
    ecosystems: Vec<Ecosystem>,
    /// The index in `ecosystems` of the one each extension picks.
    by_extension: HashMap<String, usize>,
}

impl Conventions {
    /// The conventions Whetstone embeds.
    pub(super) fn embedded() -> &'static Self {
        &CONVENTIONS
    }

    /// The conventions in `text`, a conventions file. It fails if `text` is
    /// not one, or if what it says cannot be applied: see
    /// [`ConventionsError`] and [`TestNameError`].
    pub(super) fn parse(text: &str) -> Result<Self, ConventionsError> {
        let File { ecosystems } = toml::from_str(text).map_err(ConventionsError::Toml)?;
        let mut by_extension = HashMap::new();
        let mut manifests = HashMap::new();
        for (index, ecosystem) in ecosystems.iter().enumerate() {
            ecosystem.validate()?;
            for extension in &ecosystem.extensions {
                if let Some(first) = by_extension.insert(extension.clone(), index) {
                    return Err(ConventionsError::ExtensionClaimedTwice {
                        extension: extension.clone(),
                        first: ecosystems[first].name.clone(),
                        second: ecosystem.name.clone(),
                    });
                }
            }
            for manifest in &ecosystem.manifests {
                if let Some(first) = manifests.insert(manifest, index) {
                    return Err(ConventionsError::ManifestListedTwice {
                        manifest: manifest.clone(),
                        first: ecosystems[first].name.clone(),
                        second: ecosystem.name.clone(),
                    });
                }
            }
        }
        Ok(Self {
            ecosystems,
            by_extension,
        })
    }

    /// The ecosystem a file with `extension` is one of, if any.
    pub(super) fn by_extension(&self, extension: &str) -> Option<&Ecosystem> {
        self.by_extension
            .get(extension)
            .map(|&index| &self.ecosystems[index])
    }

    /// Every ecosystem's manifests, in order of preference.
    pub(super) fn manifests(&self) -> impl Iterator<Item = &str> {
        self.ecosystems
            .iter()
            .flat_map(|ecosystem| ecosystem.manifests.iter().map(String::as_str))
    }
}

/// Why a conventions file cannot be used.
#[derive(Debug, thiserror::Error)]
pub(super) enum ConventionsError {
    /// It is not TOML, or not in the conventions file's shape: an unknown
    /// field or kind of place to look in, a missing field, or a test name
    /// that cannot be used ([`TestNameError`]).
    #[error("the conventions file is not valid")]
    Toml(#[source] toml::de::Error),
    /// An extension is not one a file name can end with.
    #[error("{ecosystem} has the extension `{extension}`, which is empty or has a `.` or `/`")]
    BadExtension {
        ecosystem: String,
        extension: String,
    },
    /// Two ecosystems claim the same extension, so it could pick either.
    #[error("both {first} and {second} claim the extension `{extension}`")]
    ExtensionClaimedTwice {
        extension: String,
        first: String,
        second: String,
    },
    /// Two ecosystems list the same manifest, so which one it builds is
    /// unclear.
    #[error("both {first} and {second} list the manifest `{manifest}`")]
    ManifestListedTwice {
        manifest: String,
        first: String,
        second: String,
    },
    /// A manifest's or a directory's name is not one path segment.
    #[error("{ecosystem} has `{name}` as a file or directory name, which is empty or has a `/`")]
    BadName { ecosystem: String, name: String },
    /// An ecosystem looks beside its manifest but has none.
    #[error("{ecosystem} looks beside its manifest, but lists none")]
    NoManifest { ecosystem: String },
    /// An ecosystem tells a test by an empty start or end of its stem,
    /// which every file has, so it would never have related tests.
    #[error("{ecosystem} tells a test by an empty start or end of its stem")]
    EmptyStemAffix { ecosystem: String },
}

/// The conventions file as written.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(rename = "ecosystem")]
    ecosystems: Vec<Ecosystem>,
}

/// A language's ecosystem: which files are its, what builds them, and what
/// tests them.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Ecosystem {
    /// What error messages call it.
    name: String,
    /// The extensions, without the dot, of its files.
    pub(super) extensions: Vec<String>,
    /// Its build manifests' file names, in order of preference.
    #[serde(default)]
    pub(super) manifests: Vec<String>,
    /// Its test rules.
    #[serde(default)]
    pub(super) tests: Vec<TestRule>,
    /// How to tell that one of its files is a test itself.
    #[serde(default)]
    pub(super) is_test: IsTest,
}

impl Ecosystem {
    /// Whether its manifests include `manifest`.
    pub(super) fn builds_with(&self, manifest: &str) -> bool {
        self.manifests.iter().any(|own| own == manifest)
    }

    /// Checks what the file's shape cannot.
    fn validate(&self) -> Result<(), ConventionsError> {
        let ecosystem = || self.name.clone();
        if let Some(extension) = self
            .extensions
            .iter()
            .find(|extension| extension.is_empty() || extension.contains(['.', '/']))
        {
            return Err(ConventionsError::BadExtension {
                ecosystem: ecosystem(),
                extension: extension.clone(),
            });
        }
        let names = self
            .manifests
            .iter()
            .chain(&self.is_test.under_dir)
            .chain(&self.is_test.under_dir_beside_manifest)
            .map(String::as_str)
            .chain(self.tests.iter().flat_map(|rule| rule.look_in.dirs()));
        for name in names {
            if name.is_empty() || name.contains('/') {
                return Err(ConventionsError::BadName {
                    ecosystem: ecosystem(),
                    name: name.to_owned(),
                });
            }
        }
        let beside_manifest = self
            .tests
            .iter()
            .any(|rule| matches!(rule.look_in, Location::BesideManifest(_)))
            || !self.is_test.under_dir_beside_manifest.is_empty();
        if beside_manifest && self.manifests.is_empty() {
            return Err(ConventionsError::NoManifest {
                ecosystem: ecosystem(),
            });
        }
        if self
            .is_test
            .stem_starts_with
            .iter()
            .chain(&self.is_test.stem_ends_with)
            .any(String::is_empty)
        {
            return Err(ConventionsError::EmptyStemAffix {
                ecosystem: ecosystem(),
            });
        }
        Ok(())
    }
}

/// A test rule: files with one of its names, in one place.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TestRule {
    /// The names of the files that test a touched file.
    pub(super) names: Vec<TestName>,
    /// Where they are.
    pub(super) look_in: Location,
}

/// Where a test rule looks for a touched file's tests.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Location {
    /// The touched file's directory.
    SameDir,
    /// The directory of this name inside the touched file's directory.
    Subdir(String),
    /// The directory of this name beside the nearest of the ecosystem's
    /// manifests, walking up from the touched file.
    BesideManifest(String),
    /// Anywhere, at any depth, under a directory with one of these names
    /// that is in the touched file's directory or a directory above it.
    UnderAncestorDir(Vec<String>),
    /// The touched file's directory with the last `from` in its path
    /// replaced by `to`, both `/`-separated and matched by whole segments.
    /// A touched file with no `from` in its directory's path has no tests
    /// here.
    Mirror { from: String, to: String },
}

impl Location {
    /// The directory names it has, each one path segment if it is valid. A
    /// mirror's paths are split into theirs.
    fn dirs(&self) -> Vec<&str> {
        match self {
            Self::SameDir => Vec::new(),
            Self::Subdir(dir) | Self::BesideManifest(dir) => vec![dir],
            Self::UnderAncestorDir(dirs) => dirs.iter().map(String::as_str).collect(),
            Self::Mirror { from, to } => from.split('/').chain(to.split('/')).collect(),
        }
    }
}

/// How to tell that a touched file is a test itself. It is one if any of
/// these says so.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct IsTest {
    /// Its stem starts with one of these.
    pub(super) stem_starts_with: Vec<String>,
    /// Its stem ends with one of these.
    pub(super) stem_ends_with: Vec<String>,
    /// It is anywhere under a directory with one of these names.
    pub(super) under_dir: Vec<String>,
    /// It is anywhere under a directory with one of these names beside the
    /// nearest of the ecosystem's manifests.
    pub(super) under_dir_beside_manifest: Vec<String>,
}

/// A test's file name, with `{stem}` for the touched file's stem and, if
/// it has it, `{ext}` for each of the ecosystem's extensions.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub(super) struct TestName(Vec<Piece>);

/// A piece of a [`TestName`].
#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    /// Text as written.
    Text(String),
    /// `{stem}`.
    Stem,
    /// `{ext}`.
    Extension,
}

impl TestName {
    /// The file names it stands for, for a touched file with `stem`: one,
    /// or one for each of `extensions` if it has `{ext}`.
    pub(super) fn expand(&self, stem: &str, extensions: &[String]) -> Vec<String> {
        let fill = |extension: &str| -> String {
            self.0
                .iter()
                .map(|piece| match piece {
                    Piece::Text(text) => text.as_str(),
                    Piece::Stem => stem,
                    Piece::Extension => extension,
                })
                .collect()
        };
        if self.0.contains(&Piece::Extension) {
            extensions.iter().map(|extension| fill(extension)).collect()
        } else {
            vec![fill("")]
        }
    }
}

impl TryFrom<String> for TestName {
    type Error = TestNameError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        if name.is_empty() {
            return Err(TestNameError::Empty);
        }
        if name.contains('/') {
            return Err(TestNameError::NotAFileName(name));
        }
        let mut pieces = Vec::new();
        let mut rest = name.as_str();
        while !rest.is_empty() {
            let (text, placeholder) = match rest.split_once('{') {
                Some((text, after)) => {
                    let Some((placeholder, after)) = after.split_once('}') else {
                        return Err(TestNameError::UnmatchedBrace(name));
                    };
                    rest = after;
                    (text, Some(placeholder))
                }
                None => (std::mem::take(&mut rest), None),
            };
            if text.contains('}') {
                return Err(TestNameError::UnmatchedBrace(name));
            }
            if !text.is_empty() {
                pieces.push(Piece::Text(text.to_owned()));
            }
            match placeholder {
                Some("stem") => pieces.push(Piece::Stem),
                Some("ext") => pieces.push(Piece::Extension),
                Some(unknown) => {
                    return Err(TestNameError::UnknownPlaceholder {
                        placeholder: unknown.to_owned(),
                        name,
                    });
                }
                None => {}
            }
        }
        if !pieces.contains(&Piece::Stem) {
            return Err(TestNameError::NoStem(name));
        }
        Ok(Self(pieces))
    }
}

/// Why a test name cannot be used.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(super) enum TestNameError {
    /// It is empty.
    #[error("a test name is empty")]
    Empty,
    /// It has a `/`, so it is not a file name.
    #[error("the test name `{0}` has a `/`, so it is not a file name")]
    NotAFileName(String),
    /// A `{` or `}` is not part of a placeholder.
    #[error("the test name `{0}` has a `{{` or `}}` that is not part of a placeholder")]
    UnmatchedBrace(String),
    /// A placeholder is neither `{stem}` nor `{ext}`.
    #[error("the test name `{name}` has `{{{placeholder}}}`, which is not `{{stem}}` or `{{ext}}`")]
    UnknownPlaceholder { name: String, placeholder: String },
    /// It has no `{stem}`, so it would be the test of every file.
    #[error("the test name `{0}` has no `{{stem}}`")]
    NoStem(String),
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::iter;

    use super::*;

    /// Why `text` is not a valid conventions file, with each cause after
    /// the error it caused.
    fn why_invalid(text: &str) -> String {
        let error = Conventions::parse(text).expect_err("the conventions are not valid");
        iter::successors(Some(&error as &dyn Error), |&error| error.source())
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(": ")
    }

    /// A conventions file of one ecosystem, `Lang`, with `extra` added to
    /// its table and one test rule, looking in `look_in` for `names`.
    fn one_ecosystem(extra: &str, names: &str, look_in: &str) -> String {
        format!(
            "[[ecosystem]]\n\
             name = \"Lang\"\n\
             extensions = [\"lg\"]\n\
             {extra}\n\
             [[ecosystem.tests]]\n\
             names = {names}\n\
             look_in = {look_in}\n"
        )
    }

    #[test]
    fn the_embedded_conventions_file_is_valid() {
        let conventions =
            Conventions::parse(EMBEDDED).expect("the embedded conventions file is valid");

        assert_eq!(
            conventions
                .by_extension("tsx")
                .map(|ecosystem| ecosystem.name.as_str()),
            Some("JavaScript and TypeScript")
        );
        assert_eq!(conventions.manifests().next(), Some("Cargo.toml"));
    }

    #[test]
    fn each_kind_of_place_to_look_in_is_read() {
        for look_in in [
            "\"same_dir\"",
            "{ subdir = \"__tests__\" }",
            "{ under_ancestor_dir = [\"tests\", \"test\"] }",
            "{ mirror = { from = \"src/main\", to = \"src/test\" } }",
        ] {
            let text = one_ecosystem("", "[\"{stem}_test.{ext}\"]", look_in);

            assert!(Conventions::parse(&text).is_ok(), "{look_in}");
        }
        let text = one_ecosystem(
            "manifests = [\"lang.mod\"]",
            "[\"{stem}.lg\"]",
            "{ beside_manifest = \"tests\" }",
        );
        assert!(Conventions::parse(&text).is_ok());
    }

    #[test]
    fn an_unknown_kind_of_place_to_look_in_is_an_error() {
        let why = why_invalid(&one_ecosystem("", "[\"{stem}_test.lg\"]", "\"nearby\""));

        assert!(
            why.starts_with("the conventions file is not valid: "),
            "{why}"
        );
        assert!(why.contains("unknown variant `nearby`"), "{why}");
    }

    #[test]
    fn an_unknown_field_is_an_error() {
        for (extra, look_in, field) in [
            (
                "is_test.stem_contains = [\"test\"]",
                "\"same_dir\"",
                "unknown field `stem_contains`",
            ),
            (
                "tests_in = \"spec\"",
                "\"same_dir\"",
                "unknown field `tests_in`",
            ),
            (
                "",
                "{ mirror = { from = \"a\", to = \"b\", depth = 2 } }",
                "unexpected keys in table: depth",
            ),
        ] {
            let why = why_invalid(&one_ecosystem(extra, "[\"{stem}_test.lg\"]", look_in));

            assert!(why.contains(field), "{why}");
        }
    }

    #[test]
    fn a_test_name_that_cannot_be_used_is_an_error_where_it_is_written() {
        let why = why_invalid(&one_ecosystem("", "[\"{stem}_{kind}.lg\"]", "\"same_dir\""));

        assert!(
            why.contains(
                "the test name `{stem}_{kind}.lg` has `{kind}`, which is not `{stem}` or `{ext}`"
            ),
            "{why}"
        );
        assert!(why.contains("line 6"), "{why}");
    }

    #[test]
    fn a_test_name_needs_its_stem_and_known_placeholders_only() {
        let parse = |name: &str| TestName::try_from(name.to_owned());

        assert_eq!(parse(""), Err(TestNameError::Empty));
        assert_eq!(
            parse("{stem}/index.lg"),
            Err(TestNameError::NotAFileName("{stem}/index.lg".to_owned()))
        );
        assert_eq!(
            parse("{name}_test.lg"),
            Err(TestNameError::UnknownPlaceholder {
                name: "{name}_test.lg".to_owned(),
                placeholder: "name".to_owned(),
            })
        );
        for unmatched in ["{stem_test.lg", "{stem}_test}.lg", "}{stem}.lg"] {
            assert_eq!(
                parse(unmatched),
                Err(TestNameError::UnmatchedBrace(unmatched.to_owned()))
            );
        }
        assert_eq!(
            parse("conftest.{ext}"),
            Err(TestNameError::NoStem("conftest.{ext}".to_owned()))
        );
    }

    #[test]
    fn a_test_name_stands_for_one_file_or_one_per_extension() {
        let extensions = ["java".to_owned(), "kt".to_owned()];
        let expand = |name: &str| {
            TestName::try_from(name.to_owned())
                .expect("the test name is valid")
                .expand("Store", &extensions)
        };

        assert_eq!(expand("Test{stem}.java"), ["TestStore.java"]);
        assert_eq!(expand("{stem}IT.{ext}"), ["StoreIT.java", "StoreIT.kt"]);
        assert_eq!(expand("{stem}.{stem}"), ["Store.Store"]);
    }

    #[test]
    fn an_extension_claimed_by_two_ecosystems_is_an_error() {
        let text = "[[ecosystem]]\n\
                    name = \"TypeScript\"\n\
                    extensions = [\"ts\"]\n\
                    [[ecosystem]]\n\
                    name = \"Deno\"\n\
                    extensions = [\"js\", \"ts\"]\n";

        assert_eq!(
            why_invalid(text),
            "both TypeScript and Deno claim the extension `ts`"
        );
    }

    #[test]
    fn a_manifest_listed_by_two_ecosystems_is_an_error() {
        let text = "[[ecosystem]]\n\
                    name = \"Java\"\n\
                    extensions = [\"java\"]\n\
                    manifests = [\"pom.xml\", \"build.gradle\"]\n\
                    [[ecosystem]]\n\
                    name = \"Kotlin\"\n\
                    extensions = [\"kt\"]\n\
                    manifests = [\"build.gradle\"]\n";

        assert_eq!(
            why_invalid(text),
            "both Java and Kotlin list the manifest `build.gradle`"
        );
    }

    #[test]
    fn an_extension_with_a_dot_or_none_at_all_is_an_error() {
        for extension in [".lg", "", "d.lg"] {
            let text = format!("[[ecosystem]]\nname = \"Lang\"\nextensions = [\"{extension}\"]\n");

            assert_eq!(
                why_invalid(&text),
                format!("Lang has the extension `{extension}`, which is empty or has a `.` or `/`")
            );
        }
    }

    #[test]
    fn a_manifest_or_directory_name_that_is_not_one_path_segment_is_an_error() {
        for (extra, look_in, name) in [
            (
                "manifests = [\"build/lang.mod\"]",
                "\"same_dir\"",
                "build/lang.mod",
            ),
            ("", "{ subdir = \"\" }", ""),
            ("", "{ under_ancestor_dir = [\"spec/unit\"] }", "spec/unit"),
            ("is_test.under_dir = [\"\"]", "\"same_dir\"", ""),
            (
                "",
                "{ mirror = { from = \"src//main\", to = \"src/test\" } }",
                "",
            ),
        ] {
            let why = why_invalid(&one_ecosystem(extra, "[\"{stem}_test.lg\"]", look_in));

            assert_eq!(
                why,
                format!(
                    "Lang has `{name}` as a file or directory name, which is empty or has a `/`"
                )
            );
        }
    }

    #[test]
    fn looking_beside_a_manifest_needs_one() {
        for (extra, look_in) in [
            ("", "{ beside_manifest = \"tests\" }"),
            (
                "is_test.under_dir_beside_manifest = [\"tests\"]",
                "\"same_dir\"",
            ),
        ] {
            let why = why_invalid(&one_ecosystem(extra, "[\"{stem}.lg\"]", look_in));

            assert_eq!(why, "Lang looks beside its manifest, but lists none");
        }
    }

    #[test]
    fn an_empty_start_or_end_of_a_tests_stem_is_an_error() {
        for extra in [
            "is_test.stem_starts_with = [\"\"]",
            "is_test.stem_ends_with = [\"_test\", \"\"]",
        ] {
            let why = why_invalid(&one_ecosystem(
                extra,
                "[\"{stem}_test.lg\"]",
                "\"same_dir\"",
            ));

            assert_eq!(
                why,
                "Lang tells a test by an empty start or end of its stem"
            );
        }
    }
}
