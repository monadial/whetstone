//! Provenance: what produced each result, saved with it in the session
//! record, so that any outcome can be traced to the models, versions,
//! questions and assessment policy that produced it, and what it cost.
//!
//! Each request to Jev or the generator has its own [`Provenance`]. Usage
//! and cost are kept only as a reply reports them, never estimated, and a
//! session's [`SessionCost`] adds up what was reported.

use std::fmt;

use crate::area::{AreaId, AreaSource};
use crate::assessment_policy::PolicyVersion;
use crate::jev::Probability;
use crate::question_check::Depth;

/// Who a request went to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// The evaluator, which returns probabilities and never writes prose.
    Jev,
    /// The model that writes questions, explanations and hints.
    Generator,
}

/// A model's identity on `OpenRouter`, such as `typesafe/jev-1.14`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModelId(String);

/// The tokens a reply says its request used, each counter where it reports
/// it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    /// The tokens the request sent.
    pub input_tokens: Option<u64>,
    /// The tokens the reply returned.
    pub output_tokens: Option<u64>,
}

/// What a reply says its request cost, in US dollars of `OpenRouter` credit,
/// exactly as reported.
///
/// ```
/// use whetstone_core::Cost;
///
/// assert_eq!(Cost::new(0.000_023_982)?.usd(), 0.000_023_982);
/// assert!(Cost::new(-0.01).is_err());
/// # Ok::<(), whetstone_core::InvalidCost>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Cost(f64);

/// A reported cost that is not one: negative, infinite or not a number.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[error("{0} is not a cost: it must be a finite number of dollars, 0 or more")]
pub struct InvalidCost(f64);

/// A Practice's cost: what each reply reported, added up for Jev and for the
/// generator, and how many requests reported none. It never estimates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SessionCost {
    jev: Cost,
    generator: Cost,
    not_reported: usize,
}

/// What produced one request's result: Whetstone's own identity for the
/// request; who it went to; the model asked for and the model that
/// answered; the versions behind it; for Jev, the exact questions sent and
/// the answers as returned; and its usage and cost where the reply reports
/// them.
///
/// Its `Debug` output shows no question's text.
#[derive(Debug, Clone, PartialEq)]
pub struct Provenance {
    request: RequestId,
    role: Role,
    requested_model: ModelId,
    reported_model: Option<ModelId>,
    versions: Versions,
    exchanges: Vec<JevExchange>,
    usage: Usage,
    cost: Option<Cost>,
}

/// Whetstone's own identity for a request, unique within a run.
/// `OpenRouter`'s generation ID joins it once Rig exposes that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestId(u64);

/// The versions behind a request: Whetstone's own, its base prompts', the
/// assessment policy's, and each area file's that went into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Versions {
    whetstone: String,
    base_prompts: ContentHash,
    policy: PolicyVersion,
    area_files: Vec<AreaFileVersion>,
}

/// An area file that went into a request: its area, where the file comes
/// from and a hash of its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaFileVersion {
    area: AreaId,
    source: AreaSource,
    content: ContentHash,
}

/// A hash of some content, as lowercase hexadecimal digits: the version of
/// Whetstone's base prompts, or of an area file. Whoever holds the content
/// computes it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContentHash(String);

/// Text that is not a content hash: empty, or not lowercase hexadecimal
/// digits.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a content hash: use lowercase hexadecimal digits")]
pub struct InvalidContentHash(String);

/// One question Whetstone sent Jev, exactly as sent, with Jev's answer,
/// exactly as returned. Its `Debug` output shows the question's name and
/// answer, never its text.
#[derive(Clone, PartialEq)]
pub enum JevExchange {
    /// A yes/no question, answered with the probability of yes.
    YesNo {
        /// The question's name in the request, such as `option_a`.
        name: String,
        /// What Jev was asked.
        instructions: String,
        /// The description of yes, if one was sent.
        yes: Option<String>,
        /// The description of no, if one was sent.
        no: Option<String>,
        /// Jev's answer.
        answer: Probability,
    },
    /// A score question over ordered levels, answered with a distribution
    /// and its expected value: the depth question.
    Score {
        /// The question's name in the request, such as `depth`.
        name: String,
        /// What Jev was asked.
        instructions: String,
        /// The description of each level, from the lowest.
        levels: Vec<String>,
        /// Jev's answer.
        answer: Depth,
    },
}

impl ModelId {
    /// The model `id`, as `OpenRouter` names it.
    #[must_use]
    pub fn new(id: String) -> Self {
        Self(id)
    }

    /// The ID as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Cost {
    /// The cost `usd`, if it is one: a finite number of dollars, 0 or more.
    pub fn new(usd: f64) -> Result<Self, InvalidCost> {
        if usd.is_finite() && usd >= 0.0 {
            Ok(Self(usd))
        } else {
            Err(InvalidCost(usd))
        }
    }

    /// The cost in US dollars.
    #[must_use]
    pub fn usd(self) -> f64 {
        self.0
    }
}

impl SessionCost {
    /// The cost of a Practice whose requests are `requests`: the costs their
    /// replies reported, added up by role, and how many reported none.
    #[must_use]
    pub fn of<'a>(requests: impl IntoIterator<Item = &'a Provenance>) -> Self {
        let mut jev = 0.0;
        let mut generator = 0.0;
        let mut not_reported = 0;
        for request in requests {
            match (request.role, request.cost) {
                (Role::Jev, Some(cost)) => jev += cost.0,
                (Role::Generator, Some(cost)) => generator += cost.0,
                (_, None) => not_reported += 1,
            }
        }
        Self {
            jev: Cost(jev),
            generator: Cost(generator),
            not_reported,
        }
    }

    /// What Jev's replies reported.
    #[must_use]
    pub fn jev(self) -> Cost {
        self.jev
    }

    /// What the generator's replies reported.
    #[must_use]
    pub fn generator(self) -> Cost {
        self.generator
    }

    /// What every reply reported, Jev's and the generator's together.
    #[must_use]
    pub fn total(self) -> Cost {
        Cost(self.jev.0 + self.generator.0)
    }

    /// How many requests' replies reported no cost: the summary says "not
    /// reported for *n* requests" instead of estimating theirs.
    #[must_use]
    pub fn not_reported(self) -> usize {
        self.not_reported
    }
}

impl RequestId {
    /// The request numbered `id`.
    #[must_use]
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// The identity as a number.
    #[must_use]
    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Versions {
    /// The versions behind a request: Whetstone's own `whetstone` version,
    /// such as `0.1.0`; the hash of its `base_prompts`; the assessment
    /// `policy`'s version; and the `area_files` that went into it.
    #[must_use]
    pub fn new(
        whetstone: String,
        base_prompts: ContentHash,
        policy: PolicyVersion,
        area_files: Vec<AreaFileVersion>,
    ) -> Self {
        Self {
            whetstone,
            base_prompts,
            policy,
            area_files,
        }
    }

    /// Whetstone's version.
    #[must_use]
    pub fn whetstone(&self) -> &str {
        &self.whetstone
    }

    /// The version of Whetstone's base prompts: a hash over all of them.
    #[must_use]
    pub fn base_prompts(&self) -> &ContentHash {
        &self.base_prompts
    }

    /// The assessment policy's version.
    #[must_use]
    pub fn policy(&self) -> PolicyVersion {
        self.policy
    }

    /// The area files that went into the request.
    #[must_use]
    pub fn area_files(&self) -> &[AreaFileVersion] {
        &self.area_files
    }
}

impl AreaFileVersion {
    /// The file for `area`, from `source`, whose content hashes to
    /// `content`.
    #[must_use]
    pub fn new(area: AreaId, source: AreaSource, content: ContentHash) -> Self {
        Self {
            area,
            source,
            content,
        }
    }

    /// The area the file defines.
    #[must_use]
    pub fn area(&self) -> &AreaId {
        &self.area
    }

    /// Where the file comes from.
    #[must_use]
    pub fn source(&self) -> AreaSource {
        self.source
    }

    /// The hash of its content.
    #[must_use]
    pub fn content(&self) -> &ContentHash {
        &self.content
    }
}

impl ContentHash {
    /// The hash written as `hex`, if it is lowercase hexadecimal digits.
    pub fn new(hex: String) -> Result<Self, InvalidContentHash> {
        if !hex.is_empty()
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            Ok(Self(hex))
        } else {
            Err(InvalidContentHash(hex))
        }
    }

    /// The hash as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl JevExchange {
    /// The question's name in the request.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::YesNo { name, .. } | Self::Score { name, .. } => name,
        }
    }
}

/// Shows the question's name, the text's lengths and the answer, never the
/// text.
impl fmt::Debug for JevExchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::YesNo {
                name,
                instructions,
                yes,
                no,
                answer,
            } => f
                .debug_struct("YesNo")
                .field("name", name)
                .field("instructions_bytes", &instructions.len())
                .field("yes_bytes", &yes.as_ref().map(String::len))
                .field("no_bytes", &no.as_ref().map(String::len))
                .field("answer", answer)
                .finish(),
            Self::Score {
                name,
                instructions,
                levels,
                answer,
            } => f
                .debug_struct("Score")
                .field("name", name)
                .field("instructions_bytes", &instructions.len())
                .field(
                    "levels_bytes",
                    &levels.iter().map(String::len).collect::<Vec<usize>>(),
                )
                .field("answer", answer)
                .finish(),
        }
    }
}

impl Provenance {
    /// The `request` to `role`, asking for `requested_model`, behind which
    /// are `versions`. Until the reply says otherwise, it reports no model,
    /// answers, usage or cost.
    #[must_use]
    pub fn new(
        request: RequestId,
        role: Role,
        requested_model: ModelId,
        versions: Versions,
    ) -> Self {
        Self {
            request,
            role,
            requested_model,
            reported_model: None,
            versions,
            exchanges: Vec::new(),
            usage: Usage::default(),
            cost: None,
        }
    }

    /// The same, with the questions sent to Jev, each with its answer as
    /// returned. A request to the generator has none.
    #[must_use]
    pub fn with_exchanges(self, exchanges: Vec<JevExchange>) -> Self {
        Self { exchanges, ..self }
    }

    /// The same, with the model the reply says answered.
    #[must_use]
    pub fn with_reported_model(self, reported_model: ModelId) -> Self {
        Self {
            reported_model: Some(reported_model),
            ..self
        }
    }

    /// The same, with the usage the reply reports.
    #[must_use]
    pub fn with_usage(self, usage: Usage) -> Self {
        Self { usage, ..self }
    }

    /// The same, with the cost the reply reports.
    #[must_use]
    pub fn with_cost(self, cost: Cost) -> Self {
        Self {
            cost: Some(cost),
            ..self
        }
    }

    /// Whetstone's own identity for the request.
    #[must_use]
    pub fn request(&self) -> RequestId {
        self.request
    }

    /// Who the request went to.
    #[must_use]
    pub fn role(&self) -> Role {
        self.role
    }

    /// The model asked for.
    #[must_use]
    pub fn requested_model(&self) -> &ModelId {
        &self.requested_model
    }

    /// The model the reply says answered, if it says.
    #[must_use]
    pub fn reported_model(&self) -> Option<&ModelId> {
        self.reported_model.as_ref()
    }

    /// The versions behind the request.
    #[must_use]
    pub fn versions(&self) -> &Versions {
        &self.versions
    }

    /// For Jev, the exact questions sent, each with its answer as returned,
    /// in the order sent. A request to the generator has none.
    #[must_use]
    pub fn exchanges(&self) -> &[JevExchange] {
        &self.exchanges
    }

    /// The tokens the reply says the request used.
    #[must_use]
    pub fn usage(&self) -> Usage {
        self.usage
    }

    /// What the reply says the request cost, if it says.
    #[must_use]
    pub fn cost(&self) -> Option<Cost> {
        self.cost
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assessment_policy::AssessmentPolicy;
    use crate::question_check::tests::{depth, probability};

    fn model(id: &str) -> ModelId {
        ModelId::new(id.to_owned())
    }

    fn hash(hex: &str) -> ContentHash {
        ContentHash::new(hex.to_owned()).expect("lowercase hexadecimal")
    }

    fn versions() -> Versions {
        Versions::new(
            "0.1.0".to_owned(),
            hash("9f2c"),
            AssessmentPolicy::current().version(),
            vec![AreaFileVersion::new(
                "domain".parse().expect("an area ID"),
                AreaSource::Default,
                hash("41ab"),
            )],
        )
    }

    #[test]
    fn a_request_reports_nothing_until_its_reply_does() {
        let provenance = Provenance::new(
            RequestId::new(7),
            Role::Generator,
            model("anthropic/claude"),
            versions(),
        );

        assert_eq!(provenance.request(), RequestId::new(7));
        assert_eq!(provenance.role(), Role::Generator);
        assert_eq!(provenance.requested_model().as_str(), "anthropic/claude");
        assert_eq!(provenance.reported_model(), None);
        assert_eq!(provenance.versions(), &versions());
        assert_eq!(provenance.exchanges(), []);
        assert_eq!(provenance.usage(), Usage::default());
        assert_eq!(provenance.cost(), None);
    }

    #[test]
    fn a_request_keeps_the_versions_behind_it() {
        let provenance = Provenance::new(
            RequestId::new(3),
            Role::Jev,
            model("typesafe/jev-1.14"),
            versions(),
        );

        let versions = provenance.versions();
        assert_eq!(versions.whetstone(), "0.1.0");
        assert_eq!(versions.base_prompts().as_str(), "9f2c");
        assert_eq!(versions.policy().get(), 0);
        let [area_file] = versions.area_files() else {
            panic!("one area file: {versions:?}");
        };
        assert_eq!(area_file.area().as_str(), "domain");
        assert_eq!(area_file.source(), AreaSource::Default);
        assert_eq!(area_file.content().to_string(), "41ab");
        assert_eq!(provenance.request().to_string(), "3");
    }

    #[test]
    fn a_jev_request_keeps_the_exact_questions_sent_and_the_answers_as_returned() {
        let exchanges = vec![
            JevExchange::YesNo {
                name: "option_a".to_owned(),
                instructions: "Given the evidence, this option is right.".to_owned(),
                yes: Some("It answers the question".to_owned()),
                no: Some("It doesn't".to_owned()),
                answer: probability(0.91),
            },
            JevExchange::Score {
                name: "depth".to_owned(),
                instructions: "How much does answering need?".to_owned(),
                levels: vec!["One line".to_owned(), "The change".to_owned()],
                answer: depth(1.25),
            },
        ];

        let provenance = Provenance::new(
            RequestId::new(1),
            Role::Jev,
            model("typesafe/jev-1.14"),
            versions(),
        )
        .with_exchanges(exchanges.clone());

        assert_eq!(provenance.exchanges(), exchanges);
        let names: Vec<&str> = provenance
            .exchanges()
            .iter()
            .map(JevExchange::name)
            .collect();
        assert_eq!(names, ["option_a", "depth"]);
        let debug = format!("{provenance:?}");
        for text in [
            "Given the evidence",
            "answers the question",
            "much does",
            "One line",
        ] {
            assert!(!debug.contains(text), "{debug}");
        }
        assert!(debug.contains("option_a"), "{debug}");
    }

    #[test]
    fn a_content_hash_is_lowercase_hexadecimal_digits() {
        assert_eq!(hash("0123456789abcdef").as_str(), "0123456789abcdef");
        for hex in ["", "ABC", "12g4", "ab cd"] {
            assert_eq!(
                ContentHash::new(hex.to_owned()),
                Err(InvalidContentHash(hex.to_owned())),
                "{hex:?}"
            );
        }
    }

    #[test]
    fn a_request_keeps_what_its_reply_reports() {
        let cost = Cost::new(0.000_023_982).expect("a cost");
        let usage = Usage {
            input_tokens: Some(571),
            output_tokens: Some(73),
        };

        let provenance = Provenance::new(
            RequestId::new(2),
            Role::Jev,
            model("typesafe/jev-1.14"),
            versions(),
        )
        .with_reported_model(model("typesafe/jev-1.14-20261014"))
        .with_usage(usage)
        .with_cost(cost);

        assert_eq!(provenance.role(), Role::Jev);
        assert_eq!(
            provenance.requested_model().to_string(),
            "typesafe/jev-1.14"
        );
        assert_eq!(
            provenance.reported_model().map(ModelId::as_str),
            Some("typesafe/jev-1.14-20261014")
        );
        assert_eq!(provenance.usage(), usage);
        assert_eq!(provenance.cost(), Some(cost));
    }

    fn request(role: Role, usd: Option<f64>) -> Provenance {
        let provenance = Provenance::new(RequestId::new(0), role, model("some/model"), versions());
        match usd {
            Some(usd) => provenance.with_cost(Cost::new(usd).expect("a cost")),
            None => provenance,
        }
    }

    #[test]
    fn a_sessions_cost_adds_up_what_each_reply_reported_by_role() {
        let requests = [
            request(Role::Jev, Some(0.25)),
            request(Role::Generator, Some(1.5)),
            request(Role::Jev, Some(0.125)),
            request(Role::Generator, Some(0.5)),
        ];

        let cost = SessionCost::of(&requests);

        assert_eq!(cost.jev(), Cost::new(0.375).expect("a cost"));
        assert_eq!(cost.generator(), Cost::new(2.0).expect("a cost"));
        assert_eq!(cost.total(), Cost::new(2.375).expect("a cost"));
        assert_eq!(cost.not_reported(), 0);
    }

    #[test]
    fn a_request_without_a_reported_cost_is_counted_never_estimated() {
        let requests = [
            request(Role::Jev, Some(0.25)),
            request(Role::Jev, None),
            request(Role::Generator, None),
        ];

        let cost = SessionCost::of(&requests);

        assert_eq!(cost.jev(), Cost::new(0.25).expect("a cost"));
        assert_eq!(cost.generator(), Cost::new(0.0).expect("a cost"));
        assert_eq!(cost.total(), Cost::new(0.25).expect("a cost"));
        assert_eq!(cost.not_reported(), 2);
    }

    #[test]
    fn a_session_without_requests_cost_nothing() {
        let cost = SessionCost::of([]);

        assert_eq!(cost.total(), Cost::new(0.0).expect("a cost"));
        assert_eq!(cost.not_reported(), 0);
    }

    #[test]
    fn a_cost_is_a_finite_number_of_dollars_0_or_more() {
        for usd in [0.0, 0.000_011_886, 2.5] {
            assert!(
                Cost::new(usd).expect("a cost").usd().to_bits() == usd.to_bits(),
                "{usd}"
            );
        }
        for usd in [-0.000_001, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(Cost::new(usd).is_err(), "{usd}");
        }
    }
}
