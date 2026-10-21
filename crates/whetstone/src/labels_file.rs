//! The learner's labels file: their votes on the checks that withheld
//! questions, `labels.toml` in Whetstone's state folder, beside the log.
//! Each vote is one labelled example in the format the labelled-example tool
//! reads (`showcase/labels/README.md`): a candidate example with
//! `source = "learner vote"` and a label for the one check voted on, saying
//! what Jev should have answered. "Rightly withheld" is the answer that
//! withholds the candidate, and "Should have passed" the answer the show
//! rule needed.
//!
//! The file is created owner-only, it is never sent anywhere, and nothing in
//! it goes to the log (rules 43, 44): a vote is logged by its number alone.
//! Votes about the learner's own repository hold private code from it.
//!
//! Each vote is appended as it is cast, by a background job, off the UI loop
//! (rule 19), one append at a time. The run remembers each vote cast and
//! whether it was saved, so a review opened again shows it, and a vote saved
//! once is never asked for again in the run.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use toml::value::{Date, Datetime};
use whetstone_ai::question_check_name;
use whetstone_core::{Band, Candidate, CandidateCheck, OptionLetter};

use crate::config;
use crate::environment::Environment;

/// The labels file's name in the state folder.
const FILE_NAME: &str = "labels.toml";

/// What heads the file once it is created.
const HEADER: &str = "\
# Your votes on the checks that withheld questions in Whetstone's Practice.
# Each is a labelled example the labelled-example tool reads, with
# `whetstone-labels check --labels` and this file's path. Nothing here is
# ever sent anywhere. Votes about your own repository hold private code
# from it.
";

/// Where the learner's votes are kept, and the votes cast during this run.
/// The app keeps one for the whole run, shared with the review of withheld
/// questions and with the jobs that save its votes.
pub struct LabelsFile {
    /// The file, if Whetstone has a state folder to keep it in.
    path: Option<PathBuf>,
    /// The file's path as the review shows it.
    shown: String,
    /// What time it is, as each vote is dated.
    clock: fn() -> SystemTime,
    /// Taken while a vote is appended, so appends never interleave. Only
    /// the jobs that save votes take it.
    appending: Mutex<()>,
    /// Each vote cast during this run, by the candidate and check it is on.
    votes: Mutex<Votes>,
}

/// The votes cast during a run.
#[derive(Debug, Default)]
struct Votes {
    /// How many votes were cast, the last one's number.
    cast: usize,
    /// Each check voted on, with the vote's state.
    on: BTreeMap<(CandidateKey, CandidateCheck), VoteState>,
}

/// A candidate, as votes on it are remembered: by everything it says, since
/// a Retry's candidates reuse the places of those before.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CandidateKey(String);

/// Shows its length, never the candidate's text.
impl fmt::Debug for CandidateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CandidateKey").field(&self.0.len()).finish()
    }
}

impl CandidateKey {
    fn of(candidate: &Candidate) -> Self {
        let mut text = format!("{}\u{0}{}", candidate.area(), candidate.stem());
        for letter in OptionLetter::ALL {
            text.push('\u{0}');
            text.push_str(candidate.option(letter));
        }
        Self(text)
    }
}

/// The learner's verdict on one check that withheld a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The check was right to withhold it.
    RightlyWithheld,
    /// The candidate should have passed the check.
    ShouldHavePassed,
}

impl Verdict {
    /// The verdict as the learner chooses it.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::RightlyWithheld => "Rightly withheld",
            Self::ShouldHavePassed => "Should have passed",
        }
    }
}

/// Where a vote stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VoteState {
    /// Cast, and being saved.
    Saving(Verdict),
    /// Saved to the labels file.
    Saved(Verdict),
    /// It couldn't be saved, for this reason: the learner can vote again.
    Unsaved(Verdict, String),
}

/// The change a candidate was written about, as a vote names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VotedChange {
    /// The commit's subject.
    pub(crate) subject: String,
    /// The commit's full ID, when it is one of the learner's own
    /// repository's rather than the showcase project's.
    pub(crate) commit_id: Option<String>,
}

/// One vote, as it is saved.
#[derive(Debug, Clone)]
pub(crate) struct Vote {
    pub(crate) change: VotedChange,
    /// The candidate withheld.
    pub(crate) candidate: Candidate,
    /// The check voted on.
    pub(crate) check: CandidateCheck,
    /// The band the show rule needed for the check.
    pub(crate) needed: Band,
    pub(crate) verdict: Verdict,
    /// The learner's note, if they wrote one.
    pub(crate) note: Option<String>,
}

/// A vote that is cast, for a job to save.
#[derive(Debug)]
pub(crate) struct Cast {
    /// Its number among the votes cast during the run, from 1.
    number: usize,
    /// When it was cast.
    at: SystemTime,
    vote: Vote,
}

impl LabelsFile {
    /// The labels file in `state_dir`, Whetstone's state folder, shown as
    /// `~/…` under the home directory `environment` names, with votes dated
    /// by `clock`.
    #[must_use]
    pub fn in_state_dir(
        state_dir: &Path,
        environment: &Environment,
        clock: fn() -> SystemTime,
    ) -> Self {
        let path = state_dir.join(FILE_NAME);
        Self {
            shown: config::shown_path(&path, environment),
            path: Some(path),
            clock,
            appending: Mutex::new(()),
            votes: Mutex::default(),
        }
    }

    /// No labels file: votes can't be saved.
    pub(crate) fn none() -> Self {
        Self {
            path: None,
            shown: String::new(),
            clock: SystemTime::now,
            appending: Mutex::new(()),
            votes: Mutex::default(),
        }
    }

    /// The file's path as the review shows it, if there is a file.
    pub(crate) fn shown(&self) -> Option<&str> {
        self.path.as_ref().map(|_| self.shown.as_str())
    }

    /// Where the vote on `check` of `candidate` stands, if one was cast
    /// during this run.
    pub(crate) fn vote(&self, candidate: &Candidate, check: CandidateCheck) -> Option<VoteState> {
        self.votes()
            .on
            .get(&(CandidateKey::of(candidate), check))
            .cloned()
    }

    /// Casts `vote`, which is then being saved, and returns it for a job to
    /// [save](Self::save).
    pub(crate) fn cast(&self, vote: Vote) -> Cast {
        let mut votes = self.votes();
        votes.cast += 1;
        votes.on.insert(
            (CandidateKey::of(&vote.candidate), vote.check),
            VoteState::Saving(vote.verdict),
        );
        Cast {
            number: votes.cast,
            at: (self.clock)(),
            vote,
        }
    }

    /// Appends the `cast` vote to the file, creating it owner-only if it
    /// isn't there, and remembers whether it was saved. It blocks, so only
    /// a background job calls it.
    pub(crate) fn save(&self, cast: &Cast) {
        let saved = match &self.path {
            Some(path) => self.append(path, cast),
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Whetstone has no state folder to keep votes in",
            )),
        };
        let verdict = cast.vote.verdict;
        let state = match saved {
            Ok(()) => {
                tracing::info!(vote = cast.number, "saved a vote to the labels file");
                VoteState::Saved(verdict)
            }
            Err(error) => {
                tracing::warn!(vote = cast.number, %error, "a vote wasn't saved to the labels file");
                VoteState::Unsaved(verdict, error.to_string())
            }
        };
        self.votes().on.insert(
            (CandidateKey::of(&cast.vote.candidate), cast.vote.check),
            state,
        );
    }

    /// Appends `cast` to the file at `path` as a labelled example.
    fn append(&self, path: &Path, cast: &Cast) -> io::Result<()> {
        let text = example(cast).map_err(io::Error::other)?;
        // Appends take turns, and a poisoned lock guards nothing broken.
        let _appending = self
            .appending
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)?;
        let text = if file.metadata()?.len() == 0 {
            format!("{HEADER}\n{text}")
        } else {
            format!("\n{text}")
        };
        file.write_all(text.as_bytes())?;
        file.flush()
    }

    fn votes(&self) -> MutexGuard<'_, Votes> {
        // It is held only to read or change the map, never across a wait or
        // a write, so a poisoned lock can only follow a panic that is
        // already ending the app; the map is still whole.
        self.votes.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Shows where the file is and how many votes were cast, never a vote.
impl fmt::Debug for LabelsFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LabelsFile")
            .field("path", &self.path)
            .field("cast", &self.votes().cast)
            .finish_non_exhaustive()
    }
}

/// `cast` as a labelled example: a `[[candidate]]` table in TOML.
fn example(cast: &Cast) -> Result<String, toml::ser::Error> {
    let vote = &cast.vote;
    let candidate = &vote.candidate;
    let (date, time) = civil(cast.at);
    let expected = match (vote.verdict, vote.needed) {
        (Verdict::ShouldHavePassed, Band::ClearYes) | (Verdict::RightlyWithheld, Band::ClearNo) => {
            "yes"
        }
        _ => "no",
    };
    let file = ExampleFile {
        candidate: [Example {
            id: format!(
                "learner-vote-{:04}{:02}{:02}T{time}-{}",
                date.year, date.month, date.day, cast.number
            ),
            source: "learner vote",
            by: "learner",
            on: Datetime {
                date: Some(date),
                time: None,
                offset: None,
            },
            note: vote.note.as_deref(),
            commit: &vote.change.subject,
            commit_id: vote.change.commit_id.as_deref(),
            area: candidate.area().as_str(),
            stem: candidate.stem(),
            answer_key: candidate.answer_key().to_string(),
            why_it_matters: candidate.why_it_matters(),
            hint: candidate.hint().text(),
            options: OptionLetter::ALL
                .map(|letter| ExampleOption {
                    letter: letter.to_string(),
                    text: candidate.option(letter),
                    rationale: candidate.rationale(letter),
                })
                .into(),
            rubric: candidate
                .rubric()
                .iter()
                .map(|criterion| ExampleCriterion {
                    statement: criterion.statement(),
                    met: criterion.met(),
                    not_met: criterion.not_met(),
                })
                .collect(),
            citations: candidate
                .citations()
                .iter()
                .map(|citation| ExampleCitation {
                    path: citation.path(),
                    first_line: citation.lines().first(),
                    last_line: citation.lines().last(),
                })
                .collect(),
            labels: BTreeMap::from([(question_check_name(vote.check), expected)]),
        }],
    };
    toml::to_string(&file)
}

/// A file of one example, as TOML writes it.
#[derive(Serialize)]
struct ExampleFile<'a> {
    candidate: [Example<'a>; 1],
}

/// A candidate example, its plain keys before its tables, as TOML needs.
#[derive(Serialize)]
struct Example<'a> {
    id: String,
    source: &'static str,
    by: &'static str,
    on: Datetime,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<&'a str>,
    commit: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    commit_id: Option<&'a str>,
    area: &'a str,
    stem: &'a str,
    answer_key: String,
    why_it_matters: &'a str,
    hint: &'a str,
    options: Vec<ExampleOption<'a>>,
    rubric: Vec<ExampleCriterion<'a>>,
    citations: Vec<ExampleCitation<'a>>,
    labels: BTreeMap<String, &'static str>,
}

#[derive(Serialize)]
struct ExampleOption<'a> {
    letter: String,
    text: &'a str,
    rationale: &'a str,
}

#[derive(Serialize)]
struct ExampleCriterion<'a> {
    statement: &'a str,
    met: &'a str,
    not_met: &'a str,
}

#[derive(Serialize)]
struct ExampleCitation<'a> {
    path: &'a str,
    first_line: usize,
    last_line: usize,
}

/// The UTC day `at` falls on, and its time of day as `HHMMSS`. A time
/// before 1970 counts as 1970's first second.
fn civil(at: SystemTime) -> (Date, String) {
    let seconds = at
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let days = seconds / 86_400;
    let of_day = seconds % 86_400;
    (
        date(days),
        format!(
            "{:02}{:02}{:02}",
            of_day / 3_600,
            of_day % 3_600 / 60,
            of_day % 60
        ),
    )
}

/// The day `days` after 1 January 1970, in the proleptic Gregorian
/// calendar (Howard Hinnant's `civil_from_days`).
fn date(days: u64) -> Date {
    let z = days + 719_468;
    let era = z / 146_097;
    let day_of_era = z % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    Date {
        year: u16::try_from(year).unwrap_or(u16::MAX),
        month: u8::try_from(month).expect("a month is from 1 to 12"),
        day: u8::try_from(day).expect("a day of the month is from 1 to 31"),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    #[test]
    fn a_vote_is_dated_by_the_utc_day_and_time_it_was_cast() {
        let day = |seconds| {
            let (date, time) = civil(at(seconds));
            format!("{date} {time}")
        };

        assert_eq!(day(0), "1970-01-01 000000");
        // 2026-10-21 10:15:02 UTC.
        assert_eq!(day(1_792_577_702), "2026-10-21 101502");
        // The last second of a leap day, and the first of the next.
        assert_eq!(day(951_868_799), "2000-02-29 235959");
        assert_eq!(day(951_868_800), "2000-03-01 000000");
        assert_eq!(day(1_735_689_599), "2024-12-31 235959");
    }
}
