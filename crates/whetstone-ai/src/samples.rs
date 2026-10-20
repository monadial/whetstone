//! Test support: the made-up change, area and candidate the requests'
//! fixtures were recorded with, and the helpers that run a built request
//! through a local server replaying a fixture (rule 48). None of it is
//! repository code.

use std::time::Duration;

use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use whetstone_core::{
    Area, AssessmentPolicy, Candidate, CandidateCheck, Change, ConnectedCodeLimit, Connection,
    ConnectionGroup, Connections, CriterionOutcome, Depth, Evidence, ExcludePatterns, Extent,
    LineRange, ModelId, OptionLetter, Probability, Question, QuestionCheckAnswers, QuestionType,
    Relation, ReviewChoices, Tier, TouchedFile, WrittenCandidate, WrittenCitation,
    WrittenCriterion, WrittenOption, plain_checks, show_rule,
};

use crate::replay::{GENERATOR_MODEL, JEV_MODEL, connection, received, server};
use crate::{
    AiError, GeneratorReply, GeneratorRequest, JevReply, JevRequest, Saved, Timeouts,
    WrittenParagraph, explanation_paragraphs,
};

pub(crate) fn jev_model() -> ModelId {
    ModelId::new(JEV_MODEL.to_owned())
}

pub(crate) fn generator_model() -> ModelId {
    ModelId::new(GENERATOR_MODEL.to_owned())
}

/// The area the samples are about.
pub(crate) fn area() -> Area {
    area_file(
        "domain.md",
        "+++\n\
         name = \"Domain\"\n\
         description = \"The rules, concepts and language the work is about\"\n\
         +++\n\
         \n\
         Focus on the rules the change encodes: what must always hold, and what\n\
         would go wrong if it didn't.\n",
    )
}

/// A second area, which the sample change hardly touches.
pub(crate) fn observability() -> Area {
    area_file(
        "observability.md",
        "+++\n\
         name = \"Observability\"\n\
         description = \"What the running system tells you about itself\"\n\
         +++\n\
         \n\
         Look at the logs, metrics and traces the change adds or changes.\n",
    )
}

/// The area defined by the area file `name` holding `contents`.
pub(crate) fn area_file(name: &str, contents: &str) -> Area {
    Area::offered([(name, Ok(contents.as_bytes()))])
        .remove(0)
        .expect("the sample area file is valid")
}

const RETRY_BEFORE: &str = "\
pub async fn charge_with_retry(client: &Client, original: &Charge) -> Result<Receipt, Error> {
    match client.charge(original.amount, original.key.clone()).await {
        Err(Error::Timeout) => {
            let key = OperationKey::new();
            client.charge(original.amount, key).await
        }
        result => result,
    }
}
";

const RETRY_AFTER: &str = "\
pub async fn charge_with_retry(client: &Client, original: &Charge) -> Result<Receipt, Error> {
    match client.charge(original.amount, original.key.clone()).await {
        Err(Error::Timeout) => {
            // The provider treats a repeated key as the same charge.
            let key = original.key.clone();
            client.charge(original.amount, key).await
        }
        result => result,
    }
}
";

const DIFF: &str = "\
diff --git a/src/payments/retry.rs b/src/payments/retry.rs
--- a/src/payments/retry.rs
+++ b/src/payments/retry.rs
@@ -1,9 +1,10 @@
 pub async fn charge_with_retry(client: &Client, original: &Charge) -> Result<Receipt, Error> {
     match client.charge(original.amount, original.key.clone()).await {
         Err(Error::Timeout) => {
-            let key = OperationKey::new();
+            // The provider treats a repeated key as the same charge.
+            let key = original.key.clone();
             client.charge(original.amount, key).await
         }
         result => result,
     }
 }
diff --git a/CHANGELOG.md b/CHANGELOG.md
--- a/CHANGELOG.md
+++ b/CHANGELOG.md
@@ -1,3 +1,5 @@
 # Changelog
\x20
 ## Unreleased
+
+- A timed-out charge is retried with its original key, so it is never charged twice.
diff --git a/Cargo.lock b/Cargo.lock
--- a/Cargo.lock
+++ b/Cargo.lock
@@ -1 +1 @@
-version = 3
+version = 4
";

/// The client's `charge`, which is lines 12–16 of its file.
const CHARGE: &str = "    /// Charges `amount`. The provider charges at most once per key: a
    /// request with a key it has seen returns the first charge's receipt.
    pub async fn charge(&self, amount: Money, key: OperationKey) -> Result<Receipt, Error> {
        self.post(\"/charges\", &ChargeRequest { amount, idempotency_key: key }).await
    }
";

const TEST: &str = "\
#[tokio::test]
async fn a_timed_out_charge_is_retried_once() {
    let client = FakeClient::timing_out_once();
    charge_with_retry(&client, &charge()).await.unwrap();
    assert_eq!(client.charges().len(), 2);
}
";

/// Evidence for a change that makes a timed-out charge's retry reuse its
/// key: it sends `src/payments/retry.rs` with the client's `charge` as
/// connected code and its test, and `CHANGELOG.md` with no connected code.
/// `Cargo.lock` is left out by default, so nothing of it is sent.
pub(crate) fn evidence() -> Evidence {
    let retry = "src/payments/retry.rs";
    let change = Change::new(
        "Reuse the charge's key when retrying after a timeout".to_owned(),
        Vec::new(),
        DIFF.to_owned(),
        vec![
            TouchedFile::modified(
                retry.to_owned(),
                RETRY_BEFORE.to_owned(),
                RETRY_AFTER.to_owned(),
            ),
            TouchedFile::modified(
                "CHANGELOG.md".to_owned(),
                "# Changelog\n\n## Unreleased\n".to_owned(),
                "# Changelog\n\n## Unreleased\n\n\
                 - A timed-out charge is retried with its original key, so it is never charged twice.\n"
                    .to_owned(),
            ),
            TouchedFile::modified(
                "Cargo.lock".to_owned(),
                "version = 3\n".to_owned(),
                "version = 4\n".to_owned(),
            ),
        ],
    );
    let connections = Connections::Found(vec![
        Connection::new(
            "src/payments/client.rs".to_owned(),
            Extent::Lines(LineRange::new(12, 16).expect("lines 12 to 16 are a range")),
            ConnectionGroup::ConnectedCode,
            Tier::High,
            vec![Relation::CalledBy {
                callable: "charge_with_retry".to_owned(),
                touched_file: retry.to_owned(),
            }],
            format!("{}{CHARGE}", "\n".repeat(11)).into(),
        ),
        Connection::new(
            "tests/retry.rs".to_owned(),
            Extent::WholeFile,
            ConnectionGroup::RelatedTest,
            Tier::High,
            vec![Relation::TestFor {
                touched_file: retry.to_owned(),
            }],
            TEST.to_owned().into(),
        ),
    ]);
    Evidence::build(
        &change,
        &ExcludePatterns::default(),
        &[area()],
        &connections,
        ConnectedCodeLimit::effective(None, None),
        &ReviewChoices::default(),
    )
}

/// A candidate the generator might write for [`evidence`], with option A as
/// its answer key and two criteria, as written.
pub(crate) fn written() -> WrittenCandidate {
    let option = |letter: &str, text: &str, rationale: &str| WrittenOption {
        letter: letter.to_owned(),
        text: text.to_owned(),
        rationale: rationale.to_owned(),
    };
    let criterion = |statement: &str, met: &str, not_met: &str| WrittenCriterion {
        statement: statement.to_owned(),
        met: met.to_owned(),
        not_met: not_met.to_owned(),
    };
    let cite = |path: &str, first_line, last_line| WrittenCitation {
        path: path.to_owned(),
        first_line,
        last_line,
    };
    WrittenCandidate {
        question_type: QuestionType::SingleChoice,
        stem: "Why does the retry in `charge_with_retry` now reuse `original.key` \
               instead of making a new `OperationKey`?"
            .to_owned(),
        options: vec![
            option(
                "A",
                "So that the provider treats the retry as the same charge, and a charge \
                 that went through before the timeout isn't made twice.",
                "Correct: `Client::charge` says the provider charges at most once per key \
                 and returns the first charge's receipt for a key it has seen.",
            ),
            option(
                "B",
                "So that the retry skips the provider's checks on the card and completes \
                 faster than the first attempt.",
                "Wrong: nothing in the evidence says a repeated key skips checks or speeds \
                 anything up.",
            ),
            option(
                "C",
                "So that the provider rejects the retry if the first attempt failed, and \
                 the caller sees the original timeout.",
                "Wrong: for a key it has seen, the provider returns the first charge's \
                 receipt; a timeout is retried, not passed on.",
            ),
        ],
        answer_key: "A".to_owned(),
        why_it_matters: "A timeout doesn't say whether a charge went through, so a retry \
                         must never be able to charge the customer twice."
            .to_owned(),
        rubric: vec![
            criterion(
                "Explains that a timed-out charge may still have gone through.",
                "Says the first charge might have succeeded even though the client saw a timeout.",
                "Treats the timeout as proof that nothing was charged, or doesn't consider it.",
            ),
            criterion(
                "Connects reusing the key to the provider charging at most once per key.",
                "Says the provider recognises the repeated key and returns the first charge \
                 instead of charging again.",
                "Doesn't say how the reused key prevents a second charge.",
            ),
        ],
        hint: "Read what `Client::charge` says about a key the provider has already seen."
            .to_owned(),
        citations: vec![
            cite("src/payments/retry.rs", 3, 6),
            cite("src/payments/client.rs", 12, 14),
        ],
    }
}

/// [`written`] after the plain checks against [`evidence`].
pub(crate) fn candidate() -> Candidate {
    let area = area();
    let mut passed = plain_checks(area.id(), vec![written()], &evidence()).into_passed();
    assert_eq!(
        passed.len(),
        1,
        "the sample candidate passes the plain checks"
    );
    passed.remove(0)
}

/// [`candidate`] after a question check it passes: every check clear yes,
/// but options B and C clear no.
pub(crate) fn question() -> Question {
    let candidate = candidate();
    let answer = |value| Probability::new(value).expect("the value is a probability");
    let answers = QuestionCheckAnswers::new(
        candidate.checks().into_iter().map(|check| {
            let wrong = [
                CandidateCheck::Option(OptionLetter::B),
                CandidateCheck::Option(OptionLetter::C),
            ];
            (
                check,
                answer(if wrong.contains(&check) { 0.05 } else { 0.95 }),
            )
        }),
        Depth::new([answer(0.1), answer(0.3), answer(0.6)], 1.5).expect("the depth is one"),
    );
    show_rule(candidate, answers, AssessmentPolicy::current()).expect("the sample passes")
}

/// A learner's reasoning for [`question`]: it connects the reused key to the
/// provider charging once per key, but never says the timed-out charge may
/// have gone through.
pub(crate) const REASONING: &str = "The provider only charges once for each key and \
     gives back the first receipt when it sees a key again. Reusing the original key \
     means the retry can't turn into a second charge.";

/// A session store's acknowledgement that it saved a response whose
/// reasoning is `0`, as the app's store gives one.
#[derive(Debug)]
pub(crate) struct Acknowledged(pub(crate) &'static str);

impl Saved for Acknowledged {
    fn reasoning(&self) -> &str {
        self.0
    }
}

/// The outcomes of [`question`]'s two criteria that the sample explanation
/// explains: the first not met, the second needing review.
pub(crate) const OUTCOMES: [CriterionOutcome; 2] =
    [CriterionOutcome::NotMet, CriterionOutcome::NeedsReview];

/// The paragraphs of the explanation recorded for [`OUTCOMES`].
pub(crate) fn paragraphs() -> Vec<WrittenParagraph> {
    let reply: Value = serde_json::from_str(include_str!("../fixtures/explanation.json"))
        .expect("the reply is JSON");
    let output = reply["choices"][0]["message"]["content"]
        .as_str()
        .expect("the reply has content");
    explanation_paragraphs(
        &OUTCOMES,
        serde_json::from_str(output).expect("the recorded output decodes"),
    )
    .expect("the recorded output explains each criterion once")
}

/// Sends `request` to Jev through a local server that replies with
/// `reply`, and returns the decoded reply and the body that was sent.
pub(crate) async fn evaluated(
    request: &JevRequest,
    reply: &str,
) -> (Result<JevReply, AiError>, Value) {
    let server = server("/systemone", 200, reply, Duration::ZERO).await;
    let result = connection(&server, Timeouts::default())
        .evaluate(request, &CancellationToken::new())
        .await;
    (result, received(&server).await.1)
}

/// Runs `request` through a local server that replies with `reply`, and
/// returns the decoded reply and the body that was sent.
pub(crate) async fn generated<T>(
    request: &GeneratorRequest,
    reply: &str,
) -> (Result<GeneratorReply<T>, AiError>, Value)
where
    T: JsonSchema + DeserializeOwned + Send + 'static,
{
    let server = server("/chat/completions", 200, reply, Duration::ZERO).await;
    let result = connection(&server, Timeouts::default())
        .generate::<T>(request, &CancellationToken::new())
        .await;
    (result, received(&server).await.1)
}

/// A generator request `body` as a reviewable snapshot: the body as JSON
/// with its preamble and input set apart, then the preamble as text and the
/// input as JSON.
pub(crate) fn generator_body(mut body: Value) -> String {
    let preamble = body["messages"][0]["content"][0]["text"].take();
    let input = body["messages"][1]["content"].take();
    let input: Value = serde_json::from_str(input.as_str().expect("the input is text"))
        .expect("the input is JSON");
    format!(
        "{}\n\n--- preamble ---\n\n{}\n\n--- input ---\n\n{}\n",
        serde_json::to_string_pretty(&body).expect("the body is JSON"),
        preamble.as_str().expect("the preamble is text"),
        serde_json::to_string_pretty(&input).expect("the input is JSON"),
    )
}

/// `value` as pretty JSON, for a snapshot.
pub(crate) fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).expect("the value is JSON")
}

/// The recorded chat `reply` with its message's content replaced by
/// `output`, for a test of output the generator didn't write.
pub(crate) fn with_output(reply: &str, output: &Value) -> String {
    let mut reply: Value = serde_json::from_str(reply).expect("the reply is JSON");
    reply["choices"][0]["message"]["content"] = Value::String(output.to_string());
    reply.to_string()
}
