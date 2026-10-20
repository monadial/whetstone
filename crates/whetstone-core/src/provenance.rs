//! Provenance: what produced each result, saved with it in the session
//! record, so that any outcome can be traced to the models that produced
//! it and what they cost.
//!
//! Each request to Jev or the generator has its own [`Provenance`]. Usage
//! and cost are kept only as a reply reports them, never estimated.

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
