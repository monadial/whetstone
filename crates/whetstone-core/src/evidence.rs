//! Evidence: exactly what a Practice on the learner's repository will send,
//! and what the outbound review shows beside it.
//!
//! [`Evidence::build`] is a pure function of the change, the repository's
//! exclude patterns, the selected areas and the learner's
//! [`ReviewChoices`]. The outbound review draws only from its value, and the
//! AI step will send only what it holds, so what is shown and what is sent
//! cannot diverge. A file that is left out keeps no text in it.

use std::collections::{BTreeSet, HashSet};
use std::fmt;

use crate::area::{Area, AreaId};
use crate::change::{Change, FileContent, TouchedFile};
use crate::exclusion::{DefaultExclusion, ExcludePattern, ExcludePatterns};
use crate::secret::{SecretFinding, find_secrets};

/// What a Practice sends: the commit message; for each included file, its
/// diff and its full content (the content before the change for a
/// deletion); and the selected areas with their area prompts.
///
/// Alongside it, but not sent: every touched file's status, with the reason
/// a left-out file is left out, the possible secrets found in what is sent,
/// and the total size against [`Evidence::SIZE_LIMIT`].
///
/// Its `Debug` output shows no message, file text or area prompt, so
/// logging it cannot spread a secret.
#[derive(Clone, PartialEq, Eq)]
pub struct Evidence {
    message: String,
    files: Vec<EvidenceFile>,
    areas: Vec<Area>,
    findings: Vec<Finding>,
    size: usize,
}

/// A touched file's place in its change, which identifies it in the
/// evidence and in the learner's choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileIndex(usize);

/// A file the change touches, as the evidence holds it: whether it is
/// sent, and what it sends while it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceFile {
    index: FileIndex,
    path: String,
    renamed_from: Option<String>,
    status: FileStatus,
    /// The bytes it sends while included, or a binary file's size.
    size: Option<usize>,
    /// What it sends, only while it is included.
    sent: Option<SentText>,
}

/// An included file's diff and full content.
#[derive(Clone, PartialEq, Eq)]
struct SentText {
    diff: String,
    content: String,
    side: Side,
}

/// What a touched file is, as far as sending it goes.
#[derive(Debug, Clone, Copy)]
enum Kind<'a> {
    /// Text on every side it has, with its full content from `side`.
    Text {
        content: &'a str,
        side: Side,
    },
    /// Binary on a side, of this many bytes there.
    Binary {
        size: usize,
    },
    Submodule,
    Symlink,
}

impl<'a> Kind<'a> {
    /// What `file` is: binary, a submodule or a symbolic link if either
    /// side is, and otherwise text.
    fn of(file: &'a TouchedFile) -> Self {
        let sides = || file.before().into_iter().chain(file.after());
        if let Some(size) = sides().find_map(|side| match side {
            FileContent::Binary { size } => Some(*size),
            _ => None,
        }) {
            Self::Binary { size }
        } else if sides().any(|side| *side == FileContent::Submodule) {
            Self::Submodule
        } else if sides().any(|side| *side == FileContent::Symlink) {
            Self::Symlink
        } else {
            let (content, side) = file
                .after()
                .map(|after| (after, Side::After))
                .or_else(|| file.before().map(|before| (before, Side::Before)))
                .expect("a touched file has a side before or after the change");
            Self::Text {
                content: content
                    .text()
                    .expect("a side that is not binary, a submodule or a symlink is text"),
                side,
            }
        }
    }
}

/// Which side of the change a file's full content comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Before the change: the change deletes the file.
    Before,
    /// After the change.
    After,
}

/// Whether a touched file is sent, and why. It reads as the outbound review
/// shows it:
///
/// ```
/// use whetstone_core::FileStatus;
///
/// assert_eq!(FileStatus::IncludedByYou.to_string(), "included by you");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileStatus {
    /// Sent: no rule leaves it out.
    Included,
    /// Sent, because the learner brought it back from a rule that leaves it
    /// out by default.
    IncludedByYou,
    /// Not sent: the learner left it out.
    LeftOutByYou,
    /// Not sent, for this reason.
    LeftOut(Exclusion),
}

/// Why a file is left out, by the first rule that matches it, in the order
/// they apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exclusion {
    /// Always left out: a binary file, by Git's heuristic or because it is
    /// not UTF-8 text.
    Binary,
    /// Always left out: a submodule, another repository's commit.
    Submodule,
    /// Always left out: a symbolic link, which can point outside the
    /// repository.
    Symlink,
    /// Left out by default: a lockfile or a common secret path.
    Default(DefaultExclusion),
    /// Left out by this `exclude` pattern in the repository's config.
    Config(ExcludePattern),
}

/// A possible secret in what is sent, where it is, and whether the learner
/// has resolved it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    place: Place,
    matched: SecretFinding,
    status: FindingStatus,
}

/// Where a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Place {
    /// The commit message.
    CommitMessage,
    /// The full content of this file.
    Content(FileIndex),
    /// A line the change removes, in the diff of this file. Lines the diff
    /// adds or keeps are in the file's full content, and are found there.
    Diff(FileIndex),
}

/// Whether a finding still stops **Send**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingStatus {
    /// Neither left out nor marked as not a secret.
    Unresolved,
    /// The learner marked the match as not a secret.
    NotASecret,
}

/// Why evidence cannot be sent, the first that applies, in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsendable {
    /// A possible secret is neither left out nor marked as not a secret.
    UnresolvedFindings,
    /// It is larger than [`Evidence::SIZE_LIMIT`].
    TooLarge,
    /// Every touched file is left out.
    NothingToSend,
}

/// The learner's choices on the outbound review of one change: the files
/// they left out or brought back, and the matches they marked as not
/// secrets. They live only in memory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewChoices {
    left_out: BTreeSet<FileIndex>,
    brought_back: BTreeSet<FileIndex>,
    not_secrets: HashSet<(Place, SecretFinding)>,
}

impl Evidence {
    /// The most bytes a Practice sends: roughly 100k tokens, well within
    /// Jev 1.14's context window of 262,144 tokens.
    pub const SIZE_LIMIT: usize = 400_000;

    /// The evidence for `change`, leaving out what `exclude` and the rules
    /// before it leave out, with the selected `areas`, as `choices` adjust
    /// it.
    ///
    /// Rules leave a file out in this order, and the first that matches
    /// gives the reason: binary files, submodules and symbolic links,
    /// always; lockfiles and common secret paths, by default; the
    /// repository's `exclude` patterns; and the learner. The learner can
    /// bring back a file the second or third leaves out.
    ///
    /// The commit message and every file sent are checked for secrets, so
    /// a file brought back is checked as it returns.
    #[must_use]
    pub fn build(
        change: &Change,
        exclude: &ExcludePatterns,
        areas: &[Area],
        choices: &ReviewChoices,
    ) -> Self {
        let files: Vec<EvidenceFile> = change
            .files()
            .iter()
            .zip(change.file_diffs())
            .enumerate()
            .map(|(index, (file, diff))| {
                EvidenceFile::new(FileIndex(index), file, diff, exclude, choices)
            })
            .collect();
        let mut found: Vec<(Place, SecretFinding)> = find_secrets(change.message())
            .into_iter()
            .map(|matched| (Place::CommitMessage, matched))
            .collect();
        for file in &files {
            let Some(sent) = &file.sent else {
                continue;
            };
            found.extend(
                find_secrets(&sent.content)
                    .into_iter()
                    .map(|matched| (Place::Content(file.index), matched)),
            );
            // A deleted file's content holds every line its diff removes.
            if sent.side == Side::After {
                let removed = removed_lines(&sent.diff);
                found.extend(
                    find_secrets(&sent.diff)
                        .into_iter()
                        .filter(|matched| removed.contains(&matched.line()))
                        .map(|matched| (Place::Diff(file.index), matched)),
                );
            }
        }
        let findings = found
            .into_iter()
            .map(|(place, matched)| {
                let status = if choices.not_secrets.contains(&(place, matched.clone())) {
                    FindingStatus::NotASecret
                } else {
                    FindingStatus::Unresolved
                };
                Finding {
                    place,
                    matched,
                    status,
                }
            })
            .collect();
        let size = change.message().len()
            + files
                .iter()
                .filter(|file| file.sent.is_some())
                .filter_map(|file| file.size)
                .sum::<usize>()
            + areas
                .iter()
                .map(|area| area.name().len() + area.prompt().len())
                .sum::<usize>();
        Self {
            message: change.message().to_owned(),
            files,
            areas: areas.to_vec(),
            findings,
            size,
        }
    }

    /// The commit message, as it is sent.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Every file the change touches, in its order, sent or not.
    #[must_use]
    pub fn files(&self) -> &[EvidenceFile] {
        &self.files
    }

    /// The touched file at `index`.
    #[must_use]
    pub fn file(&self, index: FileIndex) -> Option<&EvidenceFile> {
        self.files.get(index.0)
    }

    /// The selected areas, whose names and area prompts are sent.
    #[must_use]
    pub fn areas(&self) -> &[Area] {
        &self.areas
    }

    /// The possible secrets in what is sent, the commit message's first,
    /// then each file's in order.
    #[must_use]
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// How many bytes are sent: every byte of the message, of each
    /// included file's path, diff and full content, and of each selected
    /// area's name and prompt.
    #[must_use]
    pub fn size(&self) -> usize {
        self.size
    }

    /// Why the evidence cannot be sent, if it cannot: unresolved findings
    /// first, then its size, then having nothing to send.
    #[must_use]
    pub fn unsendable(&self) -> Option<Unsendable> {
        if self
            .findings
            .iter()
            .any(|finding| finding.status == FindingStatus::Unresolved)
        {
            Some(Unsendable::UnresolvedFindings)
        } else if self.size > Self::SIZE_LIMIT {
            Some(Unsendable::TooLarge)
        } else if self.files.iter().all(|file| file.sent.is_none()) {
            Some(Unsendable::NothingToSend)
        } else {
            None
        }
    }
}

/// Shows the message's length, the areas by ID and the files by status,
/// but never a message, file text or area prompt.
impl fmt::Debug for Evidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Evidence")
            .field("message_bytes", &self.message.len())
            .field("files", &self.files)
            .field(
                "areas",
                &self.areas.iter().map(Area::id).collect::<Vec<&AreaId>>(),
            )
            .field("findings", &self.findings)
            .field("size", &self.size)
            .finish()
    }
}

/// The 1-based lines of `diff` that its hunks remove.
fn removed_lines(diff: &str) -> HashSet<usize> {
    diff.lines()
        .enumerate()
        .skip_while(|(_, line)| !line.starts_with("@@"))
        .filter(|(_, line)| line.starts_with('-'))
        .map(|(index, _)| index + 1)
        .collect()
}

/// What an included file sends.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SentFile<'a> {
    /// The file's diff.
    pub diff: &'a str,
    /// The file's full content.
    pub content: &'a str,
    /// Which side of the change the content comes from.
    pub side: Side,
}

/// Shows the text's lengths, never the text.
impl fmt::Debug for SentFile<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SentFile")
            .field("diff_bytes", &self.diff.len())
            .field("content_bytes", &self.content.len())
            .field("side", &self.side)
            .finish()
    }
}

/// Shows the text's lengths, never the text.
impl fmt::Debug for SentText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SentText")
            .field("diff_bytes", &self.diff.len())
            .field("content_bytes", &self.content.len())
            .field("side", &self.side)
            .finish()
    }
}

impl EvidenceFile {
    /// The touched `file` at `index`, with its `diff`, as the rules and
    /// `choices` leave it.
    fn new(
        index: FileIndex,
        file: &TouchedFile,
        diff: &str,
        exclude: &ExcludePatterns,
        choices: &ReviewChoices,
    ) -> Self {
        let path = file.path();
        let kind = Kind::of(file);
        let status = match kind {
            Kind::Binary { .. } => FileStatus::LeftOut(Exclusion::Binary),
            Kind::Submodule => FileStatus::LeftOut(Exclusion::Submodule),
            Kind::Symlink => FileStatus::LeftOut(Exclusion::Symlink),
            Kind::Text { .. } => {
                let rule = DefaultExclusion::of(path)
                    .map(Exclusion::Default)
                    .or_else(|| exclude.first_match(path).cloned().map(Exclusion::Config));
                match rule {
                    Some(_) if choices.brought_back.contains(&index) => FileStatus::IncludedByYou,
                    Some(reason) => FileStatus::LeftOut(reason),
                    None if choices.left_out.contains(&index) => FileStatus::LeftOutByYou,
                    None => FileStatus::Included,
                }
            }
        };
        let size = match kind {
            Kind::Text { content, .. } => Some(path.len() + diff.len() + content.len()),
            Kind::Binary { size } => Some(size),
            Kind::Submodule | Kind::Symlink => None,
        };
        let sent = match kind {
            Kind::Text { content, side }
                if matches!(status, FileStatus::Included | FileStatus::IncludedByYou) =>
            {
                Some(SentText {
                    diff: diff.to_owned(),
                    content: content.to_owned(),
                    side,
                })
            }
            _ => None,
        };
        Self {
            index,
            path: path.to_owned(),
            renamed_from: file.renamed_from().map(str::to_owned),
            status,
            size,
            sent,
        }
    }

    /// Its place in the change.
    #[must_use]
    pub fn index(&self) -> FileIndex {
        self.index
    }

    /// Where the file is, relative to the repository's root: after the
    /// change, or before it if the change deletes it.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Where the file was before the change renamed it, if it did.
    #[must_use]
    pub fn renamed_from(&self) -> Option<&str> {
        self.renamed_from.as_deref()
    }

    /// Whether it is sent, and why.
    #[must_use]
    pub fn status(&self) -> &FileStatus {
        &self.status
    }

    /// The bytes it sends while included, its path, diff and full content;
    /// a binary file's size; or nothing for a submodule or a symbolic link,
    /// which have no content.
    #[must_use]
    pub fn size(&self) -> Option<usize> {
        self.size
    }

    /// What it sends, if it is included.
    #[must_use]
    pub fn sent(&self) -> Option<SentFile<'_>> {
        self.sent.as_ref().map(|sent| SentFile {
            diff: &sent.diff,
            content: &sent.content,
            side: sent.side,
        })
    }
}

impl fmt::Display for FileStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Included => f.write_str("included"),
            Self::IncludedByYou => f.write_str("included by you"),
            Self::LeftOutByYou => f.write_str("left out by you"),
            Self::LeftOut(reason) => write!(f, "left out: {reason}"),
        }
    }
}

impl Exclusion {
    /// Whether the learner can bring back a file it leaves out: one left
    /// out by default or by the repository's config, but never a binary
    /// file, a submodule or a symbolic link.
    #[must_use]
    pub fn can_bring_back(&self) -> bool {
        matches!(self, Self::Default(_) | Self::Config(_))
    }
}

/// The reason as the outbound review shows it: "binary file", "lockfile",
/// or "exclude pattern fixtures/".
impl fmt::Display for Exclusion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binary => f.write_str("binary file"),
            Self::Submodule => f.write_str("submodule"),
            Self::Symlink => f.write_str("symlink"),
            Self::Default(exclusion) => f.write_str(exclusion.kind().reason()),
            Self::Config(pattern) => write!(f, "exclude pattern {}", pattern.as_str()),
        }
    }
}

impl Finding {
    /// Where it is.
    #[must_use]
    pub fn place(&self) -> Place {
        self.place
    }

    /// The match: its line, the rule that matched and the masked excerpt.
    #[must_use]
    pub fn matched(&self) -> &SecretFinding {
        &self.matched
    }

    /// Whether it still stops **Send**.
    #[must_use]
    pub fn status(&self) -> FindingStatus {
        self.status
    }
}

impl Place {
    /// The file it is in, unless it is in the commit message.
    #[must_use]
    pub fn file(self) -> Option<FileIndex> {
        match self {
            Self::CommitMessage => None,
            Self::Content(file) | Self::Diff(file) => Some(file),
        }
    }
}

impl ReviewChoices {
    /// Leaves out the file at `index` of `evidence` if it is sent, or
    /// brings it back if it is left out and can be.
    pub fn toggle_file(&mut self, evidence: &Evidence, index: FileIndex) {
        match evidence.file(index).map(EvidenceFile::status) {
            Some(FileStatus::Included | FileStatus::IncludedByYou) => self.leave_out(index),
            Some(FileStatus::LeftOutByYou) => {
                self.left_out.remove(&index);
            }
            Some(FileStatus::LeftOut(reason)) if reason.can_bring_back() => {
                self.brought_back.insert(index);
            }
            Some(FileStatus::LeftOut(_)) | None => {}
        }
    }

    /// Leaves out the file at `index`: one the learner brought back returns
    /// to the rule that left it out, and any other is left out by them.
    pub fn leave_out(&mut self, index: FileIndex) {
        if !self.brought_back.remove(&index) {
            self.left_out.insert(index);
        }
    }

    /// Marks `finding`'s match as not a secret, or unmarks it.
    pub fn toggle_not_a_secret(&mut self, finding: &Finding) {
        let key = (finding.place, finding.matched.clone());
        if !self.not_secrets.remove(&key) {
            self.not_secrets.insert(key);
        }
    }
}
