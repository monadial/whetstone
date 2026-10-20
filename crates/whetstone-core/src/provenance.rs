//! Provenance: what produced each result, saved with it in the session
//! record, so that any outcome can be traced to the models that produced
//! it and what they cost.
//!
//! Each request to Jev or the generator has its own [`Provenance`]. Usage
//! and cost are kept only as a reply reports them, never estimated, and a
//! session's [`SessionCost`] adds up what was reported.

use std::fmt;

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

/// What produced one request's result: who it went to, the model asked for
/// and the model that answered, and its usage and cost where the reply
/// reports them.
#[derive(Debug, Clone, PartialEq)]
pub struct Provenance {
    role: Role,
    requested_model: ModelId,
    reported_model: Option<ModelId>,
    usage: Usage,
    cost: Option<Cost>,
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

impl Provenance {
    /// A request to `role`, asking for `requested_model`. Until the reply
    /// says otherwise, it reports no model, usage or cost.
    #[must_use]
    pub fn new(role: Role, requested_model: ModelId) -> Self {
        Self {
            role,
            requested_model,
            reported_model: None,
            usage: Usage::default(),
            cost: None,
        }
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

    fn model(id: &str) -> ModelId {
        ModelId::new(id.to_owned())
    }

    #[test]
    fn a_request_reports_nothing_until_its_reply_does() {
        let provenance = Provenance::new(Role::Generator, model("anthropic/claude"));

        assert_eq!(provenance.role(), Role::Generator);
        assert_eq!(provenance.requested_model().as_str(), "anthropic/claude");
        assert_eq!(provenance.reported_model(), None);
        assert_eq!(provenance.usage(), Usage::default());
        assert_eq!(provenance.cost(), None);
    }

    #[test]
    fn a_request_keeps_what_its_reply_reports() {
        let cost = Cost::new(0.000_023_982).expect("a cost");
        let usage = Usage {
            input_tokens: Some(571),
            output_tokens: Some(73),
        };

        let provenance = Provenance::new(Role::Jev, model("typesafe/jev-1.14"))
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
        let provenance = Provenance::new(role, model("some/model"));
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
