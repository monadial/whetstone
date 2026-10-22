//! Domain types for Whetstone Practice.
//!
//! This crate holds the vocabulary from `GLOSSARY.md` (Practice, Showcase
//! project, Change, Area, ...) as plain data and pure logic. It performs no
//! I/O: no files, network, processes, environment or terminal. Its
//! `clippy.toml` disallows the common standard-library I/O entry points, so
//! lints catch most breaches; review covers the rest.

mod area;
mod assessment;
mod assessment_policy;
mod change;
mod connected_code_limit;
mod connection;
mod evidence;
mod excerpt;
mod exclusion;
mod jev;
mod picking;
mod provenance;
mod question;
mod question_check;
mod repository_config;
mod secret;
mod selection;
mod showcase;
mod shuffle;
mod toml_error;

pub use crate::area::{Area, AreaFileError, AreaId, AreaSource, InvalidAreaFile, InvalidAreaId};
pub use crate::assessment::{
    Assessed, ChoiceOutcome, CriterionOutcome, OptionRating, Reasoning, Response, WrittenReasoning,
};
pub use crate::assessment_policy::{
    AppliedCutOffs, AssessmentPolicy, Band, CutOffs, CutOffsOrigin, InvalidCutOffs, Job,
    PolicyVersion, Validation,
};
pub use crate::change::{
    Change, ChangeSize, CommitId, Comparison, FileContent, InvalidCommitId, Origin, TouchedFile,
};
pub use crate::connected_code_limit::{
    ConnectedCodeLimit, ConnectedCodeLimitError, ConnectedCodeLimitKb, ConnectedCodeLimitSource,
};
pub use crate::connection::{
    Connection, ConnectionGroup, ConnectionId, Connections, Extent, InvalidLineRange, LineRange,
    Relation, Tier, merge_into_whole_files,
};
pub use crate::evidence::{
    AreaText, ConnectionsState, Evidence, EvidenceConnection, EvidenceFile, Exclusion, FileIndex,
    FileStatus, Finding, FindingStatus, Place, ReviewChoices, SentConnection, SentFile, Side,
    Unsendable,
};
pub use crate::excerpt::{DiffMark, Excerpt, ExcerptLine, ExcerptSource};
pub use crate::exclusion::{
    DefaultExclusion, DefaultExclusionKind, ExcludePattern, ExcludePatternError,
    ExcludePatternProblem, ExcludePatterns,
};
pub use crate::jev::{InvalidProbability, Probability};
pub use crate::picking::{Picked, QuestionCount, QuestionCountError, pick};
pub use crate::provenance::{
    AreaFileVersion, ContentHash, Cost, GenerationId, InvalidContentHash, InvalidCost, JevExchange,
    ModelId, Provenance, RequestId, Role, SessionCost, Usage, Versions,
};
pub use crate::question::{
    Candidate, CandidateId, Citation, Criterion, DropReason, Hint, OptionLetter, Part, PlainChecks,
    QuestionType, WrittenCandidate, WrittenCitation, WrittenCriterion, WrittenOption, plain_checks,
};
pub use crate::question_check::{
    AnswerProbe, CandidateCheck, Depth, DepthLevel, InvalidDepth, Question, QuestionCheckAnswers,
    Withheld, show_rule,
};
pub use crate::repository_config::{RepositoryConfig, RepositoryConfigError};
pub use crate::secret::{SecretFinding, SecretRule, SecretRules, Severity, find_secrets};
pub use crate::selection::NotSent;
pub use crate::showcase::ShowcaseProject;
pub use crate::shuffle::shuffle_options;
