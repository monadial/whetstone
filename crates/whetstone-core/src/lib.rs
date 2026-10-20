//! Domain types for Whetstone Practice.
//!
//! This crate holds the vocabulary from `GLOSSARY.md` (Practice, Showcase
//! project, Change, Area, ...) as plain data and pure logic. It performs no
//! I/O: no files, network, processes, environment or terminal. Its
//! `clippy.toml` disallows the common standard-library I/O entry points, so
//! lints catch most breaches; review covers the rest.

mod area;
mod change;
mod connected_code_limit;
mod connection;
mod evidence;
mod exclusion;
mod repository_config;
mod secret;
mod showcase;
mod toml_error;

pub use crate::area::{Area, AreaFileError, AreaId, AreaSource, InvalidAreaFile, InvalidAreaId};
pub use crate::change::{
    Change, CommitId, Comparison, FileContent, InvalidCommitId, Origin, TouchedFile,
};
pub use crate::connected_code_limit::{
    ConnectedCodeLimit, ConnectedCodeLimitError, ConnectedCodeLimitKb, ConnectedCodeLimitSource,
};
pub use crate::connection::{
    Connection, ConnectionGroup, ConnectionId, Connections, Extent, InvalidLineRange, LineRange,
    Relation, Tier,
};
pub use crate::evidence::{
    ConnectionsState, Evidence, EvidenceConnection, EvidenceFile, Exclusion, FileIndex, FileStatus,
    Finding, FindingStatus, Place, ReviewChoices, SentConnection, SentFile, Side, Unsendable,
};
pub use crate::exclusion::{
    DefaultExclusion, DefaultExclusionKind, ExcludePattern, ExcludePatternError,
    ExcludePatternProblem, ExcludePatterns,
};
pub use crate::repository_config::{RepositoryConfig, RepositoryConfigError};
pub use crate::secret::{
    SECRET_RULES, SECRET_RULES_VERSION, SecretFinding, SecretRule, find_secrets,
};
pub use crate::showcase::ShowcaseProject;
