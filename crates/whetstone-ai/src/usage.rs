//! What a call used and cost, as `OpenRouter` reported it.

/// A call's token counts and cost, each as the reply reported it. A field the
/// reply left out is `None`: nothing here is ever estimated.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Usage {
    /// Input tokens.
    pub input_tokens: Option<u64>,
    /// Output tokens, reasoning included.
    pub output_tokens: Option<u64>,
    /// What the call cost, in US dollars of `OpenRouter` credit.
    pub cost: Option<f64>,
}

impl Usage {
    /// Rig's usage, keeping only the cost the provider reported in full.
    pub(crate) fn from_rig(usage: &rig::completion::Usage) -> Self {
        Self {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cost: usage
                .cost
                .filter(rig::completion::Cost::is_complete)
                .map(|cost| cost.total),
        }
    }
}
