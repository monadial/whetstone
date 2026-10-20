//! Whetstone's base prompts: the files in this crate's `prompts/` directory,
//! bundled into the binary at build time, as area files are.
//!
//! The generator's preambles are Markdown; Jev's instructions are TOML, one
//! table per question, with what yes and no mean. A repository can't
//! replace any of them: nothing takes a base prompt from outside this crate.
//! A repository's area prompt only adds focus to them.
//!
//! [`base_prompts_version`] is a hash over every file, so that provenance
//! names exactly the prompts a result came from.

use std::fmt::Write as _;
use std::sync::LazyLock;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

use crate::jev::JevQuestion;

/// How the generator writes candidate questions; `{count}` is how many.
pub(crate) const WRITING: &str = include_str!("../prompts/writing.md");

/// What the evidence holds and how to cite it, for the generator.
pub(crate) const EVIDENCE: &str = include_str!("../prompts/evidence.md");

/// How the generator explains what reasoning missed.
pub(crate) const EXPLANATION: &str = include_str!("../prompts/explanation.md");

/// Introduces an area's prompt; `{name}`, `{description}` and `{prompt}` are
/// the area's.
pub(crate) const AREA: &str = include_str!("../prompts/area.md");

const EXPLANATION_CHECK_FILE: &str = include_str!("../prompts/explanation-check.toml");

const GATE_FILE: &str = include_str!("../prompts/gate.toml");

const QUESTION_CHECK_FILE: &str = include_str!("../prompts/question-check.toml");

const REASONING_FILE: &str = include_str!("../prompts/reasoning.toml");

/// Every base-prompt file, by name, in name order.
const FILES: &[(&str, &str)] = &[
    ("area.md", AREA),
    ("evidence.md", EVIDENCE),
    ("explanation-check.toml", EXPLANATION_CHECK_FILE),
    ("explanation.md", EXPLANATION),
    ("gate.toml", GATE_FILE),
    ("question-check.toml", QUESTION_CHECK_FILE),
    ("reasoning.toml", REASONING_FILE),
    ("writing.md", WRITING),
];

/// The explanation check's questions about one paragraph; `{criterion}` is
/// its criterion's name.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExplanationCheckPrompts {
    pub(crate) supported: YesNoPrompt,
    pub(crate) agrees: YesNoPrompt,
}

/// The relevance gate's question about one area.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GatePrompts {
    /// `{name}` and `{description}` are the area's.
    pub(crate) area: YesNoPrompt,
}

/// The question check's questions about one candidate.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuestionCheckPrompts {
    pub(crate) answerable: YesNoPrompt,
    pub(crate) about_area: YesNoPrompt,
    pub(crate) needs_change: YesNoPrompt,
    pub(crate) no_giveaway: YesNoPrompt,
    /// One criterion's check; `{criterion}` is the criterion's name.
    pub(crate) criterion: YesNoPrompt,
    /// One option's rating; `{letter}` is the option's.
    pub(crate) option: YesNoPrompt,
    /// One rationale's check; `{letter}` is its option's.
    pub(crate) rationale: YesNoPrompt,
    pub(crate) why_it_matters: YesNoPrompt,
    pub(crate) hint_points_to_evidence: YesNoPrompt,
    pub(crate) hint_hides_answer_key: YesNoPrompt,
    /// The depth score, over its three levels.
    pub(crate) depth: ScorePrompt,
}

/// Reasoning's question about one criterion.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReasoningPrompts {
    /// `{statement}` is the criterion's statement; the criterion's own
    /// descriptions say what yes and no mean.
    pub(crate) criterion: InstructionsPrompt,
}

/// A yes/no question to Jev whose meanings of yes and no come from
/// elsewhere: its instructions.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstructionsPrompt {
    instructions: String,
}

impl InstructionsPrompt {
    /// The question, with each placeholder filled from `values`, and `yes`
    /// and `no` as what yes and no mean.
    pub(crate) fn question(&self, values: &[(&str, &str)], yes: &str, no: &str) -> JevQuestion {
        JevQuestion::yes_no_with_meanings(fill(&self.instructions, values), yes, no)
    }
}

/// A yes/no question to Jev: its instructions, and what yes and no mean.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct YesNoPrompt {
    instructions: String,
    yes: String,
    no: String,
}

pub(crate) static EXPLANATION_CHECK: LazyLock<ExplanationCheckPrompts> =
    LazyLock::new(|| parse(EXPLANATION_CHECK_FILE));

pub(crate) static GATE: LazyLock<GatePrompts> = LazyLock::new(|| parse(GATE_FILE));

pub(crate) static QUESTION_CHECK: LazyLock<QuestionCheckPrompts> =
    LazyLock::new(|| parse(QUESTION_CHECK_FILE));

pub(crate) static REASONING: LazyLock<ReasoningPrompts> = LazyLock::new(|| parse(REASONING_FILE));

static VERSION: LazyLock<String> = LazyLock::new(|| {
    let mut hash = Sha256::new();
    for (name, content) in FILES {
        // Each file's name and length frame its content, so that moving
        // text from one file to the next changes the hash.
        hash.update(name.as_bytes());
        hash.update([0]);
        hash.update(
            u64::try_from(content.len())
                .expect("a base prompt is smaller than 2^64 bytes")
                .to_le_bytes(),
        );
        hash.update(content.as_bytes());
    }
    hash.finalize()
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            write!(hex, "{byte:02x}").expect("writing to a String never fails");
            hex
        })
});

/// The version of Whetstone's base prompts: a SHA-256 hash over every
/// base-prompt file, its name and its content, as 64 lowercase hexadecimal
/// digits. Any change to any base prompt changes it.
///
/// ```
/// let version = whetstone_ai::base_prompts_version();
///
/// assert_eq!(version.len(), 64);
/// assert!(version.chars().all(|digit| matches!(digit, '0'..='9' | 'a'..='f')));
/// ```
#[must_use]
pub fn base_prompts_version() -> &'static str {
    &VERSION
}

/// A score question to Jev: its instructions, and its three levels, the
/// lowest first.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScorePrompt {
    instructions: String,
    levels: [String; 3],
}

impl ScorePrompt {
    /// The question.
    pub(crate) fn question(&self) -> JevQuestion {
        JevQuestion::score(&self.instructions, &self.levels)
    }
}

impl YesNoPrompt {
    /// The question, with each placeholder filled from `values`.
    pub(crate) fn question(&self, values: &[(&str, &str)]) -> JevQuestion {
        JevQuestion::yes_no_with_meanings(
            fill(&self.instructions, values),
            fill(&self.yes, values),
            fill(&self.no, values),
        )
    }
}

/// `template` with each `{key}` of `values` replaced by its value. It reads
/// the template once, so text it fills in is never read as a placeholder,
/// and leaves any other brace as it is.
pub(crate) fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut filled = String::with_capacity(template.len());
    let mut rest = template;
    while let Some((before, after)) = rest.split_once('{') {
        filled.push_str(before);
        let placeholder = after.split_once('}').and_then(|(key, tail)| {
            values
                .iter()
                .find(|&&(name, _)| name == key)
                .map(|&(_, value)| (value, tail))
        });
        if let Some((value, tail)) = placeholder {
            filled.push_str(value);
            rest = tail;
        } else {
            filled.push('{');
            rest = after;
        }
    }
    filled.push_str(rest);
    filled
}

/// The bundled TOML `text`, as `T`.
fn parse<T: DeserializeOwned>(text: &str) -> T {
    toml::from_str(text)
        .expect("Whetstone's bundled base prompts are valid: every builder's tests read them")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn the_version_covers_every_file_in_the_prompts_directory() {
        let on_disk: BTreeSet<String> =
            std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts"))
                .expect("the prompts directory exists")
                .map(|entry| {
                    entry
                        .expect("the entry is readable")
                        .file_name()
                        .into_string()
                        .expect("the file name is UTF-8")
                })
                .collect();
        let hashed: Vec<&str> = FILES.iter().map(|&(name, _)| name).collect();

        assert_eq!(
            hashed,
            on_disk.iter().map(String::as_str).collect::<Vec<_>>()
        );
    }
}
