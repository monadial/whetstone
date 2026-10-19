//! Embeds the showcase project's curated history in the binary, so Practice
//! needs no git and no showcase files at runtime.
//!
//! The history is the `git format-patch` series in
//! `showcase/payments-history/`, and the project declares its areas in
//! `showcase/payments/.whetstone/config.toml` (`showcase/README.md` describes
//! both). This script replays the series in pure Rust, from an empty tree,
//! and writes `$OUT_DIR/showcase.rs`, which `src/showcase.rs` includes: for
//! each change its message, the areas its `Areas:` trailer declares, its diff,
//! and each touched file before and after.
//!
//! The output depends only on those files, so the build is deterministic. A
//! hunk that does not apply exactly, an area the config does not declare, or
//! anything else the replay does not understand fails the build.

use std::collections::BTreeMap;
use std::error::Error as _;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::{env, fs, io};

use whetstone_core::{Area, ShowcaseProject};

/// The showcase project's patch series, relative to this crate.
const HISTORY: &str = "../../showcase/payments-history";
/// The showcase project's config, relative to this crate.
const CONFIG: &str = "../../showcase/payments/.whetstone/config.toml";

fn main() -> ExitCode {
    match embed() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let mut message = error.to_string();
            let mut source = error.source();
            while let Some(cause) = source {
                let _ = write!(message, ": {cause}");
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

/// The showcase history could not be embedded.
#[derive(Debug, thiserror::Error)]
enum EmbedError {
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
    #[error("{} is not valid TOML", path.display())]
    Toml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("{}: {problem}", path.display())]
    Invalid { path: PathBuf, problem: String },
}

fn embed() -> Result<(), EmbedError> {
    tell_cargo(&[HISTORY, CONFIG]);
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let declared = read_areas(&crate_dir.join(CONFIG))?;
    // The areas a learner is offered, which the trailers must name.
    let offered = ShowcaseProject::new(declared.clone(), Vec::new());
    let mut replay = Replay::default();
    for path in patch_files(&crate_dir.join(HISTORY))? {
        let text = read(&path)?;
        let invalid = |problem: String| EmbedError::Invalid {
            path: path.clone(),
            problem,
        };
        let patch = Patch::parse(&text).map_err(invalid)?;
        if let Some(area) = patch
            .areas
            .iter()
            .find(|area| !offered.areas().contains(&Area::new(**area)))
        {
            return Err(invalid(format!(
                "the change declares the area {area:?}, which the showcase project does not"
            )));
        }
        replay.apply(&patch).map_err(invalid)?;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap_or_default()).join("showcase.rs");
    fs::write(&out, replay.source(&declared))
        .map_err(|source| EmbedError::Write { path: out, source })
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

/// The areas the project's config declares: its `areas` array, or none.
fn read_areas(config: &Path) -> Result<Vec<Area>, EmbedError> {
    let table: toml::Table = read(config)?.parse().map_err(|source| EmbedError::Toml {
        path: config.to_owned(),
        source,
    })?;
    let invalid = || EmbedError::Invalid {
        path: config.to_owned(),
        problem: "`areas` must be an array of area names".to_owned(),
    };
    match table.get("areas") {
        None => Ok(Vec::new()),
        Some(areas) => areas
            .as_array()
            .ok_or_else(invalid)?
            .iter()
            .map(|area| area.as_str().map(Area::new).ok_or_else(invalid))
            .collect(),
    }
}

/// One change of the series, parsed from its patch file.
#[derive(Debug)]
struct Patch<'a> {
    /// The commit message, without its `Areas:` trailer.
    message: String,
    /// The areas the `Areas:` trailer declares.
    areas: Vec<&'a str>,
    /// Every file's diff, as the patch has it.
    diff: String,
    files: Vec<FileDiff>,
}

/// One file's part of a patch.
#[derive(Debug)]
struct FileDiff {
    path: String,
    kind: FileChange,
    hunks: Vec<Hunk>,
}

/// What a patch does to a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileChange {
    Created,
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
    fn parse(text: &'a str) -> Result<Self, String> {
        let lines: Vec<&str> = text.split('\n').collect();
        let blank = lines
            .iter()
            .position(|line| line.is_empty())
            .ok_or("no blank line after the mail headers")?;
        let subject = subject(&lines[..blank])?;
        let separator = blank
            + lines[blank..]
                .iter()
                .position(|line| *line == "---")
                .ok_or("no `---` line after the message")?;
        let (message, areas) = message(&subject, &lines[blank + 1..separator])?;
        let first_diff = separator
            + lines[separator..]
                .iter()
                .position(|line| line.starts_with("diff --git "))
                .ok_or("no diff")?;
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
            return Err(format!("unexpected line after the diff: {line:?}"));
        }
        Ok(Self {
            message,
            areas,
            diff: lines[first_diff..at].join("\n") + "\n",
            files,
        })
    }
}

/// The subject from the mail `headers`, without its `[PATCH n/m]` prefix.
/// A long subject is folded onto indented lines, which are joined again.
fn subject(headers: &[&str]) -> Result<String, String> {
    let start = headers
        .iter()
        .position(|line| line.starts_with("Subject: "))
        .ok_or("no Subject header")?;
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
        return Err(format!(
            "the subject {subject:?} is MIME-encoded, which the replay does not decode"
        ));
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
/// `Areas:` trailer that ends it, and the areas that trailer declares.
fn message<'a>(subject: &str, body: &[&'a str]) -> Result<(String, Vec<&'a str>), String> {
    let mut body = body.to_vec();
    while body.last().is_some_and(|line| line.is_empty()) {
        body.pop();
    }
    let areas = body
        .pop()
        .and_then(|line| line.strip_prefix("Areas:"))
        .ok_or("the message does not end with an `Areas:` trailer")?;
    let areas: Vec<&str> = areas.split(',').map(str::trim).collect();
    if areas.iter().any(|area| area.is_empty()) {
        return Err("the `Areas:` trailer names an empty area".to_owned());
    }
    while body.last().is_some_and(|line| line.is_empty()) {
        body.pop();
    }
    let mut message = subject.to_owned();
    if !body.is_empty() {
        message.push_str("\n\n");
        message.push_str(&body.join("\n"));
    }
    Ok((message, areas))
}

impl FileDiff {
    /// Parses the file diff starting at the `diff --git` line `at`, and
    /// returns it with the index of the line after it.
    fn parse(lines: &[&str], mut at: usize) -> Result<(Self, usize), String> {
        let header = lines[at];
        at += 1;
        let mut kind = FileChange::Modified;
        loop {
            let line = lines.get(at).copied().unwrap_or_default();
            if line.starts_with("--- ") {
                break;
            } else if line.starts_with("new file mode ") {
                kind = FileChange::Created;
            } else if line.starts_with("deleted file mode ") {
                kind = FileChange::Deleted;
            } else if !line.starts_with("index ") {
                // Renames, copies, mode changes and binary files.
                return Err(format!("unsupported line {line:?} after {header:?}"));
            }
            at += 1;
        }
        let old = lines[at].strip_prefix("--- ").unwrap_or_default();
        let new = lines
            .get(at + 1)
            .and_then(|line| line.strip_prefix("+++ "))
            .ok_or_else(|| format!("no `+++` line after {header:?}"))?;
        at += 2;
        let path = match (kind, old.strip_prefix("a/"), new.strip_prefix("b/")) {
            (FileChange::Created, None, Some(path)) if old == "/dev/null" => path,
            (FileChange::Deleted, Some(path), None) if new == "/dev/null" => path,
            (FileChange::Modified, Some(old), Some(new)) if old == new => new,
            _ => return Err(format!("unsupported paths {old:?} and {new:?}")),
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
                kind,
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
    fn parse(lines: &[&str], mut at: usize) -> Result<(Self, usize), String> {
        let header = lines[at];
        let (old, new) = header
            .strip_prefix("@@ -")
            .and_then(|rest| rest.split_once(" @@"))
            .and_then(|(ranges, _)| ranges.split_once(" +"))
            .ok_or_else(|| format!("malformed hunk header {header:?}"))?;
        let (old_start, mut old_left) = range(old, header)?;
        let (_, mut new_left) = range(new, header)?;
        let old_len = old_left;
        at += 1;
        let mut hunk_lines = Vec::new();
        let short = || format!("the hunk {header:?} is shorter than it says");
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
                no_newline(&mut hunk_lines);
                continue;
            } else {
                return Err(format!("unexpected line {line:?} in the hunk {header:?}"));
            };
            old_left = old_left.checked_sub(old_lines).ok_or_else(short)?;
            new_left = new_left.checked_sub(new_lines).ok_or_else(short)?;
            hunk_lines.push(line);
        }
        if lines.get(at).is_some_and(|line| line.starts_with('\\')) {
            no_newline(&mut hunk_lines);
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

/// Applies a `\ No newline at end of file` marker to the line before it.
fn no_newline(lines: &mut [HunkLine]) {
    if let Some(HunkLine::Context(text) | HunkLine::Removed(text) | HunkLine::Added(text)) =
        lines.last_mut()
    {
        text.pop();
    }
}

/// A hunk header's `start,len` range. A range without `,len` is one line.
fn range(range: &str, header: &str) -> Result<(usize, usize), String> {
    let number = |text: &str| {
        text.parse()
            .map_err(|_| format!("malformed hunk header {header:?}"))
    };
    match range.split_once(',') {
        Some((start, len)) => Ok((number(start)?, number(len)?)),
        None => Ok((number(range)?, 1)),
    }
}

/// The series replayed so far: the tree it has built, and every change as
/// it will be embedded.
#[derive(Debug, Default)]
struct Replay {
    /// Each file in the tree, and the index of its contents.
    tree: BTreeMap<String, usize>,
    /// Every version of every file. A file's version before a change is its
    /// version after the change before, so each is stored once.
    contents: Vec<String>,
    /// Each change, as Rust source.
    changes: String,
}

impl Replay {
    /// Applies `patch` to the tree, and records it as a change.
    fn apply(&mut self, patch: &Patch<'_>) -> Result<(), String> {
        let mut files = String::new();
        for file in &patch.files {
            let before = self.tree.get(&file.path).copied();
            let after =
                apply(before.map(|index| self.contents[index].as_str()), file)?.map(|after| {
                    self.contents.push(after);
                    self.contents.len() - 1
                });
            match after {
                Some(index) => self.tree.insert(file.path.clone(), index),
                None => self.tree.remove(&file.path),
            };
            let _ = write!(
                files,
                "EmbeddedFile {{ path: {:?}, before: {}, after: {} }}, ",
                file.path,
                contents_source(before),
                contents_source(after),
            );
        }
        let mut areas = String::new();
        for area in &patch.areas {
            let _ = write!(areas, "{area:?}, ");
        }
        let _ = writeln!(
            self.changes,
            "    EmbeddedChange {{ message: {:?}, areas: &[{areas}], diff: {:?}, files: &[{files}] }},",
            patch.message, patch.diff,
        );
        Ok(())
    }

    /// The Rust source `src/showcase.rs` includes, with the areas the config
    /// declares.
    fn source(&self, declared: &[Area]) -> String {
        let mut declared_areas = String::new();
        for area in declared {
            let _ = write!(declared_areas, "{:?}, ", area.name());
        }
        let mut contents = String::new();
        for text in &self.contents {
            let _ = writeln!(contents, "    {text:?},");
        }
        format!(
            "// @generated by crates/whetstone/build.rs from the showcase project's\n\
             // patch series and config. Do not edit.\n\n\
             const DECLARED_AREAS: &[&str] = &[{declared_areas}];\n\n\
             const CHANGES: &[EmbeddedChange] = &[\n{}];\n\n\
             const CONTENTS: &[&str] = &[\n{contents}];\n",
            self.changes,
        )
    }
}

/// A file's version as Rust source: an entry of `CONTENTS`, or `None`.
fn contents_source(index: Option<usize>) -> String {
    index.map_or_else(
        || "None".to_owned(),
        |index| format!("Some(CONTENTS[{index}])"),
    )
}

/// The file `diff` changes, as it is after the change, from its contents
/// `before` it. Every hunk must match exactly where it says it applies.
/// Returns `None` if the change deletes the file.
fn apply(before: Option<&str>, diff: &FileDiff) -> Result<Option<String>, String> {
    let path = &diff.path;
    match (diff.kind, before) {
        (FileChange::Created, Some(_)) => {
            return Err(format!("the patch creates {path}, which already exists"));
        }
        (FileChange::Modified | FileChange::Deleted, None) => {
            return Err(format!("the patch changes {path}, which does not exist"));
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
            return Err(format!(
                "a hunk for {path} starts at line {}, out of order or past the end",
                hunk.old_start
            ));
        }
        after.extend(old[next..start].iter().copied());
        let mut at = start;
        for line in &hunk.lines {
            match line {
                HunkLine::Context(text) | HunkLine::Removed(text) => {
                    if old.get(at) != Some(&text.as_str()) {
                        return Err(format!(
                            "a hunk does not match {path} at line {}: expected {text:?}",
                            at + 1
                        ));
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
    match diff.kind {
        FileChange::Deleted if after.is_empty() => Ok(None),
        FileChange::Deleted => Err(format!("the patch deletes {path} but leaves lines in it")),
        FileChange::Created | FileChange::Modified => Ok(Some(after)),
    }
}
