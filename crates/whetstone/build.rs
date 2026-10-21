//! Embeds the showcase project's curated history in the binary, so Practice
//! needs no git and no showcase files at runtime.
//!
//! The history is the `git format-patch` series in
//! `showcase/payments-history/` (`showcase/README.md` describes it). The
//! project ships no area files, so it offers Whetstone's default areas, and
//! each change's `Areas:` trailer names the IDs of default areas. This script
//! replays the series in pure Rust, from an empty tree, and writes
//! `$OUT_DIR/showcase.rs`, which `src/showcase.rs` includes: for each change
//! its message, the area IDs its trailer names, its diff, and each touched
//! file before and after.
//!
//! The output depends only on those files and the default areas, so the
//! build is deterministic. A default area file that is invalid, a hunk that
//! does not apply exactly, a trailer that names anything but a default
//! area's ID, or anything else the replay does not understand fails the
//! build. `build/areas.rs` holds the checks on areas, which the integration
//! tests share.
//!
//! It also sets `WHETSTONE_LONG_VERSION`, the version `whetstone --version`
//! prints: the package version, then the commit git says this crate is built
//! from and that commit's date (`build/version.rs`). Without git, or outside
//! a repository, it is the package version alone, and the build carries on.

use std::collections::BTreeMap;
use std::error::Error as _;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::{env, fs, io};

use whetstone_core::{Area, AreaId};

#[path = "build/areas.rs"]
mod areas;
#[path = "build/version.rs"]
mod version;

/// The showcase project's patch series, relative to this crate.
const HISTORY: &str = "../../showcase/payments-history";

fn main() -> ExitCode {
    set_long_version();
    match embed() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let mut message = error.to_string();
            let mut source = error.source();
            while let Some(cause) = source {
                message = format!("{message}: {cause}");
                source = cause.source();
            }
            report_failure(&message);
            ExitCode::FAILURE
        }
    }
}

#[expect(
    clippy::print_stderr,
    reason = "a build script reports its failure on stderr"
)]
fn report_failure(message: &str) {
    eprintln!("error: cannot embed the showcase project: {message}");
}

/// The showcase project could not be embedded.
#[derive(Debug, thiserror::Error)]
enum EmbedError {
    #[error("Cargo did not set OUT_DIR")]
    NoOutDir,
    #[error("cannot read {}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot write {}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("a default area it offers is invalid")]
    DefaultArea(#[source] whetstone_core::InvalidAreaFile),
    #[error("{}", path.display())]
    History {
        path: PathBuf,
        #[source]
        source: HistoryError,
    },
}

/// A patch the replay cannot parse or apply, and why.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct HistoryError(String);

/// A `HistoryError` saying `problem`.
fn malformed(problem: impl Into<String>) -> HistoryError {
    HistoryError(problem.into())
}

fn embed() -> Result<(), EmbedError> {
    tell_cargo(&[HISTORY]);
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or(EmbedError::NoOutDir)?);
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    // A trailer may name only the areas a learner is offered: the defaults,
    // which must all be valid.
    let defaults = areas::default_ids(Area::default_files()).map_err(EmbedError::DefaultArea)?;
    let mut replay = Replay::default();
    for path in patch_files(&crate_dir.join(HISTORY))? {
        let text = read(&path)?;
        Patch::parse(&text)
            .and_then(|patch| patch.check_areas(&defaults).map(|()| patch))
            .and_then(|patch| replay.apply(patch))
            .map_err(|source| EmbedError::History { path, source })?;
    }
    let out = out.join("showcase.rs");
    fs::write(&out, rust_source(&replay)).map_err(|source| EmbedError::Write { path: out, source })
}

/// Sets `WHETSTONE_LONG_VERSION` for the crate, and asks Cargo to run this
/// script again when HEAD moves to another commit.
fn set_long_version() {
    let git_log = git(&["log", "-1", version::GIT_LOG_FORMAT]);
    let long_version = version::long_version(env!("CARGO_PKG_VERSION"), git_log.as_deref());
    println!("cargo::rustc-env=WHETSTONE_LONG_VERSION={long_version}");
    for path in head_files() {
        println!("cargo::rerun-if-changed={}", path.display());
    }
}

/// What `git` prints with `args` in this crate's directory, or `None` when
/// git isn't installed, the crate isn't in a repository, or git fails.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()?;
    if output.status.success() {
        String::from_utf8(output.stdout).ok()
    } else {
        None
    }
}

/// The files git changes when HEAD moves to another commit: HEAD and its
/// log, the branch HEAD is on, and the packed refs. A linked worktree has a
/// HEAD of its own but shares branches with the main repository. Only files
/// that exist are named, because Cargo reruns a script on every build while
/// a file it watches is missing.
fn head_files() -> Vec<PathBuf> {
    let Some(dirs) = git(&[
        "rev-parse",
        "--path-format=absolute",
        "--git-dir",
        "--git-common-dir",
    ]) else {
        return Vec::new();
    };
    let mut dirs = dirs.lines().map(Path::new);
    let (Some(own), Some(common)) = (dirs.next(), dirs.next()) else {
        return Vec::new();
    };
    let mut files = vec![
        own.join("HEAD"),
        own.join("logs/HEAD"),
        common.join("packed-refs"),
    ];
    if let Some(branch) = git(&["symbolic-ref", "--quiet", "HEAD"]) {
        files.push(common.join(branch.trim_end()));
    }
    files.retain(|file| file.is_file());
    files
}

/// Asks Cargo to run this script again when any of `paths` changes. A
/// directory counts as changed when anything in it does.
fn tell_cargo(paths: &[&str]) {
    println!("cargo::rerun-if-changed=build.rs");
    for path in paths {
        println!("cargo::rerun-if-changed={path}");
    }
}

fn read(path: &Path) -> Result<String, EmbedError> {
    fs::read_to_string(path).map_err(|source| EmbedError::Read {
        path: path.to_owned(),
        source,
    })
}

/// The patch files in `dir`, in the series' order.
fn patch_files(dir: &Path) -> Result<Vec<PathBuf>, EmbedError> {
    let read_dir = |source| EmbedError::Read {
        path: dir.to_owned(),
        source,
    };
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir).map_err(read_dir)? {
        let path = entry.map_err(read_dir)?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "patch")
        {
            paths.push(path);
        }
    }
    // `format-patch` numbers its files, so their names sort in series order.
    paths.sort();
    Ok(paths)
}

/// One file of the series: a change as `git format-patch` writes it.
#[derive(Debug)]
struct Patch<'a> {
    /// The commit message, without its `Areas:` trailer.
    message: String,
    /// The area IDs the `Areas:` trailer names.
    areas: Vec<&'a str>,
    /// Every file's diff, as the patch has it.
    diff: String,
    files: Vec<FileDiff>,
}

/// One file's part of a patch.
#[derive(Debug)]
struct FileDiff {
    path: String,
    status: FileStatus,
    hunks: Vec<Hunk>,
}

/// What a patch does to a file, in git's terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileStatus {
    Added,
    Modified,
    Deleted,
}

/// A hunk: where it starts in the file before the change, how many lines it
/// covers there, and its lines.
#[derive(Debug)]
struct Hunk {
    old_start: usize,
    old_len: usize,
    lines: Vec<HunkLine>,
}

/// A line of a hunk, with its line ending if it has one.
#[derive(Debug)]
enum HunkLine {
    Context(String),
    Removed(String),
    Added(String),
}

impl<'a> Patch<'a> {
    /// Parses a `git format-patch` file: its mail headers, its message up to
    /// the `---` line, the diffstat, then each file's diff.
    fn parse(text: &'a str) -> Result<Self, HistoryError> {
        let lines: Vec<&str> = text.split('\n').collect();
        let blank = lines
            .iter()
            .position(|line| line.is_empty())
            .ok_or_else(|| malformed("no blank line after the mail headers"))?;
        let subject = subject(&lines[..blank])?;
        let separator = blank
            + lines[blank..]
                .iter()
                .position(|line| *line == "---")
                .ok_or_else(|| malformed("no `---` line after the message"))?;
        let (message, areas) = message(&subject, &lines[blank + 1..separator])?;
        let first_diff = separator
            + lines[separator..]
                .iter()
                .position(|line| line.starts_with("diff --git "))
                .ok_or_else(|| malformed("no diff"))?;
        let mut at = first_diff;
        let mut files = Vec::new();
        while lines
            .get(at)
            .is_some_and(|line| line.starts_with("diff --git "))
        {
            let (file, next) = FileDiff::parse(&lines, at)?;
            files.push(file);
            at = next;
        }
        // Only blank lines, or a signature after `-- `, may follow.
        if let Some(line) = lines[at..]
            .iter()
            .take_while(|line| **line != "-- ")
            .find(|line| !line.is_empty())
        {
            return Err(malformed(format!(
                "unexpected line after the diff: {line:?}"
            )));
        }
        Ok(Self {
            message,
            areas,
            diff: lines[first_diff..at].join("\n") + "\n",
            files,
        })
    }

    /// Fails unless every area the trailer names is the ID of one of the
    /// `defaults`.
    fn check_areas(&self, defaults: &[AreaId]) -> Result<(), HistoryError> {
        areas::check_trailer(&self.areas, defaults).map_err(|error| malformed(error.to_string()))
    }
}

/// The subject from the mail `headers`, without its `[PATCH n/m]` prefix.
/// A long subject is folded onto indented lines, which are joined again.
fn subject(headers: &[&str]) -> Result<String, HistoryError> {
    let start = headers
        .iter()
        .position(|line| line.starts_with("Subject: "))
        .ok_or_else(|| malformed("no Subject header"))?;
    let mut subject = headers[start]
        .strip_prefix("Subject: ")
        .unwrap_or_default()
        .to_owned();
    for folded in headers[start + 1..]
        .iter()
        .take_while(|line| line.starts_with([' ', '\t']))
    {
        subject.push(' ');
        subject.push_str(folded.trim_start());
    }
    if subject.contains("=?") {
        return Err(malformed(format!(
            "the subject {subject:?} is MIME-encoded, which the replay does not decode"
        )));
    }
    Ok(
        match subject
            .strip_prefix('[')
            .and_then(|rest| rest.split_once("] "))
        {
            Some((_, subject)) => subject.to_owned(),
            None => subject,
        },
    )
}

/// The commit message from `subject` and the `body` lines, without the
/// `Areas:` trailer that ends it, and the area IDs that trailer names. As in
/// git, the trailer is a paragraph of its own.
fn message<'a>(subject: &str, body: &[&'a str]) -> Result<(String, Vec<&'a str>), HistoryError> {
    let mut body = without_blank_lines_at_end(body);
    let areas = body
        .pop()
        .and_then(|line| line.strip_prefix("Areas:"))
        .ok_or_else(|| malformed("the message does not end with an `Areas:` trailer"))?;
    if body.last().is_some_and(|line| !line.is_empty()) {
        return Err(malformed(
            "the `Areas:` trailer is not a paragraph of its own",
        ));
    }
    let areas: Vec<&str> = areas.split(',').map(str::trim).collect();
    if areas.iter().any(|area| area.is_empty()) {
        return Err(malformed("the `Areas:` trailer names an empty area"));
    }
    let body = without_blank_lines_at_end(&body);
    Ok((
        if body.is_empty() {
            subject.to_owned()
        } else {
            format!("{subject}\n\n{}", body.join("\n"))
        },
        areas,
    ))
}

/// `lines` without the blank lines at their end.
fn without_blank_lines_at_end<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    let end = lines
        .iter()
        .rposition(|line| !line.is_empty())
        .map_or(0, |last| last + 1);
    lines[..end].to_vec()
}

impl FileDiff {
    /// Parses the file diff starting at the `diff --git` line `at`, and
    /// returns it with the index of the line after it.
    fn parse(lines: &[&str], mut at: usize) -> Result<(Self, usize), HistoryError> {
        let header = lines[at];
        at += 1;
        let mut status = FileStatus::Modified;
        loop {
            let line = lines.get(at).copied().unwrap_or_default();
            if line.starts_with("--- ") {
                break;
            } else if line.starts_with("new file mode ") {
                status = FileStatus::Added;
            } else if line.starts_with("deleted file mode ") {
                status = FileStatus::Deleted;
            } else if !line.starts_with("index ") {
                // Renames, copies, mode changes and binary files.
                return Err(malformed(format!(
                    "unsupported line {line:?} after {header:?}"
                )));
            }
            at += 1;
        }
        let old = lines[at].strip_prefix("--- ").unwrap_or_default();
        let new = lines
            .get(at + 1)
            .and_then(|line| line.strip_prefix("+++ "))
            .ok_or_else(|| malformed(format!("no `+++` line after {header:?}")))?;
        at += 2;
        let path = match (status, old.strip_prefix("a/"), new.strip_prefix("b/")) {
            (FileStatus::Added, None, Some(path)) if old == "/dev/null" => path,
            (FileStatus::Deleted, Some(path), None) if new == "/dev/null" => path,
            (FileStatus::Modified, Some(old), Some(new)) if old == new => new,
            _ => {
                return Err(malformed(format!("unsupported paths {old:?} and {new:?}")));
            }
        };
        let mut hunks = Vec::new();
        while lines.get(at).is_some_and(|line| line.starts_with("@@ ")) {
            let (hunk, next) = Hunk::parse(lines, at)?;
            hunks.push(hunk);
            at = next;
        }
        Ok((
            Self {
                path: path.to_owned(),
                status,
                hunks,
            },
            at,
        ))
    }
}

impl Hunk {
    /// Parses the hunk starting at the `@@` line `at`, and returns it with the
    /// index of the line after it. The counts in the `@@` line say where it
    /// ends.
    fn parse(lines: &[&str], mut at: usize) -> Result<(Self, usize), HistoryError> {
        let header = lines[at];
        let (old, new) = header
            .strip_prefix("@@ -")
            .and_then(|rest| rest.split_once(" @@"))
            .and_then(|(ranges, _)| ranges.split_once(" +"))
            .ok_or_else(|| malformed(format!("malformed hunk header {header:?}")))?;
        let (old_start, mut old_left) = range(old, header)?;
        let (_, mut new_left) = range(new, header)?;
        let old_len = old_left;
        at += 1;
        let mut hunk_lines = Vec::new();
        let short = || malformed(format!("the hunk {header:?} is shorter than it says"));
        while old_left > 0 || new_left > 0 {
            let line = lines.get(at).ok_or_else(short)?;
            at += 1;
            let (line, old_lines, new_lines) = if let Some(text) = line.strip_prefix(' ') {
                (HunkLine::Context(format!("{text}\n")), 1, 1)
            } else if let Some(text) = line.strip_prefix('-') {
                (HunkLine::Removed(format!("{text}\n")), 1, 0)
            } else if let Some(text) = line.strip_prefix('+') {
                (HunkLine::Added(format!("{text}\n")), 0, 1)
            } else if line.starts_with('\\') {
                strip_line_ending(&mut hunk_lines);
                continue;
            } else {
                return Err(malformed(format!(
                    "unexpected line {line:?} in the hunk {header:?}"
                )));
            };
            old_left = old_left.checked_sub(old_lines).ok_or_else(short)?;
            new_left = new_left.checked_sub(new_lines).ok_or_else(short)?;
            hunk_lines.push(line);
        }
        if lines.get(at).is_some_and(|line| line.starts_with('\\')) {
            strip_line_ending(&mut hunk_lines);
            at += 1;
        }
        Ok((
            Self {
                old_start,
                old_len,
                lines: hunk_lines,
            },
            at,
        ))
    }
}

/// Removes the line ending from the last of `lines`, as a
/// `\ No newline at end of file` marker after it says.
fn strip_line_ending(lines: &mut [HunkLine]) {
    if let Some(HunkLine::Context(text) | HunkLine::Removed(text) | HunkLine::Added(text)) =
        lines.last_mut()
    {
        text.pop();
    }
}

/// A hunk header's `start,len` range. A range without `,len` is one line.
fn range(range: &str, header: &str) -> Result<(usize, usize), HistoryError> {
    let number = |text: &str| {
        text.parse()
            .map_err(|_| malformed(format!("malformed hunk header {header:?}")))
    };
    match range.split_once(',') {
        Some((start, len)) => Ok((number(start)?, number(len)?)),
        None => Ok((number(range)?, 1)),
    }
}

/// The series replayed so far: the tree it has built, and every change.
#[derive(Debug, Default)]
struct Replay {
    /// Each file in the tree, and the index of its contents.
    tree: BTreeMap<String, usize>,
    /// Every version of every file. A file's version before a change is its
    /// version after the change before, so each is kept once.
    contents: Vec<String>,
    changes: Vec<ReplayedChange>,
}

/// A change as the replay applied it.
#[derive(Debug)]
struct ReplayedChange {
    message: String,
    areas: Vec<String>,
    diff: String,
    files: Vec<(String, Versions)>,
}

/// A touched file's contents on each side of a change, as indexes into
/// [`Replay::contents`].
#[derive(Debug, Clone, Copy)]
enum Versions {
    Created { after: usize },
    Modified { before: usize, after: usize },
    Deleted { before: usize },
}

impl Replay {
    /// Applies `patch` to the tree, and records it as a change.
    fn apply(&mut self, patch: Patch<'_>) -> Result<(), HistoryError> {
        let mut files = Vec::new();
        for file in &patch.files {
            let before = self.tree.get(&file.path).copied();
            let after = apply_file_diff(before.map(|index| self.contents[index].as_str()), file)?
                .map(|after| {
                    self.contents.push(after);
                    self.contents.len() - 1
                });
            let versions = match (before, after) {
                (None, Some(after)) => Versions::Created { after },
                (Some(before), Some(after)) => Versions::Modified { before, after },
                (Some(before), None) => Versions::Deleted { before },
                (None, None) => {
                    return Err(malformed(format!(
                        "the patch leaves {} absent before and after",
                        file.path
                    )));
                }
            };
            match after {
                Some(index) => self.tree.insert(file.path.clone(), index),
                None => self.tree.remove(&file.path),
            };
            files.push((file.path.clone(), versions));
        }
        self.changes.push(ReplayedChange {
            message: patch.message,
            areas: patch.areas.iter().map(|&area| area.to_owned()).collect(),
            diff: patch.diff,
            files,
        });
        Ok(())
    }
}

/// The file `diff` changes, as it is after the change, from its contents
/// `before` it. Every hunk must match exactly where it says it applies.
/// Returns `None` if the change deletes the file.
fn apply_file_diff(before: Option<&str>, diff: &FileDiff) -> Result<Option<String>, HistoryError> {
    let path = &diff.path;
    match (diff.status, before) {
        (FileStatus::Added, Some(_)) => {
            return Err(malformed(format!(
                "the patch creates {path}, which already exists"
            )));
        }
        (FileStatus::Modified | FileStatus::Deleted, None) => {
            return Err(malformed(format!(
                "the patch changes {path}, which does not exist"
            )));
        }
        _ => {}
    }
    let old: Vec<&str> = before.unwrap_or_default().split_inclusive('\n').collect();
    let mut after = String::new();
    let mut next = 0;
    for hunk in &diff.hunks {
        // A hunk that removes nothing inserts after line `old_start`.
        let start = if hunk.old_len == 0 {
            hunk.old_start
        } else {
            hunk.old_start.saturating_sub(1)
        };
        if start < next || start > old.len() {
            return Err(malformed(format!(
                "a hunk for {path} starts at line {}, out of order or past the end",
                hunk.old_start
            )));
        }
        after.extend(old[next..start].iter().copied());
        let mut at = start;
        for line in &hunk.lines {
            match line {
                HunkLine::Context(text) | HunkLine::Removed(text) => {
                    if old.get(at) != Some(&text.as_str()) {
                        return Err(malformed(format!(
                            "a hunk does not match {path} at line {}: expected {text:?}",
                            at + 1
                        )));
                    }
                    if let HunkLine::Context(text) = line {
                        after.push_str(text);
                    }
                    at += 1;
                }
                HunkLine::Added(text) => after.push_str(text),
            }
        }
        next = at;
    }
    after.extend(old[next..].iter().copied());
    match diff.status {
        FileStatus::Deleted if after.is_empty() => Ok(None),
        FileStatus::Deleted => Err(malformed(format!(
            "the patch deletes {path} but leaves lines in it"
        ))),
        FileStatus::Added | FileStatus::Modified => Ok(Some(after)),
    }
}

/// Writing to a `String` cannot fail.
const INFALLIBLE: &str = "writing to a String never fails";

/// The Rust source `src/showcase.rs` includes: the replayed changes, built
/// with `whetstone-core`'s constructors and `src/showcase.rs`'s `area`
/// function. Each version of a file is one entry of `CONTENTS`.
fn rust_source(replay: &Replay) -> String {
    let mut source = String::from(
        "// @generated by crates/whetstone/build.rs from the showcase project's\n\
         // patch series. Do not edit.\n\n\
         fn changes() -> Vec<Change> {\n    vec![\n",
    );
    for change in &replay.changes {
        write_change(&mut source, change);
    }
    source.push_str("    ]\n}\n\nconst CONTENTS: &[&str] = &[\n");
    for text in &replay.contents {
        writeln!(source, "    {text:?},").expect(INFALLIBLE);
    }
    source.push_str("];\n");
    source
}

/// Writes one replayed change to `source` as a `Change::new` call.
fn write_change(source: &mut String, change: &ReplayedChange) {
    write!(
        source,
        "        Change::new({:?}.to_owned(), vec![",
        change.message
    )
    .expect(INFALLIBLE);
    for area in &change.areas {
        write!(source, "area({area:?}), ").expect(INFALLIBLE);
    }
    write!(source, "], {:?}.to_owned(), vec![", change.diff).expect(INFALLIBLE);
    let contents = |index: usize| format!("CONTENTS[{index}].to_owned()");
    for (path, versions) in &change.files {
        match *versions {
            Versions::Created { after } => write!(
                source,
                "TouchedFile::created({path:?}.to_owned(), {}), ",
                contents(after)
            ),
            Versions::Modified { before, after } => write!(
                source,
                "TouchedFile::modified({path:?}.to_owned(), {}, {}), ",
                contents(before),
                contents(after)
            ),
            Versions::Deleted { before } => write!(
                source,
                "TouchedFile::deleted({path:?}.to_owned(), {}), ",
                contents(before)
            ),
        }
        .expect(INFALLIBLE);
    }
    source.push_str("]),\n");
}
