//! Spike for "Run one Jev evaluation through OpenRouter on Rig 0.45".
//!
//! Sends one evaluation with the three question shapes Practice needs (a
//! relevance gate, per-option ratings, a reasoning criterion) to Jev through
//! OpenRouter's `/systemone`, on Rig 0.45.0, and writes what Rig decoded plus
//! the request body to `replies/`. The state is made up, not repository code.
//!
//! Run with `OPENROUTER_API_KEY` set: `cargo run --release`.

use anyhow::{Context, Result};
use rig::error::ProviderError;
use rig::typesafeai::{Choice, ChoiceAnswer, Evaluate, JevConfig, Noul, NoulAnswer, Query};
use serde::{Deserialize, Serialize};
use serde_json::json;

const MODEL: &str = "typesafe/jev-1.14";
const ENDPOINT: &str = "https://openrouter.ai/api/v1/systemone";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
enum Letter {
    A,
    B,
    C,
}

#[derive(Debug, Serialize, Deserialize)]
struct Practice<G = NoulAnswer, O = ChoiceAnswer<Letter>, C = NoulAnswer> {
    gate: G,
    options: O,
    criterion: C,
}

type PracticeQuery = Practice<Noul, Choice<Letter>, Noul>;

impl PracticeQuery {
    fn new() -> Result<Self, ProviderError> {
        Ok(Self {
            gate: Noul::new(
                "The change in the state gives a learner something worth answering about the area in the state.",
            )?,
            options: Choice::<Letter>::new(
                "Why does the change reuse the original operation key when it retries a charge?",
                [
                    (
                        Letter::A,
                        "So the payment provider treats the retry as the same operation and charges at most once",
                    ),
                    (Letter::B, "So the retry runs faster by skipping validation"),
                    (Letter::C, "So the logs group both attempts together"),
                ],
            )?,
            criterion: Noul::new(
                "The learner's reasoning explains that reusing the key lets the payment provider recognise the retry as a duplicate.",
            )?
            .criteria(
                "The reasoning says the provider deduplicates the retried charge by its key",
                "The reasoning doesn't connect the key to the provider deduplicating the charge",
            )?,
        })
    }
}

impl<G: Query, O: Query, C: Query> Query for Practice<G, O, C> {
    type Response = Practice<G::Response, O::Response, C::Response>;
    type Output = Practice<G::Output, O::Output, C::Output>;

    fn decode(&self, response: Self::Response) -> Result<Self::Output, ProviderError> {
        Ok(Practice {
            gate: self.gate.decode(response.gate)?,
            options: self.options.decode(response.options)?,
            criterion: self.criterion.decode(response.criterion)?,
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let key = std::env::var("OPENROUTER_API_KEY").context("OPENROUTER_API_KEY is not set")?;
    let state = json!({
        "area": "Safe retries: a retried payment operation must never charge twice.",
        "change": {
            "summary": "Retry a card charge after a timeout.",
            "diff": "-    let key = OperationKey::new();\n+    let key = original.key.clone();\n     client.charge(amount, key).await?;",
        },
        "learner_reasoning": "Using the same key means the provider sees one operation, so a retry after a timeout can't take the money twice.",
    });
    let query = PracticeQuery::new()?;

    // The exact body Rig sends, for the raw requests in raw.sh.
    let request = json!({ "model": MODEL, "state": state, "questions": query });
    std::fs::write("replies/request.json", serde_json::to_string_pretty(&request)?)?;

    let jev = JevConfig::new(key)
        .model(MODEL)
        .with_endpoint(ENDPOINT)
        .client()
        .evaluation();
    let result = jev.evaluate(&state, &query).await?;

    let decoded = json!({
        "model": result.model,
        "provider_request_id": result.provider_request_id,
        "usage": format!("{:?}", result.usage),
        "answers": result.answers,
    });
    std::fs::write("replies/rig-decoded.json", serde_json::to_string_pretty(&decoded)?)?;
    println!("{}", serde_json::to_string_pretty(&decoded)?);
    Ok(())
}
