//! Areas: the lenses on a change that questions focus on. Each is defined by
//! an area file: TOML front matter between two `+++` lines, with a `name`
//! and a one-line `description` and nothing else, then the area prompt in
//! Markdown.
//!
//! ```text
//! +++
//! name = "System design"
//! description = "How the parts fit together, and why this way"
//! +++
//!
//! Focus on the structure the change creates or alters: ...
//! ```
//!
//! A repository keeps its area files in `.whetstone/areas/`. Whetstone's
//! default area files are in this crate's `areas/` directory, embedded at
//! build time.

use std::fmt;
use std::str::FromStr;

use crate::toml_error;

/// A lens on a change that questions focus on, such as domain or system
/// design, as its area file defines it.
#[derive(Clone, PartialEq, Eq)]
pub struct Area {
    id: AreaId,
    name: String,
    description: String,
    prompt: String,
    source: AreaSource,
}

/// Shows the area prompt's length, never its text.
impl fmt::Debug for Area {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Area")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("description", &self.description)
            .field("prompt_bytes", &self.prompt.len())
            .field("source", &self.source)
            .finish()
    }
}

/// An area's identity: its file name without `.md`, such as
/// `system-design`. It is made of lowercase ASCII letters, digits and
/// hyphens.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AreaId(String);

/// Where an area's file comes from. It reads as the outbound review says
/// it:
///
/// ```
/// use whetstone_core::AreaSource;
///
/// assert_eq!(AreaSource::Repository.to_string(), "this repository");
/// assert_eq!(AreaSource::Default.to_string(), "Whetstone default");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreaSource {
    /// The learner's repository, in `.whetstone/areas/`.
    Repository,
    /// Whetstone's own defaults, which a repository without area files gets.
    Default,
}

/// Whetstone's default area files, in file-name order.
const DEFAULT_FILES: [(&str, &[u8]); 5] = [
    ("domain.md", include_bytes!("../areas/domain.md")),
    (
        "infrastructure.md",
        include_bytes!("../areas/infrastructure.md"),
    ),
    (
        "observability.md",
        include_bytes!("../areas/observability.md"),
    ),
    ("scalability.md", include_bytes!("../areas/scalability.md")),
    (
        "system-design.md",
        include_bytes!("../areas/system-design.md"),
    ),
];

/// The most characters an area's description may have.
const DESCRIPTION_LIMIT: usize = 200;

/// The keys an area file's front matter has, and the only ones it may have.
const FRONT_MATTER_KEYS: [&str; 2] = ["name", "description"];

/// Text that is not an [`AreaId`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not an area ID: use lowercase letters, digits and hyphens")]
pub struct InvalidAreaId(String);

/// Why an area file is not a valid area.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AreaFileError {
    /// The file name is not an area ID followed by `.md`.
    #[error(
        "the file name must be an area ID followed by `.md`: lowercase letters, digits and hyphens"
    )]
    InvalidFileName,
    /// The file could not be read, for the reason given, such as a symlink
    /// to nothing or a directory with an area file's name.
    #[error("the file cannot be read: {0}")]
    Unreadable(String),
    /// The file is larger than 16 KB, 16,000 bytes.
    #[error("the file is larger than {} KB", Area::FILE_LIMIT / 1000)]
    TooLarge,
    /// The file is not UTF-8 text.
    #[error("the file is not UTF-8 text")]
    NotUtf8,
    /// The file does not start with a `+++` line.
    #[error("the file must start with TOML front matter between two `+++` lines")]
    NoFrontMatter,
    /// The front matter has no closing `+++` line.
    #[error("the front matter has no closing `+++` line")]
    UnclosedFrontMatter,
    /// The front matter is not valid TOML.
    #[error("the front matter is not valid TOML at line {line}: {message}")]
    Toml {
        /// The line of the file where the problem is, counting from 1.
        line: usize,
        /// What the problem is.
        message: String,
    },
    /// The front matter lacks a required key: `name` or `description`.
    #[error("the front matter has no `{0}`")]
    MissingKey(&'static str),
    /// The front matter has a key other than `name` and `description`.
    #[error("the front matter has the key `{0}`, but only `name` and `description` are allowed")]
    UnknownKey(String),
    /// `name` or `description` is not a string.
    #[error("`{0}` in the front matter must be a string")]
    NotAString(&'static str),
    /// `name` is empty.
    #[error("`name` is empty")]
    EmptyName,
    /// `name` is more than one line.
    #[error("`name` must be one line")]
    MultilineName,
    /// `description` is empty.
    #[error("`description` is empty")]
    EmptyDescription,
    /// `description` is more than one line.
    #[error("`description` must be one line")]
    MultilineDescription,
    /// `description` is longer than 200 characters.
    #[error("`description` is longer than {DESCRIPTION_LIMIT} characters")]
    LongDescription,
    /// Nothing follows the front matter.
    #[error("the area prompt after the front matter is empty")]
    EmptyPrompt,
}

/// An area file that is not a valid area: its file name, and why.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{file_name} is not a valid area file")]
pub struct InvalidAreaFile {
    file_name: String,
    #[source]
    reason: AreaFileError,
}

impl Area {
    /// The most bytes an area file may have: 16 KB, counted in thousands as
    /// every size Whetstone shows is. A reader need not read further to know
    /// a file is too large.
    pub const FILE_LIMIT: usize = 16_000;

    /// Parses the area file called `file_name` with the contents `contents`,
    /// which comes from `source`.
    fn parse(file_name: &str, contents: &[u8], source: AreaSource) -> Result<Self, AreaFileError> {
        let id = file_name
            .strip_suffix(".md")
            .and_then(|id| id.parse().ok())
            .ok_or(AreaFileError::InvalidFileName)?;
        if contents.len() > Self::FILE_LIMIT {
            return Err(AreaFileError::TooLarge);
        }
        let text = str::from_utf8(contents).map_err(|_| AreaFileError::NotUtf8)?;
        let (front_matter, body) = split_front_matter(text)?;
        let table: toml::Table = front_matter.parse().map_err(|error: toml::de::Error| {
            let (line, message) = toml_error::line_and_message(front_matter, &error);
            // The front matter starts on the file's second line.
            AreaFileError::Toml {
                line: line + 1,
                message,
            }
        })?;
        if let Some(key) = table
            .keys()
            .find(|key| !FRONT_MATTER_KEYS.contains(&key.as_str()))
        {
            return Err(AreaFileError::UnknownKey(key.clone()));
        }
        let text_of = |key: &'static str| {
            table
                .get(key)
                .ok_or(AreaFileError::MissingKey(key))?
                .as_str()
                .ok_or(AreaFileError::NotAString(key))
        };
        let name = text_of("name")?.trim();
        if name.is_empty() {
            return Err(AreaFileError::EmptyName);
        }
        if name.contains(['\n', '\r']) {
            return Err(AreaFileError::MultilineName);
        }
        let description = text_of("description")?.trim();
        if description.is_empty() {
            return Err(AreaFileError::EmptyDescription);
        }
        if description.contains(['\n', '\r']) {
            return Err(AreaFileError::MultilineDescription);
        }
        if description.chars().count() > DESCRIPTION_LIMIT {
            return Err(AreaFileError::LongDescription);
        }
        let prompt = body.trim();
        if prompt.is_empty() {
            return Err(AreaFileError::EmptyPrompt);
        }
        Ok(Self {
            id,
            name: name.to_owned(),
            description: description.to_owned(),
            prompt: prompt.to_owned(),
            source,
        })
    }

    /// Whetstone's default areas, which a repository without area files
    /// gets, in file-name order: domain, infrastructure, observability,
    /// scalability and system design.
    #[must_use]
    pub fn defaults() -> Vec<Self> {
        DEFAULT_FILES
            .iter()
            .map(|&(file_name, contents)| {
                Self::parse(file_name, contents, AreaSource::Default)
                    .expect("build.rs checks that Whetstone's default area files are valid")
            })
            .collect()
    }

    /// Whetstone's default area files, each a file name and its contents,
    /// in file-name order: the files [`Area::defaults`] parses, for a
    /// repository to start its own from.
    #[must_use]
    pub fn default_files() -> &'static [(&'static str, &'static [u8])] {
        &DEFAULT_FILES
    }

    /// The areas a repository offers when its area files are `files`, each a
    /// file name and its contents, or why they could not be read. Any area
    /// file, valid or not, replaces the defaults entirely; without any, the
    /// repository offers [`Area::defaults`]. Either way the areas come in
    /// file-name order, and an invalid file comes with the reason:
    ///
    /// ```
    /// use whetstone_core::{Area, AreaFileError};
    ///
    /// let security = "+++\n\
    ///                 name = \"Security\"\n\
    ///                 description = \"Who can do what\"\n\
    ///                 +++\n\
    ///                 Look at who is trusted.\n";
    /// let offered = Area::offered([
    ///     ("security.md", Ok(security.as_bytes())),
    ///     ("accessibility.md", Ok(b"Who can use it".as_slice())),
    ///     ("cost.md", Err("permission denied".to_owned())),
    /// ]);
    ///
    /// let invalid = offered[0].as_ref().expect_err("no front matter");
    /// assert_eq!(invalid.file_name(), "accessibility.md");
    /// assert_eq!(invalid.reason(), &AreaFileError::NoFrontMatter);
    /// let unreadable = offered[1].as_ref().expect_err("not read");
    /// assert_eq!(unreadable.reason().to_string(), "the file cannot be read: permission denied");
    /// assert_eq!(offered[2].as_ref().map(Area::name), Ok("Security"));
    /// ```
    #[must_use]
    pub fn offered<'a>(
        files: impl IntoIterator<Item = (&'a str, Result<&'a [u8], String>)>,
    ) -> Vec<Result<Self, InvalidAreaFile>> {
        let mut files: Vec<_> = files.into_iter().collect();
        if files.is_empty() {
            return Self::defaults().into_iter().map(Ok).collect();
        }
        files.sort_by_key(|&(file_name, _)| file_name);
        files
            .into_iter()
            .map(|(file_name, contents)| {
                contents
                    .map_err(AreaFileError::Unreadable)
                    .and_then(|contents| Self::parse(file_name, contents, AreaSource::Repository))
                    .map_err(|reason| InvalidAreaFile {
                        file_name: file_name.to_owned(),
                        reason,
                    })
            })
            .collect()
    }

    /// The area's ID.
    #[must_use]
    pub fn id(&self) -> &AreaId {
        &self.id
    }

    /// The area's name, as the learner sees it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What the area looks for, in one line.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// The area prompt: the area's instructions for what to look for in a
    /// change.
    #[must_use]
    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    /// Where the area's file comes from.
    #[must_use]
    pub fn source(&self) -> AreaSource {
        self.source
    }
}

/// The front matter and the body of an area file's `text`: what lies
/// between its first line, `+++`, and the next `+++` line, and what follows.
fn split_front_matter(text: &str) -> Result<(&str, &str), AreaFileError> {
    let is_delimiter = |line: &str| line.trim_end() == "+++";
    let opening = text
        .split_inclusive('\n')
        .next()
        .filter(|line| is_delimiter(line))
        .ok_or(AreaFileError::NoFrontMatter)?;
    let (_, rest) = text.split_at(opening.len());
    let mut end = 0;
    for line in rest.split_inclusive('\n') {
        if is_delimiter(line) {
            let (front_matter, closing_and_body) = rest.split_at(end);
            let (_, body) = closing_and_body.split_at(line.len());
            return Ok((front_matter, body));
        }
        end += line.len();
    }
    Err(AreaFileError::UnclosedFrontMatter)
}

impl InvalidAreaFile {
    /// The file's name.
    #[must_use]
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// Why the file is not a valid area.
    #[must_use]
    pub fn reason(&self) -> &AreaFileError {
        &self.reason
    }
}

impl AreaId {
    /// The ID as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for AreaId {
    type Err = InvalidAreaId;

    /// The area ID `id`, if it is one: not empty, and only lowercase ASCII
    /// letters, digits and hyphens.
    ///
    /// ```
    /// use whetstone_core::AreaId;
    ///
    /// assert!("system-design".parse::<AreaId>().is_ok());
    /// assert!("system design".parse::<AreaId>().is_err());
    /// ```
    fn from_str(id: &str) -> Result<Self, Self::Err> {
        if !id.is_empty()
            && id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            Ok(Self(id.to_owned()))
        } else {
            Err(InvalidAreaId(id.to_owned()))
        }
    }
}

impl fmt::Display for AreaSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Repository => "this repository",
            Self::Default => "Whetstone default",
        })
    }
}

impl fmt::Display for AreaId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Area::parse` on `file_name` with `contents`, from a repository.
    fn parse(file_name: &str, contents: &str) -> Result<Area, AreaFileError> {
        Area::parse(file_name, contents.as_bytes(), AreaSource::Repository)
    }

    #[test]
    fn an_area_debugs_its_prompts_length_never_its_text() {
        let area = parse(
            "domain.md",
            "+++\nname = \"Domain\"\ndescription = \"The rules\"\n+++\n\nLook at the rules.\n",
        )
        .expect("the file is valid");

        let debug = format!("{area:?}");

        assert!(!debug.contains("Look at"), "{debug}");
        assert!(debug.contains("prompt_bytes: 18"), "{debug}");
    }

    #[test]
    fn an_area_file_gives_its_id_name_description_prompt_and_source() {
        let area = parse(
            "system-design.md",
            "+++\n\
             name = \"System design\"\n\
             description = \"How the parts fit together\"\n\
             +++\n\
             \n\
             Look at the boundaries between parts.\n\
             \n\
             Ask what each one owns.\n",
        )
        .expect("the file is valid");

        assert_eq!(area.id().as_str(), "system-design");
        assert_eq!(area.name(), "System design");
        assert_eq!(area.description(), "How the parts fit together");
        assert_eq!(
            area.prompt(),
            "Look at the boundaries between parts.\n\nAsk what each one owns."
        );
        assert_eq!(area.source(), AreaSource::Repository);
    }

    /// A valid area file's text.
    const VALID: &str =
        "+++\nname = \"Domain\"\ndescription = \"The rules\"\n+++\nLook at the rules.\n";

    /// Why `Area::parse` rejects `file_name` with `contents`.
    fn reason(file_name: &str, contents: &str) -> AreaFileError {
        parse(file_name, contents).expect_err("the file is invalid")
    }

    #[test]
    fn a_file_name_that_is_not_an_area_id_followed_by_md_is_invalid() {
        for file_name in [
            "System-design.md",
            "system design.md",
            "system_design.md",
            "système.md",
            ".md",
            "domain.markdown",
            "domain",
        ] {
            assert_eq!(
                reason(file_name, VALID),
                AreaFileError::InvalidFileName,
                "{file_name:?}"
            );
        }
        assert!(parse("domain-2.md", VALID).is_ok());
    }

    #[test]
    fn a_file_that_is_not_utf8_is_invalid() {
        let mut contents = VALID.as_bytes().to_vec();
        contents.extend([0xff, 0xfe]);

        assert_eq!(
            Area::parse("domain.md", &contents, AreaSource::Repository),
            Err(AreaFileError::NotUtf8)
        );
    }

    #[test]
    fn a_file_larger_than_16_kb_is_invalid() {
        let at_the_limit = format!("{VALID}{}", "x".repeat(16_000 - VALID.len()));
        assert!(parse("domain.md", &at_the_limit).is_ok());

        let reason = reason("domain.md", &format!("{at_the_limit}x"));
        assert_eq!(reason, AreaFileError::TooLarge);
        assert_eq!(reason.to_string(), "the file is larger than 16 KB");
    }

    #[test]
    fn a_file_that_does_not_start_with_a_front_matter_line_is_invalid() {
        for contents in [
            "Look at the rules.\n",
            "\n+++\nname = \"Domain\"\ndescription = \"The rules\"\n+++\nLook.\n",
            "---\nname: Domain\ndescription: The rules\n---\nLook.\n",
            "",
        ] {
            assert_eq!(
                reason("domain.md", contents),
                AreaFileError::NoFrontMatter,
                "{contents:?}"
            );
        }
    }

    #[test]
    fn front_matter_without_a_closing_line_is_invalid() {
        assert_eq!(
            reason(
                "domain.md",
                "+++\nname = \"Domain\"\ndescription = \"The rules\"\nLook.\n"
            ),
            AreaFileError::UnclosedFrontMatter
        );
    }

    #[test]
    fn front_matter_that_is_not_toml_is_invalid_and_says_where() {
        let reason = reason(
            "domain.md",
            "+++\nname = \"Domain\"\ndescription = The rules\n+++\nLook.\n",
        );

        assert!(
            matches!(reason, AreaFileError::Toml { line: 3, .. }),
            "{reason:?}"
        );
        assert!(
            reason
                .to_string()
                .starts_with("the front matter is not valid TOML at line 3: "),
            "{reason}"
        );
    }

    #[test]
    fn an_area_file_may_end_its_lines_with_crlf() {
        let area = parse(
            "domain.md",
            "+++\r\nname = \"Domain\"\r\ndescription = \"The rules\"\r\n+++\r\nLook.\r\n",
        )
        .expect("the file is valid");

        assert_eq!(
            (area.name(), area.description(), area.prompt()),
            ("Domain", "The rules", "Look.")
        );
    }

    /// An area file with `front_matter` and a short prompt.
    fn with_front_matter(front_matter: &str) -> String {
        format!("+++\n{front_matter}\n+++\nLook at the rules.\n")
    }

    #[test]
    fn front_matter_without_a_name_or_a_description_is_invalid() {
        assert_eq!(
            reason(
                "domain.md",
                &with_front_matter("description = \"The rules\"")
            ),
            AreaFileError::MissingKey("name")
        );
        assert_eq!(
            reason("domain.md", &with_front_matter("name = \"Domain\"")),
            AreaFileError::MissingKey("description")
        );
    }

    #[test]
    fn front_matter_with_any_other_key_is_invalid() {
        let reason = reason(
            "domain.md",
            &with_front_matter("name = \"Domain\"\ndescription = \"The rules\"\nmodel = \"x\""),
        );

        assert_eq!(reason, AreaFileError::UnknownKey("model".to_owned()));
    }

    #[test]
    fn a_name_or_description_that_is_not_a_string_is_invalid() {
        assert_eq!(
            reason(
                "domain.md",
                &with_front_matter("name = 3\ndescription = \"The rules\"")
            ),
            AreaFileError::NotAString("name")
        );
        assert_eq!(
            reason(
                "domain.md",
                &with_front_matter("name = \"Domain\"\ndescription = [\"The rules\"]")
            ),
            AreaFileError::NotAString("description")
        );
    }

    #[test]
    fn an_empty_name_is_invalid() {
        for name in ["", "  "] {
            assert_eq!(
                reason(
                    "domain.md",
                    &with_front_matter(&format!("name = {name:?}\ndescription = \"The rules\""))
                ),
                AreaFileError::EmptyName,
                "{name:?}"
            );
        }
    }

    #[test]
    fn a_name_of_more_than_one_line_is_invalid() {
        assert_eq!(
            reason(
                "domain.md",
                &with_front_matter("name = \"Domain\\nmodel\"\ndescription = \"The rules\"")
            ),
            AreaFileError::MultilineName
        );
    }

    /// Why an area file with `description` is invalid.
    fn description_reason(description: &str) -> AreaFileError {
        reason(
            "domain.md",
            &with_front_matter(&format!("name = \"Domain\"\ndescription = {description:?}")),
        )
    }

    #[test]
    fn an_empty_description_is_invalid() {
        assert_eq!(description_reason(""), AreaFileError::EmptyDescription);
        assert_eq!(description_reason(" \t"), AreaFileError::EmptyDescription);
    }

    #[test]
    fn a_description_of_more_than_one_line_is_invalid() {
        assert_eq!(
            description_reason("The rules\nand the words"),
            AreaFileError::MultilineDescription
        );
        assert_eq!(
            description_reason("The rules\rand the words"),
            AreaFileError::MultilineDescription
        );
    }

    #[test]
    fn a_description_longer_than_200_characters_is_invalid() {
        let at_the_limit = "é".repeat(200);
        assert!(
            parse(
                "domain.md",
                &with_front_matter(&format!(
                    "name = \"Domain\"\ndescription = {at_the_limit:?}"
                ))
            )
            .is_ok()
        );

        let reason = description_reason(&format!("{at_the_limit}e"));
        assert_eq!(reason, AreaFileError::LongDescription);
        assert_eq!(
            reason.to_string(),
            "`description` is longer than 200 characters"
        );
    }

    #[test]
    fn an_empty_area_prompt_is_invalid() {
        for body in ["", "\n", "  \n\t\n"] {
            assert_eq!(
                reason(
                    "domain.md",
                    &format!("+++\nname = \"Domain\"\ndescription = \"The rules\"\n+++\n{body}")
                ),
                AreaFileError::EmptyPrompt,
                "{body:?}"
            );
        }
    }

    /// The IDs of `areas`, valid or not: an invalid file shows as its name.
    fn ids(areas: &[Result<Area, InvalidAreaFile>]) -> Vec<&str> {
        areas
            .iter()
            .map(|area| match area {
                Ok(area) => area.id().as_str(),
                Err(invalid) => invalid.file_name(),
            })
            .collect()
    }

    #[test]
    fn a_repository_without_area_files_is_offered_the_five_defaults_in_file_name_order() {
        let offered = Area::offered([]);

        assert_eq!(
            ids(&offered),
            [
                "domain",
                "infrastructure",
                "observability",
                "scalability",
                "system-design"
            ]
        );
        assert!(
            offered.iter().all(|area| area
                .as_ref()
                .is_ok_and(|area| area.source() == AreaSource::Default)),
            "{offered:#?}"
        );
        assert_eq!(
            offered.into_iter().collect::<Result<Vec<_>, _>>(),
            Ok(Area::defaults())
        );
    }

    #[test]
    fn a_repositorys_area_files_replace_the_defaults_in_file_name_order_valid_or_not() {
        let offered = Area::offered([
            ("security.md", Ok(VALID.as_bytes())),
            ("Broken.md", Ok(VALID.as_bytes())),
            ("accessibility.md", Ok(VALID.as_bytes())),
            ("domain.md", Ok(b"no front matter".as_slice())),
        ]);

        assert_eq!(
            ids(&offered),
            ["Broken.md", "accessibility", "domain.md", "security"]
        );
        assert_eq!(
            offered
                .iter()
                .map(|area| area
                    .as_ref()
                    .map(Area::source)
                    .map_err(InvalidAreaFile::reason))
                .collect::<Vec<_>>(),
            [
                Err(&AreaFileError::InvalidFileName),
                Ok(AreaSource::Repository),
                Err(&AreaFileError::NoFrontMatter),
                Ok(AreaSource::Repository),
            ]
        );
    }

    #[test]
    fn a_repository_whose_only_area_file_is_invalid_still_gets_no_defaults() {
        let offered = Area::offered([("domain.md", Ok(b"".as_slice()))]);

        assert_eq!(ids(&offered), ["domain.md"]);
    }
}
