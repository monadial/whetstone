//! Evidence: exactly what a Practice on the learner's repository will send,
//! and what the outbound review shows beside it.
//!
//! [`Evidence::build`] is a pure function of the change, the repository's
//! exclude patterns, the selected areas, the change's connections (or why
//! there are none), the connected code limit and the learner's
//! [`ReviewChoices`]. The outbound review draws only from its value, and
//! the AI step will send only what it holds, so what is shown and what is
//! sent cannot diverge. A file or connection that is left out keeps no text
//! in it.

use std::collections::{BTreeSet, HashSet};
use std::fmt;

use crate::area::{Area, AreaId};
use crate::change::{Change, FileContent, TouchedFile};
use crate::connected_code_limit::ConnectedCodeLimit;
use crate::connection::{Connection, ConnectionGroup, ConnectionId, Connections, Relation, Tier};
use crate::exclusion::{DefaultExclusion, ExcludePattern, ExcludePatterns};
use crate::secret::{SecretFinding, Severity, find_secrets};
use crate::selection::{self, NotSent};

/// What a Practice sends: the commit message; for each included file, its
/// diff and its full content (the content before the change for a
/// deletion); the selected areas with their area prompts; and each included
/// connection's lines or whole file.
///
/// Alongside it, but not sent: every touched file's and connection's
/// status, with the reason one left out is left out, whether connections
/// were found, how many found connections did not fit and the connected
/// code limit they were selected within, the possible secrets found in
/// what is sent, and the total size against [`Evidence::SIZE_LIMIT`].
///
/// Its `Debug` output shows no message, file text, area prompt or callable
/// name, so logging it cannot spread a secret.
#[derive(Clone, PartialEq, Eq)]
pub struct Evidence {
    message: String,
    files: Vec<EvidenceFile>,
    areas: Vec<Area>,
    connections: Vec<EvidenceConnection>,
    connections_state: ConnectionsState,
    connected_code_limit: ConnectedCodeLimit,
    not_sent: Vec<(ConnectionGroup, NotSent)>,
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
    /// How many connections of connected code relate to it.
    connected: usize,
}

/// A connection, as the evidence holds it: whether it is sent, and what it
/// sends while it is.
#[derive(Clone, PartialEq, Eq)]
pub struct EvidenceConnection {
    id: ConnectionId,
    group: ConnectionGroup,
    tier: Tier,
    relations: Vec<Relation>,
    status: FileStatus,
    /// The bytes it sends while included, or a binary file's size.
    size: Option<usize>,
    /// What it sends, only while it is included.
    sent: Option<String>,
}

/// Whether the evidence has connections, or why it has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionsState {
    /// They are still being found.
    Finding,
    /// It has at least one.
    Found,
    /// None were found for this change.
    NoneFound,
    /// Finding them took longer than the time limit allows.
    TookTooLong,
    /// The analysis that finds them failed.
    AnalysisFailed,
}

/// An included file's diff and full content.
#[derive(Clone, PartialEq, Eq)]
struct SentText {
    diff: String,
    content: String,
    side: Side,
}

/// What a touched file is, as far as sending it goes.
#[derive(Clone, Copy)]
enum Kind<'a> {
    /// Text on every side it has, with its full content from `side`.
    Text {
        content: &'a str,
        side: Side,
    },
    /// Binary on a side, of this many bytes there: after the change if it
    /// is binary then.
    Binary {
        size: usize,
    },
    Submodule,
    Symlink,
}

/// Shows the content's length, never its text.
impl fmt::Debug for Kind<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text { content, side } => f
                .debug_struct("Text")
                .field("content_bytes", &content.len())
                .field("side", side)
                .finish(),
            Self::Binary { size } => f.debug_struct("Binary").field("size", size).finish(),
            Self::Submodule => f.write_str("Submodule"),
            Self::Symlink => f.write_str("Symlink"),
        }
    }
}

impl<'a> Kind<'a> {
    /// What `file` is: binary, a submodule or a symbolic link if either
    /// side is, and otherwise text.
    fn of(file: &'a TouchedFile) -> Self {
        let sides = || file.after().into_iter().chain(file.before());
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Place {
    /// The commit message.
    CommitMessage,
    /// The full content of this file.
    Content(FileIndex),
    /// A line the change removes, in the diff of this file. Lines the diff
    /// adds or keeps are in the file's full content, and are found there.
    Diff(FileIndex),
    /// What this connection sends. Its line is the line in the
    /// connection's file.
    Connection(ConnectionId),
    /// The name of the selected area with this ID.
    AreaName(AreaId),
    /// The description of the selected area with this ID.
    AreaDescription(AreaId),
    /// The area prompt of the selected area with this ID. Its line is the
    /// line in the prompt, as it is sent.
    AreaPrompt(AreaId),
}

/// Whether the learner has resolved a finding. One that blocks **Send**
/// stops it until they have; a warning never does.
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
    /// A possible secret that blocks **Send** is neither left out nor
    /// marked as not a secret.
    UnresolvedFindings,
    /// It is larger than [`Evidence::SIZE_LIMIT`].
    TooLarge,
    /// Every touched file is left out. Connections are sent only beside
    /// the change, never instead of it.
    NothingToSend,
}

/// The learner's choices on the outbound review of one change: the files
/// and connections they left out or brought back, and the matches they
/// marked as not secrets. A file is known by its place in the change, and a
/// connection by its path and extent, so a choice about one holds however
/// the connections arrive. They live only in memory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewChoices {
    left_out: BTreeSet<Item>,
    brought_back: BTreeSet<Item>,
    not_secrets: HashSet<(Place, SecretFinding)>,
}

/// What the learner leaves out or brings back: a touched file, by its place
/// in the change, or a connection, by its path and extent.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Item {
    File(FileIndex),
    Connection(ConnectionId),
}

impl Evidence {
    /// The most bytes a Practice sends: roughly 100k tokens, well within
    /// Jev 1.14's context window of 262,144 tokens.
    pub const SIZE_LIMIT: usize = 400_000;

    /// The evidence for `change`, leaving out what `exclude` and the rules
    /// before it leave out, with the selected `areas` and the change's
    /// `connections`, selected within `connected_code_limit`, as `choices`
    /// adjust it.
    ///
    /// Rules leave a file or a connection out in this order, and the first
    /// that matches gives the reason: binary files, submodules and symbolic
    /// links, always; lockfiles and common secret paths, by default; the
    /// repository's `exclude` patterns; and the learner. The learner can
    /// bring back one the second or third leaves out. A renamed file is
    /// matched by its old path as well as its new one.
    ///
    /// The commit message, every file sent, every connection sent and each
    /// selected area's name, description and prompt are checked for
    /// secrets, so a file or connection brought back is checked as it
    /// returns.
    /// Connections are listed from the most relevant to the least: related
    /// tests and related configuration first, by path; then connected code,
    /// the high tier before the medium, then the closest to a touched file
    /// it connects to first, by the leading path components they share;
    /// then by path and starting line. Found connections that are empty are
    /// [`ConnectionsState::NoneFound`].
    ///
    /// A connection a rule leaves out is listed with its reason and uses no
    /// room. Each other is added, in rank order, while it fits in the room
    /// left under [`Evidence::SIZE_LIMIT`] after the commit message, the
    /// touched files the rules send and the selected areas, and if it is
    /// connected code, within `connected_code_limit`. One that does not fit
    /// is not listed, only counted in [`Evidence::not_sent`], and smaller
    /// ones after it can still be added.
    ///
    /// The selection reads none of `choices`, so it is the same in every
    /// build of one change's evidence: the first build fixes it. Leaving a
    /// connection or a touched file out frees room without adding anything
    /// else, and a connection brought back is sent beside those selected,
    /// whatever room it takes.
    #[must_use]
    pub fn build(
        change: &Change,
        exclude: &ExcludePatterns,
        areas: &[Area],
        connections: &Connections,
        connected_code_limit: ConnectedCodeLimit,
        choices: &ReviewChoices,
    ) -> Self {
        let mut files: Vec<EvidenceFile> = change
            .files()
            .iter()
            .zip(change.file_diffs())
            .enumerate()
            .map(|(index, (file, diff))| {
                EvidenceFile::new(FileIndex(index), file, diff, exclude, choices)
            })
            .collect();
        let areas_size: usize = areas
            .iter()
            .map(|area| area.name().len() + area.prompt().len())
            .sum();
        let room = room(change.message(), &files, areas_size);
        let (connections, connections_state, not_sent) =
            EvidenceConnection::all(connections, exclude, room, connected_code_limit, choices);
        for file in &mut files {
            file.connected = connections
                .iter()
                .filter(|connection| connection.is_connected_code_for(&file.path))
                .count();
        }
        let findings = findings(change.message(), &files, &connections, areas, choices);
        let size = change.message().len()
            + files
                .iter()
                .filter(|file| file.sent.is_some())
                .filter_map(|file| file.size)
                .sum::<usize>()
            + connections
                .iter()
                .filter(|connection| connection.sent.is_some())
                .filter_map(|connection| connection.size)
                .sum::<usize>()
            + areas_size;
        Self {
            message: change.message().to_owned(),
            files,
            areas: areas.to_vec(),
            connections,
            connections_state,
            connected_code_limit,
            not_sent,
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

    /// Every connection found, from the most relevant to the least, sent or
    /// not.
    #[must_use]
    pub fn connections(&self) -> &[EvidenceConnection] {
        &self.connections
    }

    /// Whether it has connections, or why it has none.
    #[must_use]
    pub fn connections_state(&self) -> ConnectionsState {
        self.connections_state
    }

    /// The connected code limit its connections were selected within, and
    /// where that limit comes from, such as this repository lowering it.
    #[must_use]
    pub fn connected_code_limit(&self) -> ConnectedCodeLimit {
        self.connected_code_limit
    }

    /// How many bytes of connected code are sent: the path and sent text of
    /// each connection of connected code that is included. Related tests
    /// and related configuration are not connected code, and do not count
    /// towards its limit.
    #[must_use]
    pub fn connected_code_size(&self) -> usize {
        self.connections
            .iter()
            .filter(|connection| connection.group == ConnectionGroup::ConnectedCode)
            .filter(|connection| connection.sent.is_some())
            .filter_map(|connection| connection.size)
            .sum()
    }

    /// How many found connections in any of `groups` that no rule leaves
    /// out are not sent, because they did not fit, for each reason that has
    /// any: over the connected code limit first, then no room under the size
    /// limit. They are not in [`Evidence::connections`]. Only connected code
    /// can be over its limit.
    #[must_use]
    pub fn not_sent(&self, groups: &[ConnectionGroup]) -> Vec<(NotSent, usize)> {
        [NotSent::OverConnectedCodeLimit, NotSent::NoRoom]
            .into_iter()
            .map(|reason| {
                let count = self
                    .not_sent
                    .iter()
                    .filter(|(group, not_sent)| *not_sent == reason && groups.contains(group))
                    .count();
                (reason, count)
            })
            .filter(|&(_, count)| count > 0)
            .collect()
    }

    /// The possible secrets in what is sent, the commit message's first,
    /// then each file's in order, then each connection's in order, then each
    /// area's in order.
    #[must_use]
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// How many bytes are sent: every byte of the message, of each
    /// included file's path, diff and full content, of each included
    /// connection's path and sent text, and of each selected area's name and
    /// prompt.
    #[must_use]
    pub fn size(&self) -> usize {
        self.size
    }

    /// Why the evidence cannot be sent, if it cannot: unresolved findings
    /// that block **Send** first, then its size, then having nothing to
    /// send. A warning never stops it.
    #[must_use]
    pub fn unsendable(&self) -> Option<Unsendable> {
        if self.findings.iter().any(|finding| {
            finding.status == FindingStatus::Unresolved && finding.severity() == Severity::Blocking
        }) {
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
            .field("connections", &self.connections)
            .field("connections_state", &self.connections_state)
            .field("connected_code_limit", &self.connected_code_limit)
            .field("not_sent", &self.not_sent)
            .field("findings", &self.findings)
            .field("size", &self.size)
            .finish()
    }
}

/// The room for connections under [`Evidence::SIZE_LIMIT`]: what is left
/// after `message`, the touched `files` the rules alone send and
/// `areas_size` bytes of areas. It counts what the rules send whatever the
/// learner chooses, so that it is the same in every build.
fn room(message: &str, files: &[EvidenceFile], areas_size: usize) -> usize {
    let files_size: usize = files
        .iter()
        .filter(|file| file.status.sent_by_rules())
        .filter_map(|file| file.size)
        .sum();
    Evidence::SIZE_LIMIT.saturating_sub(message.len() + files_size + areas_size)
}

/// The possible secrets in what is sent: in `message`, in each included
/// file of `files` and connection of `connections`, and in each of `areas`'
/// name, description and prompt, in that order, each resolved or not as
/// `choices` say.
fn findings(
    message: &str,
    files: &[EvidenceFile],
    connections: &[EvidenceConnection],
    areas: &[Area],
    choices: &ReviewChoices,
) -> Vec<Finding> {
    let mut found: Vec<(Place, SecretFinding)> = find_secrets(message)
        .into_iter()
        .map(|matched| (Place::CommitMessage, matched))
        .collect();
    for file in files {
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
    for connection in connections {
        let Some(sent) = connection.sent() else {
            continue;
        };
        found.extend(find_secrets(sent.text).into_iter().map(|matched| {
            (
                Place::Connection(connection.id.clone()),
                matched.lines_down(sent.first_line - 1),
            )
        }));
    }
    for area in areas {
        let id = area.id();
        let texts = [
            (Place::AreaName(id.clone()), area.name()),
            (Place::AreaDescription(id.clone()), area.description()),
            (Place::AreaPrompt(id.clone()), area.prompt()),
        ];
        for (place, text) in texts {
            found.extend(
                find_secrets(text)
                    .into_iter()
                    .map(|matched| (place.clone(), matched)),
            );
        }
    }
    found
        .into_iter()
        .map(|key| {
            let status = if choices.not_secrets.contains(&key) {
                FindingStatus::NotASecret
            } else {
                FindingStatus::Unresolved
            };
            let (place, matched) = key;
            Finding {
                place,
                matched,
                status,
            }
        })
        .collect()
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

/// What an included connection sends.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SentConnection<'a> {
    /// The lines of its extent, with their own line endings.
    pub text: &'a str,
    /// The number in its file of the text's first line, which the reader
    /// shows it from.
    pub first_line: usize,
}

/// Shows the text's length, never the text.
impl fmt::Debug for SentConnection<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SentConnection")
            .field("text_bytes", &self.text.len())
            .field("first_line", &self.first_line)
            .finish()
    }
}

/// Shows the sent text's length, never the text, and its relations without
/// a callable's name.
impl fmt::Debug for EvidenceConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EvidenceConnection")
            .field("id", &self.id)
            .field("group", &self.group)
            .field("tier", &self.tier)
            .field("relations", &self.relations)
            .field("status", &self.status)
            .field("size", &self.size)
            .field("sent_bytes", &self.sent.as_ref().map(String::len))
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
                // A file moved away from a path left out is left out too:
                // its content is what was there.
                let paths: Vec<&str> = std::iter::once(path).chain(file.renamed_from()).collect();
                text_status(&paths, exclude, choices, &Item::File(index))
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
            connected: 0,
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

    /// How many connections of connected code relate to it, sent or not.
    /// Related tests and related configuration are not counted.
    #[must_use]
    pub fn connected(&self) -> usize {
        self.connected
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

/// Whether text at `paths` is sent: left out by the first rule that matches
/// one of them, a default exclusion and then an `exclude` pattern, unless
/// the learner brought `item` back; otherwise sent, unless the learner left
/// it out.
fn text_status(
    paths: &[&str],
    exclude: &ExcludePatterns,
    choices: &ReviewChoices,
    item: &Item,
) -> FileStatus {
    let rule = paths
        .iter()
        .copied()
        .find_map(DefaultExclusion::of)
        .map(Exclusion::Default)
        .or_else(|| {
            paths
                .iter()
                .find_map(|path| exclude.first_match(path))
                .cloned()
                .map(Exclusion::Config)
        });
    match rule {
        Some(_) if choices.brought_back.contains(item) => FileStatus::IncludedByYou,
        Some(reason) => FileStatus::LeftOut(reason),
        None if choices.left_out.contains(item) => FileStatus::LeftOutByYou,
        None => FileStatus::Included,
    }
}

impl EvidenceConnection {
    /// The evidence's connections from `connections`, ranked, as the rules
    /// and `choices` leave them, and whether it has any.
    fn all(
        connections: &Connections,
        exclude: &ExcludePatterns,
        room: usize,
        limit: ConnectedCodeLimit,
        choices: &ReviewChoices,
    ) -> (Vec<Self>, ConnectionsState, Vec<(ConnectionGroup, NotSent)>) {
        let state = match connections {
            Connections::Found(found) if !found.is_empty() => {
                let ranked = selection::rank(found)
                    .into_iter()
                    .map(|connection| (connection, Self::status_of(connection, exclude, choices)))
                    .collect();
                let selection = selection::select(ranked, room, limit);
                let listed = selection
                    .listed
                    .into_iter()
                    .map(|(connection, status)| Self::new(connection, status))
                    .collect();
                return (listed, ConnectionsState::Found, selection.not_sent);
            }
            Connections::Found(_) | Connections::NoneFound => ConnectionsState::NoneFound,
            Connections::Finding => ConnectionsState::Finding,
            Connections::TookTooLong => ConnectionsState::TookTooLong,
            Connections::AnalysisFailed => ConnectionsState::AnalysisFailed,
        };
        (Vec::new(), state, Vec::new())
    }

    /// Whether the found `connection` is sent, and why, as the rules and
    /// `choices` leave it.
    fn status_of(
        connection: &Connection,
        exclude: &ExcludePatterns,
        choices: &ReviewChoices,
    ) -> FileStatus {
        let id = connection.id();
        match connection.content() {
            FileContent::Binary { .. } => FileStatus::LeftOut(Exclusion::Binary),
            FileContent::Submodule => FileStatus::LeftOut(Exclusion::Submodule),
            FileContent::Symlink => FileStatus::LeftOut(Exclusion::Symlink),
            FileContent::Text(_) => text_status(
                &[id.path()],
                exclude,
                choices,
                &Item::Connection(id.clone()),
            ),
        }
    }

    /// The found `connection`, whose status is `status`.
    fn new(connection: &Connection, status: FileStatus) -> Self {
        let sent = match connection.content() {
            FileContent::Text(text)
                if matches!(status, FileStatus::Included | FileStatus::IncludedByYou) =>
            {
                Some(text.clone())
            }
            _ => None,
        };
        Self {
            id: connection.id().clone(),
            group: connection.group(),
            tier: connection.tier(),
            relations: connection.relations().to_vec(),
            status,
            size: connection.size(),
            sent,
        }
    }

    /// Its path and extent, which identify it.
    #[must_use]
    pub fn id(&self) -> &ConnectionId {
        &self.id
    }

    /// Which kind of material it is.
    #[must_use]
    pub fn group(&self) -> ConnectionGroup {
        self.group
    }

    /// How sure Whetstone is of it.
    #[must_use]
    pub fn tier(&self) -> Tier {
        self.tier
    }

    /// Why it is there.
    #[must_use]
    pub fn relations(&self) -> &[Relation] {
        &self.relations
    }

    /// Whether it is connected code with a relation to the touched file at
    /// `path`.
    fn is_connected_code_for(&self, path: &str) -> bool {
        self.group == ConnectionGroup::ConnectedCode
            && self
                .relations
                .iter()
                .any(|relation| relation.touched_file() == path)
    }

    /// Whether it is sent, and why, as for a touched file.
    #[must_use]
    pub fn status(&self) -> &FileStatus {
        &self.status
    }

    /// The bytes it sends while included, its path and sent text; a binary
    /// file's size; or nothing for a submodule or a symbolic link, which
    /// have no content.
    #[must_use]
    pub fn size(&self) -> Option<usize> {
        self.size
    }

    /// What it sends, if it is included.
    #[must_use]
    pub fn sent(&self) -> Option<SentConnection<'_>> {
        self.sent.as_deref().map(|text| SentConnection {
            text,
            first_line: self.id.extent().first_line(),
        })
    }
}

impl fmt::Display for FileStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Included => f.write_str("included"),
            Self::IncludedByYou => f.write_str("included by you"),
            Self::LeftOutByYou => f.write_str("left out by you"),
            Self::LeftOut(Exclusion::Config(pattern)) => {
                write!(f, "left out by repository config: {}", pattern.as_str())
            }
            Self::LeftOut(reason) => write!(f, "left out: {reason}"),
        }
    }
}

impl FileStatus {
    /// Whether the rules alone send it, before the learner's choices: it is
    /// included, or the learner left it out. The learner's choices never
    /// change this, since they only leave out what the rules send and bring
    /// back what the rules leave out.
    pub(crate) fn sent_by_rules(&self) -> bool {
        matches!(self, Self::Included | Self::LeftOutByYou)
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

/// The reason on its own: "binary file", "lockfile" or "exclude pattern
/// fixtures/". [`FileStatus`] shows it after "left out:", but a file an
/// `exclude` pattern leaves out as "left out by repository config:
/// fixtures/".
impl fmt::Display for Exclusion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binary => f.write_str("binary file"),
            Self::Submodule => f.write_str("submodule"),
            Self::Symlink => f.write_str("symlink"),
            Self::Default(exclusion) => exclusion.kind().fmt(f),
            Self::Config(pattern) => write!(f, "exclude pattern {}", pattern.as_str()),
        }
    }
}

impl Finding {
    /// Where it is.
    #[must_use]
    pub fn place(&self) -> &Place {
        &self.place
    }

    /// The match: its line, the rule that matched and the masked excerpt.
    #[must_use]
    pub fn matched(&self) -> &SecretFinding {
        &self.matched
    }

    /// Whether the learner has resolved it.
    #[must_use]
    pub fn status(&self) -> FindingStatus {
        self.status
    }

    /// What it does to **Send** while it is unresolved: its rule's
    /// severity.
    #[must_use]
    pub fn severity(&self) -> Severity {
        self.matched.rule().severity()
    }
}

impl Place {
    /// The touched file it is in, if it is in one.
    #[must_use]
    pub fn file(&self) -> Option<FileIndex> {
        match self {
            Self::CommitMessage
            | Self::Connection(_)
            | Self::AreaName(_)
            | Self::AreaDescription(_)
            | Self::AreaPrompt(_) => None,
            Self::Content(file) | Self::Diff(file) => Some(*file),
        }
    }

    /// The connection it is in, if it is in one.
    #[must_use]
    pub fn connection(&self) -> Option<&ConnectionId> {
        match self {
            Self::Connection(connection) => Some(connection),
            Self::CommitMessage
            | Self::Content(_)
            | Self::Diff(_)
            | Self::AreaName(_)
            | Self::AreaDescription(_)
            | Self::AreaPrompt(_) => None,
        }
    }
}

impl ReviewChoices {
    /// Leaves out the file at `index` of `evidence` if it is sent, or
    /// brings it back if it is left out and can be.
    pub fn toggle_file(&mut self, evidence: &Evidence, index: FileIndex) {
        self.toggle(
            Item::File(index),
            evidence.file(index).map(EvidenceFile::status),
        );
    }

    /// Leaves out the file at `index`: one the learner brought back returns
    /// to the rule that left it out, and any other is left out by them.
    pub fn leave_out(&mut self, index: FileIndex) {
        self.leave_out_item(Item::File(index));
    }

    /// Leaves out `connection` if it is sent, or brings it back if it is
    /// left out and can be. The choice holds for any connection with the
    /// same path and extent.
    pub fn toggle_connection(&mut self, connection: &EvidenceConnection) {
        self.toggle(
            Item::Connection(connection.id.clone()),
            Some(&connection.status),
        );
    }

    /// Leaves out the connection `id` names: one the learner brought back
    /// returns to the rule that left it out, and any other is left out by
    /// them.
    pub fn leave_out_connection(&mut self, id: &ConnectionId) {
        self.leave_out_item(Item::Connection(id.clone()));
    }

    /// Leaves out `item`, whose status is `status`, if it is sent, or brings
    /// it back if it is left out and can be.
    fn toggle(&mut self, item: Item, status: Option<&FileStatus>) {
        match status {
            Some(FileStatus::Included | FileStatus::IncludedByYou) => self.leave_out_item(item),
            Some(FileStatus::LeftOutByYou) => {
                self.left_out.remove(&item);
            }
            Some(FileStatus::LeftOut(reason)) if reason.can_bring_back() => {
                self.brought_back.insert(item);
            }
            Some(FileStatus::LeftOut(_)) | None => {}
        }
    }

    /// Leaves out `item`: one the learner brought back returns to the rule
    /// that left it out, and any other is left out by them.
    fn leave_out_item(&mut self, item: Item) {
        if !self.brought_back.remove(&item) {
            self.left_out.insert(item);
        }
    }

    /// Marks `finding`'s match as not a secret, or unmarks it.
    pub fn toggle_not_a_secret(&mut self, finding: &Finding) {
        let key = (finding.place.clone(), finding.matched.clone());
        if !self.not_secrets.remove(&key) {
            self.not_secrets.insert(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change::{CommitId, Comparison};
    use crate::connection::{Connection, ConnectionGroup, Connections, Extent, LineRange, Tier};
    use crate::secret::Severity;

    /// A commit that edits `src/pay.rs` and `src/refund.rs`.
    fn change() -> Change {
        Change::from_commit(
            CommitId::new("419ace2d5c0e1f7b8a9d3e4f5a6b7c8d9e0f1a2b", 7).expect("the ID is valid"),
            Comparison::Parent,
            "Retry failed payments".to_owned(),
            "diff --git a/src/pay.rs b/src/pay.rs\n@@ -1 +1 @@\n-fn pay() {}\n+fn pay() { retry() }\n\
             diff --git a/src/refund.rs b/src/refund.rs\n@@ -1 +1 @@\n-fn refund() {}\n+fn refund() { pay() }\n"
                .to_owned(),
            vec![
                TouchedFile::modified(
                    "src/pay.rs".to_owned(),
                    FileContent::Text("fn pay() {}\n".to_owned()),
                    FileContent::Text("fn pay() { retry() }\n".to_owned()),
                ),
                TouchedFile::modified(
                    "src/refund.rs".to_owned(),
                    FileContent::Text("fn refund() {}\n".to_owned()),
                    FileContent::Text("fn refund() { pay() }\n".to_owned()),
                ),
            ],
        )
    }

    /// Connected code: the whole of `path`, holding `text`, which calls
    /// `pay` in `src/pay.rs`.
    fn caller(path: &str, text: &str) -> Connection {
        connected(path, FileContent::Text(text.to_owned()))
    }

    /// Connected code: the whole of `path`, holding `content`, which calls
    /// `pay` in `src/pay.rs`.
    fn connected(path: &str, content: FileContent) -> Connection {
        Connection::new(
            path.to_owned(),
            Extent::WholeFile,
            ConnectionGroup::ConnectedCode,
            Tier::High,
            vec![Relation::Calls {
                callable: "pay".to_owned(),
                touched_file: "src/pay.rs".to_owned(),
            }],
            content,
        )
    }

    /// The status of `found`'s first connection, as `choices` leave it.
    fn status(found: &Connections, choices: &ReviewChoices) -> FileStatus {
        build(found, choices).connections()[0].status().clone()
    }

    fn build(connections: &Connections, choices: &ReviewChoices) -> Evidence {
        build_excluding(&ExcludePatterns::default(), connections, choices)
    }

    fn build_excluding(
        exclude: &ExcludePatterns,
        connections: &Connections,
        choices: &ReviewChoices,
    ) -> Evidence {
        Evidence::build(
            &change(),
            exclude,
            &[],
            connections,
            ConnectedCodeLimit::effective(None, None),
            choices,
        )
    }

    fn paths(evidence: &Evidence) -> Vec<&str> {
        evidence
            .connections()
            .iter()
            .map(|connection| connection.id().path())
            .collect()
    }

    #[test]
    fn evidence_without_connections_says_why_there_are_none() {
        for (connections, state) in [
            (Connections::Finding, ConnectionsState::Finding),
            (Connections::NoneFound, ConnectionsState::NoneFound),
            (Connections::Found(Vec::new()), ConnectionsState::NoneFound),
            (Connections::TookTooLong, ConnectionsState::TookTooLong),
            (
                Connections::AnalysisFailed,
                ConnectionsState::AnalysisFailed,
            ),
        ] {
            let evidence = build(&connections, &ReviewChoices::default());

            assert_eq!(evidence.connections_state(), state, "{connections:?}");
            assert!(evidence.connections().is_empty(), "{connections:?}");
        }
    }

    #[test]
    fn a_connection_sends_the_lines_of_its_range_from_their_first_number() {
        let connection = Connection::new(
            "src/checkout.rs".to_owned(),
            Extent::Lines(LineRange::new(2, 3).expect("the range is valid")),
            ConnectionGroup::ConnectedCode,
            Tier::Medium,
            Vec::new(),
            FileContent::Text("use pay;\r\nfn checkout() {\r\n    pay()\r\n}\r\n".to_owned()),
        );

        let evidence = build(
            &Connections::Found(vec![connection]),
            &ReviewChoices::default(),
        );

        let connection = &evidence.connections()[0];
        assert_eq!(connection.status(), &FileStatus::Included);
        assert_eq!(
            connection.sent(),
            Some(SentConnection {
                text: "fn checkout() {\r\n    pay()\r\n",
                first_line: 2,
            })
        );
    }

    #[test]
    fn a_connection_sends_only_the_lines_of_its_range_that_its_file_has() {
        let evidence = build(
            &Connections::Found(vec![api(lines(3, 9))]),
            &ReviewChoices::default(),
        );

        assert_eq!(
            evidence.connections()[0].sent(),
            Some(SentConnection {
                text: "c\nd\n",
                first_line: 3,
            })
        );
    }

    #[test]
    fn a_whole_file_connection_sends_all_of_it_from_line_one() {
        let evidence = build(
            &Connections::Found(vec![caller("src/api.rs", "fn post() {\n    pay()\n}")]),
            &ReviewChoices::default(),
        );

        assert_eq!(
            evidence.connections()[0].sent(),
            Some(SentConnection {
                text: "fn post() {\n    pay()\n}",
                first_line: 1,
            })
        );
    }

    #[test]
    fn an_included_connection_counts_its_path_and_sent_text_towards_the_size() {
        let without = build(&Connections::Finding, &ReviewChoices::default());

        let with = build(
            &Connections::Found(vec![caller("src/api.rs", "fn post() { pay() }\n")]),
            &ReviewChoices::default(),
        );

        // 10 bytes of path and 20 of text.
        assert_eq!(with.connections()[0].size(), Some(30));
        assert_eq!(with.size(), without.size() + 30);
    }

    #[test]
    fn a_binary_file_symlink_or_submodule_connection_is_always_left_out() {
        let nothing = build(&Connections::Finding, &ReviewChoices::default()).size();
        for (content, size, reason) in [
            (FileContent::Binary { size: 5 }, Some(5), Exclusion::Binary),
            (FileContent::Symlink, None, Exclusion::Symlink),
            (FileContent::Submodule, None, Exclusion::Submodule),
        ] {
            let found = Connections::Found(vec![connected("vendor/logo", content)]);
            let mut choices = ReviewChoices::default();

            let evidence = build(&found, &choices);
            let connection = &evidence.connections()[0];
            choices.toggle_connection(connection);

            assert_eq!(connection.status(), &FileStatus::LeftOut(reason.clone()));
            assert_eq!(connection.sent(), None);
            assert_eq!(connection.size(), size);
            assert_eq!(evidence.size(), nothing);
            assert_eq!(status(&found, &choices), FileStatus::LeftOut(reason));
        }
    }

    #[test]
    fn a_lockfile_or_secret_path_connection_is_left_out_by_default_until_brought_back() {
        for (path, reason) in [
            ("Cargo.lock", "left out: lockfile"),
            ("config/.env", "left out: possible secret path"),
        ] {
            let found = Connections::Found(vec![caller(path, "KEY=1\n")]);
            let mut choices = ReviewChoices::default();

            let evidence = build(&found, &choices);
            assert_eq!(evidence.connections()[0].status().to_string(), reason);
            assert_eq!(evidence.connections()[0].sent(), None);

            choices.toggle_connection(&evidence.connections()[0]);
            let brought_back = build(&found, &choices);
            assert_eq!(
                brought_back.connections()[0].status(),
                &FileStatus::IncludedByYou
            );
            assert_eq!(
                brought_back.connections()[0].sent().map(|sent| sent.text),
                Some("KEY=1\n")
            );

            choices.toggle_connection(&brought_back.connections()[0]);
            assert_eq!(status(&found, &choices).to_string(), reason);
        }
    }

    #[test]
    fn a_connection_an_exclude_pattern_matches_is_left_out_with_it_until_brought_back() {
        let exclude = ExcludePatterns::parse(["fixtures/"]).expect("the pattern is valid");
        let found = Connections::Found(vec![caller("tests/fixtures/pay.rs", "fn pay() {}\n")]);
        let mut choices = ReviewChoices::default();

        let evidence = build_excluding(&exclude, &found, &choices);
        assert_eq!(
            evidence.connections()[0].status().to_string(),
            "left out by repository config: fixtures/"
        );
        assert_eq!(evidence.connections()[0].sent(), None);

        choices.toggle_connection(&evidence.connections()[0]);
        let brought_back = build_excluding(&exclude, &found, &choices);
        assert_eq!(
            brought_back.connections()[0].status(),
            &FileStatus::IncludedByYou
        );
    }

    #[test]
    fn a_default_exclusion_comes_before_an_exclude_pattern_for_a_connection() {
        let exclude = ExcludePatterns::parse(["*.lock"]).expect("the pattern is valid");
        let found = Connections::Found(vec![caller("Cargo.lock", "version = 4\n")]);

        let evidence = build_excluding(&exclude, &found, &ReviewChoices::default());

        assert_eq!(
            evidence.connections()[0].status().to_string(),
            "left out: lockfile"
        );
    }

    #[test]
    fn the_learner_can_leave_out_any_connection_and_take_it_back() {
        let found = Connections::Found(vec![caller("src/api.rs", "fn post() { pay() }\n")]);
        let mut choices = ReviewChoices::default();
        let included = build(&found, &choices);

        choices.toggle_connection(&included.connections()[0]);
        let left_out = build(&found, &choices);

        assert_eq!(
            left_out.connections()[0].status(),
            &FileStatus::LeftOutByYou
        );
        assert_eq!(left_out.connections()[0].sent(), None);
        assert_eq!(left_out.size(), included.size() - 30);

        choices.toggle_connection(&left_out.connections()[0]);
        assert_eq!(status(&found, &choices), FileStatus::Included);
    }

    #[test]
    fn leaving_out_a_connection_returns_one_brought_back_to_its_rule() {
        let found = Connections::Found(vec![
            caller("Cargo.lock", "version = 4\n"),
            caller("src/api.rs", "fn post() { pay() }\n"),
        ]);
        let mut choices = ReviewChoices::default();
        let statuses = |evidence: &Evidence| -> Vec<(String, String)> {
            evidence
                .connections()
                .iter()
                .map(|connection| {
                    (
                        connection.id().path().to_owned(),
                        connection.status().to_string(),
                    )
                })
                .collect()
        };
        let evidence = build(&found, &choices);
        let lockfile = evidence
            .connections()
            .iter()
            .find(|connection| connection.id().path() == "Cargo.lock")
            .expect("the lockfile is listed");
        choices.toggle_connection(lockfile);

        for connection in build(&found, &choices).connections() {
            choices.leave_out_connection(connection.id());
        }

        assert_eq!(
            statuses(&build(&found, &choices)),
            [
                ("src/api.rs".to_owned(), "left out by you".to_owned()),
                ("Cargo.lock".to_owned(), "left out: lockfile".to_owned()),
            ]
        );
    }

    /// Connected code: lines 3–4 of `src/aws.rs`, whose fourth line holds
    /// a key, and `first_line` before them.
    fn client(first_line: &str) -> Connection {
        let key = concat!("AKIA", "FAKEFAKEFAKEFAKE");
        Connection::new(
            "src/aws.rs".to_owned(),
            Extent::Lines(LineRange::new(3, 4).expect("the range is valid")),
            ConnectionGroup::ConnectedCode,
            Tier::High,
            Vec::new(),
            FileContent::Text(format!(
                "{first_line}\nfn a() {{}}\nfn client() {{\n    let key = \"{key}\";\n}}\n"
            )),
        )
    }

    #[test]
    fn a_secret_in_a_connection_blocks_send_until_marked_not_a_secret() {
        let found = Connections::Found(vec![client("fn start() {}")]);
        let mut choices = ReviewChoices::default();

        let evidence = build(&found, &choices);

        let [finding] = evidence.findings() else {
            panic!("one finding: {:?}", evidence.findings());
        };
        assert_eq!(
            finding.place().connection().map(ConnectionId::path),
            Some("src/aws.rs")
        );
        assert_eq!(finding.place().file(), None);
        // The line in the file, as the reader numbers it.
        assert_eq!(finding.matched().line(), 4);
        assert_eq!(finding.status(), FindingStatus::Unresolved);
        assert_eq!(evidence.unsendable(), Some(Unsendable::UnresolvedFindings));

        choices.toggle_not_a_secret(finding);
        let resolved = build(&found, &choices);

        assert_eq!(resolved.findings()[0].status(), FindingStatus::NotASecret);
        assert_eq!(resolved.unsendable(), None);
    }

    /// Connected code: the whole of `src/db.rs`, whose second line holds a
    /// password in a URL.
    fn database() -> Connection {
        caller(
            "src/db.rs",
            concat!(
                "fn connect() {\n    open(\"postgres://app:",
                "hunter2@db.internal/app\")\n}\n"
            ),
        )
    }

    #[test]
    fn a_warning_never_makes_the_evidence_unsendable() {
        let evidence = build(
            &Connections::Found(vec![database()]),
            &ReviewChoices::default(),
        );

        let [finding] = evidence.findings() else {
            panic!("one finding: {:?}", evidence.findings());
        };
        assert_eq!(finding.matched().rule().name(), "Password in a URL");
        assert_eq!(finding.severity(), Severity::Warning);
        assert_eq!(finding.matched().line(), 2);
        assert_eq!(finding.status(), FindingStatus::Unresolved);
        assert_eq!(evidence.unsendable(), None);
    }

    #[test]
    fn a_blocking_finding_still_makes_the_evidence_unsendable_beside_a_warning() {
        let found = Connections::Found(vec![database(), client("fn start() {}")]);
        let mut choices = ReviewChoices::default();
        let evidence = build(&found, &choices);
        let severities: Vec<Severity> = evidence.findings().iter().map(Finding::severity).collect();
        assert_eq!(severities, [Severity::Warning, Severity::Blocking]);
        assert_eq!(evidence.unsendable(), Some(Unsendable::UnresolvedFindings));

        choices.toggle_not_a_secret(&evidence.findings()[1]);
        let resolved = build(&found, &choices);

        assert_eq!(resolved.findings()[0].status(), FindingStatus::Unresolved);
        assert_eq!(resolved.unsendable(), None);
    }

    /// The repository's area `security`, whose file is `text`.
    fn area(text: &str) -> Area {
        Area::offered([("security.md", Ok(text.as_bytes()))])
            .remove(0)
            .expect("a valid area file")
    }

    fn build_with_areas(areas: &[Area], choices: &ReviewChoices) -> Evidence {
        Evidence::build(
            &change(),
            &ExcludePatterns::default(),
            areas,
            &Connections::Finding,
            ConnectedCodeLimit::effective(None, None),
            choices,
        )
    }

    #[test]
    fn an_area_file_holding_a_github_token_blocks_send_until_marked_not_a_secret() {
        let token = concat!("ghp_", "FAKEfakeFAKEfakeFAKEfakeFAKEfakeFAKE");
        let security = area(&format!(
            "+++\nname = \"Security\"\ndescription = \"Who can do what\"\n+++\n\n\
             Look at who is trusted.\nCall the API with {token}.\n"
        ));
        let mut choices = ReviewChoices::default();

        let evidence = build_with_areas(std::slice::from_ref(&security), &choices);

        let [finding] = evidence.findings() else {
            panic!("one finding: {:?}", evidence.findings());
        };
        assert_eq!(finding.place(), &Place::AreaPrompt(security.id().clone()));
        assert_eq!(finding.place().file(), None);
        assert_eq!(finding.place().connection(), None);
        // The line in the prompt, as it is sent.
        assert_eq!(finding.matched().line(), 2);
        assert_eq!(finding.matched().rule().name(), "GitHub token");
        assert_eq!(evidence.unsendable(), Some(Unsendable::UnresolvedFindings));

        choices.toggle_not_a_secret(finding);
        let resolved = build_with_areas(&[security], &choices);

        assert_eq!(resolved.findings()[0].status(), FindingStatus::NotASecret);
        assert_eq!(resolved.unsendable(), None);
    }

    #[test]
    fn an_areas_name_and_description_are_checked_too() {
        let jwt = concat!(
            "eyJhbGciOiJIUzI1NiJ9",
            ".eyJzdWIiOiJmYWtlIn0",
            ".FAKEfakeFAKEfakeFAKEfake"
        );
        let url = concat!("https://ops:", "hunter2@vault.internal");
        let security = area(&format!(
            "+++\nname = \"Security {jwt}\"\ndescription = \"Ask {url}\"\n+++\nLook.\n"
        ));

        let evidence = build_with_areas(std::slice::from_ref(&security), &ReviewChoices::default());

        let places: Vec<(&Place, &str)> = evidence
            .findings()
            .iter()
            .map(|finding| (finding.place(), finding.matched().rule().name()))
            .collect();
        let id = security.id().clone();
        assert_eq!(
            places,
            [
                (&Place::AreaName(id.clone()), "JSON Web Token"),
                (&Place::AreaDescription(id), "Password in a URL"),
            ]
        );
    }

    #[test]
    fn leaving_out_a_connection_with_a_secret_resolves_it() {
        let found = Connections::Found(vec![client("fn start() {}")]);
        let mut choices = ReviewChoices::default();
        let evidence = build(&found, &choices);

        choices.leave_out_connection(
            evidence.findings()[0]
                .place()
                .connection()
                .expect("the finding is in a connection"),
        );
        let resolved = build(&found, &choices);

        assert_eq!(resolved.findings(), []);
        assert_eq!(resolved.unsendable(), None);
    }

    #[test]
    fn a_secret_outside_a_connections_range_is_not_sent_or_found() {
        let other_key = concat!("AKIA", "OTHERFAKEOTHERFA");

        let evidence = build(
            &Connections::Found(vec![client(&format!("const KEY: &str = \"{other_key}\";"))]),
            &ReviewChoices::default(),
        );

        let lines: Vec<usize> = evidence
            .findings()
            .iter()
            .map(|finding| finding.matched().line())
            .collect();
        assert_eq!(lines, [4]);
    }

    /// Connected code: `extent` of `src/api.rs`.
    fn api(extent: Extent) -> Connection {
        Connection::new(
            "src/api.rs".to_owned(),
            extent,
            ConnectionGroup::ConnectedCode,
            Tier::High,
            Vec::new(),
            FileContent::Text("a\nb\nc\nd\n".to_owned()),
        )
    }

    fn lines(first: usize, last: usize) -> Extent {
        Extent::Lines(LineRange::new(first, last).expect("the range is valid"))
    }

    #[test]
    fn a_choice_follows_a_connection_by_its_path_and_extent() {
        let mut choices = ReviewChoices::default();
        let first = build(
            &Connections::Found(vec![api(lines(1, 2)), api(lines(3, 4))]),
            &choices,
        );
        choices.toggle_connection(&first.connections()[0]);

        // Found again, in another order, beside others in the same file,
        // and listed in rank order.
        let again = build(
            &Connections::Found(vec![
                api(Extent::WholeFile),
                api(lines(3, 4)),
                api(lines(1, 3)),
                api(lines(1, 2)),
            ]),
            &choices,
        );

        let statuses: Vec<(String, &FileStatus)> = again
            .connections()
            .iter()
            .map(|connection| (connection.id().extent().to_string(), connection.status()))
            .collect();
        assert_eq!(
            statuses,
            [
                ("lines 1–2".to_owned(), &FileStatus::LeftOutByYou),
                ("lines 1–3".to_owned(), &FileStatus::Included),
                ("whole file".to_owned(), &FileStatus::Included),
                ("lines 3–4".to_owned(), &FileStatus::Included),
            ]
        );
    }

    #[test]
    fn a_match_marked_not_a_secret_stays_so_when_its_connection_is_found_again() {
        let mut choices = ReviewChoices::default();
        let first = build(&Connections::Found(vec![client("fn start() {}")]), &choices);
        choices.toggle_not_a_secret(&first.findings()[0]);

        let again = build(
            &Connections::Found(vec![api(lines(1, 2)), client("fn start() {}")]),
            &choices,
        );

        assert_eq!(again.findings()[0].status(), FindingStatus::NotASecret);
    }

    /// The whole of `path`, in `group`, there for `relations`.
    fn related(path: &str, group: ConnectionGroup, relations: Vec<Relation>) -> Connection {
        Connection::new(
            path.to_owned(),
            Extent::WholeFile,
            group,
            Tier::High,
            relations,
            FileContent::Text("x\n".to_owned()),
        )
    }

    fn calls(callable: &str, touched_file: &str) -> Relation {
        Relation::Calls {
            callable: callable.to_owned(),
            touched_file: touched_file.to_owned(),
        }
    }

    #[test]
    fn each_touched_file_counts_the_connected_code_that_relates_to_it() {
        let pay = || "src/pay.rs".to_owned();
        let refund = || "src/refund.rs".to_owned();
        let code = ConnectionGroup::ConnectedCode;
        let found = Connections::Found(vec![
            related(
                "src/api.rs",
                code,
                vec![calls("pay", "src/pay.rs"), calls("charge", "src/pay.rs")],
            ),
            related(
                "src/ledger.rs",
                code,
                vec![
                    Relation::CalledBy {
                        callable: "refund".to_owned(),
                        touched_file: refund(),
                    },
                    calls("pay", "src/pay.rs"),
                ],
            ),
            related(
                "docs/refunds.md",
                code,
                vec![Relation::NamedIn {
                    touched_file: refund(),
                }],
            ),
            // Left out by default, but still connected.
            related(
                "config/.env",
                code,
                vec![Relation::NamedIn {
                    touched_file: pay(),
                }],
            ),
            related(
                "tests/pay.rs",
                ConnectionGroup::RelatedTest,
                vec![Relation::TestFor {
                    touched_file: pay(),
                }],
            ),
            related(
                "Cargo.toml",
                ConnectionGroup::RelatedConfiguration,
                vec![Relation::ManifestFor {
                    touched_file: pay(),
                }],
            ),
        ]);

        let evidence = build(&found, &ReviewChoices::default());

        let counts: Vec<(&str, usize)> = evidence
            .files()
            .iter()
            .map(|file| (file.path(), file.connected()))
            .collect();
        assert_eq!(counts, [("src/pay.rs", 3), ("src/refund.rs", 2)]);
    }

    #[test]
    fn found_connections_are_in_the_evidence_in_rank_order() {
        let connections = Connections::Found(vec![
            caller("src/checkout.rs", "fn checkout() { pay() }\n"),
            related(
                "tests/pay.rs",
                ConnectionGroup::RelatedTest,
                vec![Relation::TestFor {
                    touched_file: "src/pay.rs".to_owned(),
                }],
            ),
        ]);

        let evidence = build(&connections, &ReviewChoices::default());

        assert_eq!(evidence.connections_state(), ConnectionsState::Found);
        assert_eq!(paths(&evidence), ["tests/pay.rs", "src/checkout.rs"]);
        let test = &evidence.connections()[0];
        assert_eq!(test.group(), ConnectionGroup::RelatedTest);
        assert_eq!(test.tier(), Tier::High);
        assert_eq!(test.relations()[0].to_string(), "test for `src/pay.rs`");
    }

    #[test]
    fn connections_never_change_what_touched_files_send() {
        let without = build(&Connections::Finding, &ReviewChoices::default());

        let with = build(
            &Connections::Found(vec![caller("src/api.rs", "fn post() { pay() }\n")]),
            &ReviewChoices::default(),
        );

        let sent = |evidence: &Evidence| -> Vec<(FileStatus, Option<usize>, Option<String>)> {
            evidence
                .files()
                .iter()
                .map(|file| {
                    (
                        file.status().clone(),
                        file.size(),
                        file.sent()
                            .map(|sent| format!("{}{}", sent.diff, sent.content)),
                    )
                })
                .collect()
        };
        assert_eq!(sent(&with), sent(&without));
    }

    #[test]
    fn evidence_with_every_touched_file_left_out_has_nothing_to_send_beside_its_connections() {
        let mut choices = ReviewChoices::default();
        choices.leave_out(FileIndex(0));
        choices.leave_out(FileIndex(1));

        let evidence = build(
            &Connections::Found(vec![caller("src/api.rs", "fn post() { pay() }\n")]),
            &choices,
        );

        assert!(evidence.connections()[0].sent().is_some());
        assert_eq!(evidence.unsendable(), Some(Unsendable::NothingToSend));
    }

    #[test]
    fn evidence_debugs_its_connections_without_their_text_or_callable_names() {
        let evidence = build(
            &Connections::Found(vec![Connection::new(
                "src/api.rs".to_owned(),
                Extent::WholeFile,
                ConnectionGroup::ConnectedCode,
                Tier::High,
                vec![calls("charge_quietly", "src/pay.rs")],
                FileContent::Text("PASSWORD=hunter2\n".to_owned()),
            )]),
            &ReviewChoices::default(),
        );

        let debug = format!("{evidence:?}");

        assert!(debug.contains("src/api.rs"), "{debug}");
        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(!debug.contains("charge_quietly"), "{debug}");
    }
}
