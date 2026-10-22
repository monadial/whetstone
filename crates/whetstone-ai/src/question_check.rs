//! The question check: four Jev calls per candidate, sent together, whose
//! questions are the candidate's checks. Each call's state holds only what
//! its questions are about ([`QuestionCheckCall`]), so the option ratings
//! are decided without the answer key, the hint is judged without it too,
//! and the answer probe answers the candidate from its stem and options
//! alone.
//!
//! [`question_check_request`] builds one call, [`question_check_answers`]
//! reads Jev's reply to it, and [`CheckCallAnswers::merged`] makes the four
//! calls' answers whetstone-core's [`QuestionCheckAnswers`] for the show
//! rule.

#[cfg(any(test, feature = "scripted"))]
use std::convert::Infallible;

use serde_json::{Map, Value, json};
use whetstone_core::{
    AnswerProbe, Candidate, CandidateCheck, Depth, Evidence, ModelId, OptionLetter, Probability,
    QuestionCheckAnswers,
};

use crate::answers::Answers;
use crate::evidence::evidence_as_sent;
use crate::failure::AiError;
use crate::jev::{JevQuestion, JevReply, JevRequest};
use crate::prompts::QUESTION_CHECK;

/// One of the four Jev calls of a candidate's question check. Each call's
/// state holds only what its questions are about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QuestionCheckCall {
    /// The option ratings, `option_a` to `option_c`. Its state is the
    /// evidence as sent, and of the candidate only its stem and its options'
    /// text: no answer key, rationale, rubric, hint, "why it matters" line
    /// or citation.
    OptionRatings,
    /// `hint_hides_answer_key`. Its state is the candidate's stem, its
    /// options' text and its hint: no evidence and no answer key.
    Hint,
    /// Every other check, and the depth score. Its state is the evidence as
    /// sent and the whole candidate.
    Candidate,
    /// The answer probe, `answer_probe`: a choice among the options. Its
    /// state is the candidate's stem and its options' text alone: no
    /// evidence, answer key, rationale, rubric, hint, "why it matters" line
    /// or citation.
    AnswerProbe,
}

/// Jev's answers to one call of a candidate's question check, exactly as
/// returned: the probability of yes for each yes/no check the call asked,
/// for the answer probe call its probe, and for the candidate call the
/// depth. [`merged`](Self::merged) makes the four calls' answers one
/// [`QuestionCheckAnswers`].
#[derive(Debug, Clone, PartialEq)]
pub struct CheckCallAnswers {
    /// The call answered, with what only it answers: the depth or the
    /// answer probe.
    answered: Answered,
    /// Each yes/no check the call asked, with Jev's answer, in the order
    /// asked.
    checks: Vec<(CandidateCheck, Probability)>,
}

/// Which call answered, with what only it answers: the depth, for the
/// candidate call, the only one that scores it
/// ([`QuestionCheckCall::scores_depth`]), and the probe, for the answer
/// probe call.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Answered {
    OptionRatings,
    Hint,
    Candidate(Depth),
    /// A reply from Jev always holds the probe. A scripted check may leave
    /// it out, as it may leave out any answer.
    AnswerProbe(Option<AnswerProbe>),
}

impl Answered {
    /// What `call` answered, read from `source`: with the depth `depth`
    /// reads when the call scores one, and with the probe `probe` reads for
    /// the answer probe call. Each is called only then.
    fn of<S, E>(
        call: QuestionCheckCall,
        source: S,
        depth: impl FnOnce(S) -> Result<Depth, E>,
        probe: impl FnOnce(S) -> Result<Option<AnswerProbe>, E>,
    ) -> Result<Self, E> {
        Ok(match call {
            QuestionCheckCall::OptionRatings => Self::OptionRatings,
            QuestionCheckCall::Hint => Self::Hint,
            QuestionCheckCall::Candidate => Self::Candidate(depth(source)?),
            QuestionCheckCall::AnswerProbe => Self::AnswerProbe(probe(source)?),
        })
    }

    /// The call that answered.
    fn call(self) -> QuestionCheckCall {
        match self {
            Self::OptionRatings => QuestionCheckCall::OptionRatings,
            Self::Hint => QuestionCheckCall::Hint,
            Self::Candidate(_) => QuestionCheckCall::Candidate,
            Self::AnswerProbe(_) => QuestionCheckCall::AnswerProbe,
        }
    }

    /// The depth, when the call scores one.
    fn depth(self) -> Option<Depth> {
        match self {
            Self::Candidate(depth) => Some(depth),
            Self::OptionRatings | Self::Hint | Self::AnswerProbe(_) => None,
        }
    }

    /// The answer probe, when the call asks it and it was answered.
    fn probe(self) -> Option<AnswerProbe> {
        match self {
            Self::AnswerProbe(probe) => probe,
            Self::OptionRatings | Self::Hint | Self::Candidate(_) => None,
        }
    }
}

impl QuestionCheckCall {
    /// Every call, in the order Practice sends them.
    pub const ALL: [Self; 4] = [
        Self::OptionRatings,
        Self::Hint,
        Self::Candidate,
        Self::AnswerProbe,
    ];

    /// The call that asks `check`.
    #[must_use]
    pub fn of(check: CandidateCheck) -> Self {
        match check {
            CandidateCheck::Option(_) => Self::OptionRatings,
            CandidateCheck::HintHidesAnswerKey => Self::Hint,
            CandidateCheck::AnswerProbe => Self::AnswerProbe,
            CandidateCheck::Answerable
            | CandidateCheck::AboutArea
            | CandidateCheck::Criterion(_)
            | CandidateCheck::CriterionMinimum(_)
            | CandidateCheck::Rationale(_)
            | CandidateCheck::WhyItMatters
            | CandidateCheck::HintPointsToEvidence => Self::Candidate,
        }
    }

    /// Whether this call also scores the candidate's depth: only the
    /// candidate call does.
    #[must_use]
    pub fn scores_depth(self) -> bool {
        match self {
            Self::Candidate => true,
            Self::OptionRatings | Self::Hint | Self::AnswerProbe => false,
        }
    }

    /// The checks of `candidate` this call asks, in [`Candidate::checks`]
    /// order.
    #[must_use]
    pub fn checks(self, candidate: &Candidate) -> Vec<CandidateCheck> {
        candidate
            .checks()
            .into_iter()
            .filter(|&check| Self::of(check) == self)
            .collect()
    }
}

impl CheckCallAnswers {
    /// The call answered.
    #[must_use]
    pub fn call(&self) -> QuestionCheckCall {
        self.answered.call()
    }

    /// Jev's answer to the yes/no check `check`, if the call asked it. The
    /// answer probe isn't one: see [`answer`](Self::answer).
    #[must_use]
    pub fn get(&self, check: CandidateCheck) -> Option<Probability> {
        self.checks
            .iter()
            .find_map(|&(asked, answer)| (asked == check).then_some(answer))
    }

    /// Jev's answer to `check` of a candidate whose answer key is
    /// `answer_key`, if the call asked it: the probability of yes for a
    /// yes/no check, and for the answer probe its probability on
    /// `answer_key`.
    #[must_use]
    pub fn answer(&self, check: CandidateCheck, answer_key: OptionLetter) -> Option<Probability> {
        match check {
            CandidateCheck::AnswerProbe => self
                .answered
                .probe()
                .map(|probe| probe.probability(answer_key)),
            _ => self.get(check),
        }
    }

    /// Each yes/no answer, by check, in the order the call asked them.
    pub fn iter(&self) -> impl Iterator<Item = (CandidateCheck, Probability)> + '_ {
        self.checks.iter().copied()
    }

    /// The answer probe, for the answer probe call; no other call asks it.
    #[must_use]
    pub fn probe(&self) -> Option<AnswerProbe> {
        self.answered.probe()
    }

    /// The depth, for the candidate call; no other call scores it.
    #[must_use]
    pub fn depth(&self) -> Option<Depth> {
        self.answered.depth()
    }

    /// The answers to a candidate's question check, from `answers`, its
    /// calls' answers: every yes/no check each call answered, the answer
    /// probe call's probe, and the candidate call's depth. `None` unless
    /// `answers` holds each call's answers once.
    #[must_use]
    pub fn merged(answers: impl IntoIterator<Item = Self>) -> Option<QuestionCheckAnswers> {
        let mut answered = Vec::new();
        let mut checks = Vec::new();
        let mut probe = None;
        let mut depth = None;
        for answers in answers {
            let call = answers.call();
            if answered.contains(&call) {
                return None;
            }
            answered.push(call);
            depth = depth.or(answers.depth());
            probe = probe.or(answers.probe());
            checks.extend(answers.checks);
        }
        if answered.len() != QuestionCheckCall::ALL.len() {
            return None;
        }
        let merged = QuestionCheckAnswers::new(checks, depth?);
        Some(match probe {
            Some(probe) => merged.with_probe(probe),
            None => merged,
        })
    }

    /// The answers of `call` that `answers` holds, its yes/no checks', and,
    /// for the answer probe call, the probe, and for the candidate call, the
    /// depth, as a scripted reply makes them.
    #[cfg(any(test, feature = "scripted"))]
    pub(crate) fn of_call(call: QuestionCheckCall, answers: &QuestionCheckAnswers) -> Self {
        let Ok(answered) = Answered::of(
            call,
            answers,
            |answers| Ok::<_, Infallible>(answers.depth()),
            |answers| Ok(answers.probe()),
        );
        Self {
            answered,
            checks: answers
                .iter()
                .filter(|&(check, _)| QuestionCheckCall::of(check) == call)
                .collect(),
        }
    }
}

/// The Jev request of the question check's `call` about `candidate`, with
/// `model`: one question per check of [`Candidate::checks`] that the call
/// asks, each named after its check, with its instructions from the base
/// prompts. Each is a yes/no question but the answer probe.
///
/// - The option ratings call asks `option_a` to `option_c`. Its state is
///   the evidence as sent, as `evidence`, and the candidate's stem and its
///   options' text, as `candidate`.
/// - The hint call asks `hint_hides_answer_key`. Its state is the
///   candidate's stem, its options' text and its hint, as `candidate`.
/// - The answer probe call asks `answer_probe`, a choice among the options
///   A, B and C: which correctly answers the stem. Its state is the
///   candidate's stem and its options' text, as `candidate`, and nothing
///   else.
/// - The candidate call asks every other check: `answerable` and
///   `about_area`; `criterion_1` and on, one per criterion;
///   `criterion_1_minimum` and on, one per criterion again, whether it asks
///   only for what the question needs; `rationale_a` to `rationale_c` and
///   `why_it_matters`; and `hint_points_to_evidence`. It also scores the
///   candidate's `depth` over three levels: answerable by reading one line,
///   needs reading the change, and needs an understanding of how the change
///   affects the code around it. Its state is the evidence as sent, as
///   `evidence`, and the whole candidate, as `candidate`: its area's name
///   and description, never its area prompt, its stem, its options with
///   their rationales, its answer key, its "why it matters" line, its
///   rubric, its hint and its citations.
#[must_use]
pub fn question_check_request(
    model: &ModelId,
    evidence: &Evidence,
    candidate: &Candidate,
    call: QuestionCheckCall,
) -> JevRequest {
    let state = match call {
        QuestionCheckCall::OptionRatings => json!({
            "evidence": evidence_as_sent(evidence),
            "candidate": {
                "stem": candidate.stem(),
                "options": options_text(candidate),
            },
        }),
        QuestionCheckCall::Hint => json!({
            "candidate": {
                "stem": candidate.stem(),
                "options": options_text(candidate),
                "hint": candidate.hint().text(),
            },
        }),
        QuestionCheckCall::AnswerProbe => json!({
            "candidate": {
                "stem": candidate.stem(),
                "options": options_text(candidate),
            },
        }),
        QuestionCheckCall::Candidate => json!({
            "evidence": evidence_as_sent(evidence),
            "candidate": candidate_state(evidence, candidate),
        }),
    };
    let depth = call
        .scores_depth()
        .then(|| (DEPTH.to_owned(), QUESTION_CHECK.depth.question()));
    JevRequest {
        model: model.as_str().to_owned(),
        state,
        questions: call
            .checks(candidate)
            .into_iter()
            .map(|check| (name(check), question(check)))
            .chain(depth)
            .collect(),
    }
}

/// Jev's answers to the question check's `call` about `candidate`, from
/// `reply`, exactly as returned: the probability of yes for each yes/no
/// check the call asked, for the answer probe call the probability of each
/// option, and for the candidate call the depth's distribution and expected
/// value. Jev's choice and `confidence` are never read.
///
/// Fails with [`AiError::InvalidReply`] when an answer is missing, of
/// another kind, not a probability, or to a question the call didn't ask,
/// when the answer probe's probabilities aren't for exactly the options A,
/// B and C, or when the depth isn't three probabilities with an expected
/// value from 0 to 2.
pub fn question_check_answers(
    candidate: &Candidate,
    call: QuestionCheckCall,
    reply: &JevReply,
) -> Result<CheckCallAnswers, AiError> {
    let mut answers = Answers::of(reply);
    // The answer probe is a choice, which `Answered` reads.
    let checks = call
        .checks(candidate)
        .into_iter()
        .filter(|&check| check != CandidateCheck::AnswerProbe)
        .map(|check| Ok((check, answers.yes_no(&name(check))?)))
        .collect::<Result<Vec<_>, AiError>>()?;
    let answered = Answered::of(
        call,
        &mut answers,
        |answers| answers.depth(DEPTH),
        |answers| {
            let letters = letters();
            let probabilities = answers.choice(
                &name(CandidateCheck::AnswerProbe),
                letters.iter().map(String::as_str),
            )?;
            Ok(Some(AnswerProbe::new(probabilities.try_into().expect(
                "a choice gives one probability for each option asked",
            ))))
        },
    )?;
    answers.done()?;
    Ok(CheckCallAnswers { answered, checks })
}

/// The name the depth score goes under.
pub(crate) const DEPTH: &str = "depth";

/// The name a check's question goes under, such as `criterion_1`,
/// `criterion_1_minimum` or `option_a`: Jev's name for it, which a labelled
/// example's label for it goes under too.
#[must_use]
pub fn name(check: CandidateCheck) -> String {
    match check {
        CandidateCheck::Answerable => "answerable".to_owned(),
        CandidateCheck::AboutArea => "about_area".to_owned(),
        CandidateCheck::AnswerProbe => "answer_probe".to_owned(),
        CandidateCheck::Criterion(index) => criterion_name(index),
        CandidateCheck::CriterionMinimum(index) => format!("{}_minimum", criterion_name(index)),
        CandidateCheck::Option(letter) => format!("option_{}", lowercase(letter)),
        CandidateCheck::Rationale(letter) => format!("rationale_{}", lowercase(letter)),
        CandidateCheck::WhyItMatters => "why_it_matters".to_owned(),
        CandidateCheck::HintPointsToEvidence => "hint_points_to_evidence".to_owned(),
        CandidateCheck::HintHidesAnswerKey => "hint_hides_answer_key".to_owned(),
    }
}

/// A check's question, from the base prompts.
fn question(check: CandidateCheck) -> JevQuestion {
    let prompts = &*QUESTION_CHECK;
    match check {
        CandidateCheck::Answerable => prompts.answerable.question(&[]),
        CandidateCheck::AboutArea => prompts.about_area.question(&[]),
        CandidateCheck::AnswerProbe => {
            let letters = letters();
            prompts
                .answer_probe
                .question(letters.iter().map(String::as_str))
        }
        CandidateCheck::Criterion(index) => prompts
            .criterion
            .question(&[("criterion", &criterion_name(index))]),
        CandidateCheck::CriterionMinimum(index) => prompts
            .criterion_minimum
            .question(&[("criterion", &criterion_name(index))]),
        CandidateCheck::Option(letter) => {
            prompts.option.question(&[("letter", &letter.to_string())])
        }
        CandidateCheck::Rationale(letter) => prompts
            .rationale
            .question(&[("letter", &letter.to_string())]),
        CandidateCheck::WhyItMatters => prompts.why_it_matters.question(&[]),
        CandidateCheck::HintPointsToEvidence => prompts.hint_points_to_evidence.question(&[]),
        CandidateCheck::HintHidesAnswerKey => prompts.hint_hides_answer_key.question(&[]),
    }
}

/// The options' letters, as the answer probe labels its choice's options.
fn letters() -> [String; 3] {
    OptionLetter::ALL.map(|letter| letter.to_string())
}

fn lowercase(letter: OptionLetter) -> String {
    letter.to_string().to_ascii_lowercase()
}

/// The candidate's options' text, by letter, as the learner reads them.
fn options_text(candidate: &Candidate) -> Map<String, Value> {
    OptionLetter::ALL
        .into_iter()
        .map(|letter| (letter.to_string(), json!(candidate.option(letter))))
        .collect()
}

/// The whole candidate, as the candidate call's state holds it.
fn candidate_state(evidence: &Evidence, candidate: &Candidate) -> Value {
    let mut area = json!({ "id": candidate.area().as_str() });
    // The area's name and description, from the areas the evidence was
    // approved with; its area prompt is never sent to Jev.
    if let Some(found) = evidence
        .areas()
        .iter()
        .find(|area| area.id() == candidate.area())
    {
        area["name"] = json!(found.name());
        area["description"] = json!(found.description());
    }
    let options: Map<String, Value> = OptionLetter::ALL
        .into_iter()
        .map(|letter| {
            (
                letter.to_string(),
                json!({
                    "text": candidate.option(letter),
                    "rationale": candidate.rationale(letter),
                }),
            )
        })
        .collect();
    let rubric: Map<String, Value> = candidate
        .rubric()
        .iter()
        .enumerate()
        .map(|(index, criterion)| {
            (
                criterion_name(index),
                json!({
                    "statement": criterion.statement(),
                    "met": criterion.met(),
                    "not_met": criterion.not_met(),
                }),
            )
        })
        .collect();
    let citations: Vec<Value> = candidate
        .citations()
        .iter()
        .map(|citation| {
            json!({
                "path": citation.path(),
                "first_line": citation.lines().first(),
                "last_line": citation.lines().last(),
            })
        })
        .collect();
    json!({
        "area": area,
        "stem": candidate.stem(),
        "options": options,
        "answer_key": candidate.answer_key().to_string(),
        "why_it_matters": candidate.why_it_matters(),
        "rubric": rubric,
        "hint": candidate.hint().text(),
        "citations": citations,
    })
}

/// The name of the rubric's criterion at `index`, counting from 0:
/// `criterion_1` for the first.
pub(crate) fn criterion_name(index: usize) -> String {
    format!("criterion_{}", index + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jev::JevAnswer;
    use crate::samples::{candidate, evaluated, evidence, jev_model, pretty, recorded_check};

    fn request(call: QuestionCheckCall) -> JevRequest {
        question_check_request(&jev_model(), &evidence(), &candidate(), call)
    }

    /// The body `call`'s request sends.
    async fn sent(call: QuestionCheckCall) -> Value {
        evaluated(&request(call), "{}").await.1
    }

    /// The names of the questions in the request `body`, in order.
    fn questions(body: &Value) -> Vec<&str> {
        body["questions"]
            .as_object()
            .expect("the questions are an object")
            .keys()
            .map(String::as_str)
            .collect()
    }

    /// The keys of the object at `value`, in order.
    fn keys(value: &Value) -> Vec<&str> {
        value
            .as_object()
            .expect("the state and the candidate in it are objects")
            .keys()
            .map(String::as_str)
            .collect()
    }

    /// The sample candidate's options' text, by letter.
    fn sample_options_text() -> Value {
        let candidate = candidate();
        json!({
            "A": candidate.option(OptionLetter::A),
            "B": candidate.option(OptionLetter::B),
            "C": candidate.option(OptionLetter::C),
        })
    }

    #[tokio::test]
    async fn each_call_of_the_question_check_sends_what_its_questions_are_about() {
        for (call, snapshot) in [
            (QuestionCheckCall::OptionRatings, "option_ratings_call"),
            (QuestionCheckCall::Hint, "hint_call"),
            (QuestionCheckCall::Candidate, "candidate_call"),
            (QuestionCheckCall::AnswerProbe, "answer_probe_call"),
        ] {
            insta::assert_snapshot!(snapshot, pretty(&sent(call).await));
        }
    }

    #[tokio::test]
    async fn the_option_ratings_call_sends_the_evidence_and_only_the_stem_and_the_options_text() {
        let body = sent(QuestionCheckCall::OptionRatings).await;

        assert_eq!(questions(&body), ["option_a", "option_b", "option_c"]);
        assert_eq!(keys(&body["state"]), ["evidence", "candidate"]);
        assert_eq!(
            body["state"]["candidate"],
            json!({ "stem": candidate().stem(), "options": sample_options_text() })
        );
        let text = body.to_string();
        let candidate = candidate();
        for (what, held) in [
            ("an answer key", "answer_key".to_owned()),
            (
                "a rationale",
                candidate.rationale(OptionLetter::A).to_owned(),
            ),
            (
                "a rationale",
                candidate.rationale(OptionLetter::B).to_owned(),
            ),
            (
                "a rationale",
                candidate.rationale(OptionLetter::C).to_owned(),
            ),
            ("a rubric", "rubric".to_owned()),
            ("a rubric", candidate.rubric()[0].statement().to_owned()),
            ("a rubric", candidate.rubric()[1].met().to_owned()),
            ("a hint", candidate.hint().text().to_owned()),
            (
                "a \"why it matters\" line",
                candidate.why_it_matters().to_owned(),
            ),
            ("a citation", "citations".to_owned()),
            ("a citation", "first_line".to_owned()),
        ] {
            assert!(!text.contains(&held), "the request holds {what}: {held:?}");
        }
    }

    #[tokio::test]
    async fn the_hint_call_sends_only_the_stem_the_options_text_and_the_hint() {
        let body = sent(QuestionCheckCall::Hint).await;

        assert_eq!(questions(&body), ["hint_hides_answer_key"]);
        let candidate = candidate();
        assert_eq!(
            body["state"],
            json!({
                "candidate": {
                    "stem": candidate.stem(),
                    "options": sample_options_text(),
                    "hint": candidate.hint().text(),
                },
            }),
            "no evidence and no answer key"
        );
        let text = body.to_string();
        assert!(!body["state"].to_string().contains("answer_key"));
        assert!(!text.contains(evidence().message()), "no evidence");
    }

    #[tokio::test]
    async fn the_answer_probe_call_sends_only_the_stem_and_the_options_text() {
        let body = sent(QuestionCheckCall::AnswerProbe).await;

        assert_eq!(questions(&body), ["answer_probe"]);
        assert_eq!(
            body["state"],
            json!({
                "candidate": {
                    "stem": candidate().stem(),
                    "options": sample_options_text(),
                },
            }),
            "the stem and the options' text, and nothing else"
        );
        let text = body.to_string();
        let candidate = candidate();
        let evidence = evidence();
        let mut held = vec![
            ("an answer key", "answer_key".to_owned()),
            ("evidence", "evidence".to_owned()),
            ("evidence", evidence.message().to_owned()),
            ("a rubric", "rubric".to_owned()),
            ("a hint", candidate.hint().text().to_owned()),
            (
                "a \"why it matters\" line",
                candidate.why_it_matters().to_owned(),
            ),
            ("a citation", "citations".to_owned()),
            ("a citation", "first_line".to_owned()),
        ];
        held.extend(
            OptionLetter::ALL.map(|letter| ("a rationale", candidate.rationale(letter).to_owned())),
        );
        held.extend(candidate.rubric().iter().flat_map(|criterion| {
            [criterion.statement(), criterion.met(), criterion.not_met()]
                .map(|text| ("a rubric", text.to_owned()))
        }));
        for (what, held) in held {
            assert!(!text.contains(&held), "the request holds {what}: {held:?}");
        }
    }

    #[tokio::test]
    async fn the_answer_probe_is_a_choice_among_the_options_by_letter() {
        let body = sent(QuestionCheckCall::AnswerProbe).await;

        let probe = &body["questions"]["answer_probe"];
        assert_eq!(probe["type"], "choice");
        assert_eq!(
            keys(&probe["criteria"]),
            ["A", "B", "C"],
            "one option per letter"
        );
    }

    #[tokio::test]
    async fn the_candidate_call_asks_every_other_check_and_the_depth() {
        let body = sent(QuestionCheckCall::Candidate).await;

        assert_eq!(
            questions(&body),
            [
                "about_area",
                "answerable",
                "criterion_1",
                "criterion_1_minimum",
                "criterion_2",
                "criterion_2_minimum",
                "depth",
                "hint_points_to_evidence",
                "rationale_a",
                "rationale_b",
                "rationale_c",
                "why_it_matters",
            ],
            "never the option ratings, hint_hides_answer_key or answer_probe"
        );
        assert_eq!(keys(&body["state"]), ["evidence", "candidate"]);
        assert_eq!(
            keys(&body["state"]["candidate"]),
            [
                "area",
                "stem",
                "options",
                "answer_key",
                "why_it_matters",
                "rubric",
                "hint",
                "citations"
            ],
            "the whole candidate"
        );
    }

    #[test]
    fn each_check_is_asked_by_exactly_one_call() {
        let candidate = candidate();
        let mut asked: Vec<CandidateCheck> = QuestionCheckCall::ALL
            .into_iter()
            .flat_map(|call| call.checks(&candidate))
            .collect();
        asked.sort_unstable();

        assert_eq!(asked, candidate.checks());
    }

    #[tokio::test]
    async fn a_question_check_sends_the_area_name_and_description_never_its_prompt() {
        let body = sent(QuestionCheckCall::Candidate).await;

        assert_eq!(
            body["state"]["candidate"]["area"],
            json!({
                "id": "domain",
                "name": "Domain",
                "description": "The rules, concepts and language the work is about",
            })
        );
        for call in QuestionCheckCall::ALL {
            assert!(
                !sent(call).await.to_string().contains("Focus on the rules"),
                "{call:?}"
            );
        }
    }

    /// The recorded reply to `call` of the sample candidate's check.
    async fn recorded(call: QuestionCheckCall) -> JevReply {
        evaluated(&request(call), recorded_check(call).1)
            .await
            .0
            .expect("the recorded reply decodes")
    }

    /// The recorded answers to `call`, decoded.
    async fn recorded_answers(call: QuestionCheckCall) -> CheckCallAnswers {
        question_check_answers(&candidate(), call, &recorded(call).await)
            .expect("the recorded reply answers every check the call asks")
    }

    fn probability(value: f64) -> Probability {
        Probability::new(value).expect("the value is a probability")
    }

    #[tokio::test]
    async fn recorded_answers_decode_as_returned_and_make_one_question_check() {
        let mut calls = Vec::new();
        for call in QuestionCheckCall::ALL {
            calls.push(recorded_answers(call).await);
        }
        for answers in &calls {
            assert_eq!(
                answers.depth().is_some(),
                answers.call().scores_depth(),
                "only the candidate call's"
            );
        }

        let answers = CheckCallAnswers::merged(calls).expect("each call's answers once");

        assert_eq!(
            answers.iter().collect::<Vec<_>>(),
            [
                (CandidateCheck::Answerable, 0.96),
                (CandidateCheck::AboutArea, 0.75),
                (CandidateCheck::Criterion(0), 0.81),
                (CandidateCheck::Criterion(1), 0.93),
                (CandidateCheck::CriterionMinimum(0), 0.86),
                (CandidateCheck::CriterionMinimum(1), 0.92),
                (CandidateCheck::Option(OptionLetter::A), 0.99),
                (CandidateCheck::Option(OptionLetter::B), 0.01),
                (CandidateCheck::Option(OptionLetter::C), 0.02),
                (CandidateCheck::Rationale(OptionLetter::A), 0.97),
                (CandidateCheck::Rationale(OptionLetter::B), 0.86),
                (CandidateCheck::Rationale(OptionLetter::C), 0.91),
                (CandidateCheck::WhyItMatters, 0.83),
                (CandidateCheck::HintPointsToEvidence, 0.96),
                (CandidateCheck::HintHidesAnswerKey, 0.27),
            ]
            .map(|(check, answer)| (check, probability(answer)))
        );
        assert_eq!(
            answers.probe(),
            Some(AnswerProbe::new([1.0, 0.0, 0.0].map(probability))),
            "Jev picks the sample's answer key, A, from its stem and options alone"
        );
        assert_eq!(
            answers.depth(),
            Depth::new(
                [probability(0.12), probability(0.26), probability(0.62)],
                1.5
            )
            .expect("the recorded expected depth is from 0 to 2")
        );
    }

    #[tokio::test]
    async fn the_calls_answers_make_one_question_check_only_with_each_call_once() {
        let [ratings, hint, candidate_call, probe] = [
            recorded_answers(QuestionCheckCall::OptionRatings).await,
            recorded_answers(QuestionCheckCall::Hint).await,
            recorded_answers(QuestionCheckCall::Candidate).await,
            recorded_answers(QuestionCheckCall::AnswerProbe).await,
        ];

        assert_eq!(
            CheckCallAnswers::merged([ratings.clone(), candidate_call.clone(), probe.clone()]),
            None,
            "the hint call's are missing"
        );
        assert_eq!(
            CheckCallAnswers::merged([
                ratings.clone(),
                hint.clone(),
                hint.clone(),
                candidate_call.clone(),
                probe.clone()
            ]),
            None,
            "the hint call's twice"
        );
        let merged = CheckCallAnswers::merged([probe.clone(), candidate_call, hint, ratings])
            .expect("in any order");
        assert_eq!(merged.iter().count(), 15, "every yes/no check");
        assert_eq!(merged.probe(), probe.probe(), "and the answer probe");
    }

    #[tokio::test]
    async fn a_depth_that_is_not_a_score_over_three_levels_is_an_invalid_reply() {
        for depth in [
            JevAnswer::YesNo { yes: 0.5 },
            JevAnswer::Score {
                probabilities: vec![0.4, 0.6],
                expected: 0.6,
            },
        ] {
            let mut reply = recorded(QuestionCheckCall::Candidate).await;
            reply.answers.insert("depth".to_owned(), depth);

            assert_eq!(
                question_check_answers(&candidate(), QuestionCheckCall::Candidate, &reply),
                Err(AiError::InvalidReply)
            );
        }
    }

    #[tokio::test]
    async fn a_missing_answer_in_any_call_is_an_invalid_reply() {
        for (call, missing) in [
            (QuestionCheckCall::OptionRatings, "option_b"),
            (QuestionCheckCall::AnswerProbe, "answer_probe"),
            (QuestionCheckCall::Hint, "hint_hides_answer_key"),
            (QuestionCheckCall::Candidate, "rationale_b"),
            (QuestionCheckCall::Candidate, "depth"),
        ] {
            let mut reply = recorded(call).await;
            reply.answers.remove(missing);

            assert_eq!(
                question_check_answers(&candidate(), call, &reply),
                Err(AiError::InvalidReply),
                "{missing} missing"
            );
        }
    }

    #[tokio::test]
    async fn an_answer_of_another_kind_in_any_call_is_an_invalid_reply() {
        for (call, name) in [
            (QuestionCheckCall::OptionRatings, "option_b"),
            (QuestionCheckCall::Hint, "hint_hides_answer_key"),
            (QuestionCheckCall::Candidate, "answerable"),
            (QuestionCheckCall::AnswerProbe, "answer_probe"),
        ] {
            let mut reply = recorded(call).await;
            reply.answers.insert(
                name.to_owned(),
                JevAnswer::Score {
                    probabilities: vec![0.5, 0.5],
                    expected: 0.5,
                },
            );

            assert_eq!(
                question_check_answers(&candidate(), call, &reply),
                Err(AiError::InvalidReply),
                "{name}"
            );
        }
    }

    #[tokio::test]
    async fn an_answer_to_a_question_the_call_did_not_ask_is_an_invalid_reply() {
        for (call, extra) in [
            (QuestionCheckCall::OptionRatings, "option_d"),
            (QuestionCheckCall::OptionRatings, "answerable"),
            (QuestionCheckCall::OptionRatings, "no_giveaway"),
            (QuestionCheckCall::Hint, "option_a"),
            (QuestionCheckCall::Candidate, "option_a"),
            (QuestionCheckCall::Candidate, "hint_hides_answer_key"),
            (QuestionCheckCall::Candidate, "needs_change"),
            (QuestionCheckCall::AnswerProbe, "option_a"),
        ] {
            let mut reply = recorded(call).await;
            reply
                .answers
                .insert(extra.to_owned(), JevAnswer::YesNo { yes: 0.5 });

            assert_eq!(
                question_check_answers(&candidate(), call, &reply),
                Err(AiError::InvalidReply),
                "{extra} in the {call:?} call"
            );
        }
    }

    /// The recorded reply to the answer probe call, but with `answer` to the
    /// probe: a probability for each of its options, by label.
    async fn probed(answer: &[(&str, f64)]) -> JevReply {
        let mut reply = recorded(QuestionCheckCall::AnswerProbe).await;
        reply.answers.insert(
            "answer_probe".to_owned(),
            JevAnswer::Choice {
                probabilities: answer
                    .iter()
                    .map(|&(label, answer)| (label.to_owned(), answer))
                    .collect(),
            },
        );
        reply
    }

    #[tokio::test]
    async fn the_answer_probe_keeps_jev_s_probability_for_each_option_and_bands_the_answer_key_s() {
        let reply = probed(&[("A", 0.05), ("B", 0.9), ("C", 0.05)]).await;

        let answers = question_check_answers(&candidate(), QuestionCheckCall::AnswerProbe, &reply)
            .expect("a probability for each option");

        assert_eq!(
            answers.probe(),
            Some(AnswerProbe::new([0.05, 0.9, 0.05].map(probability)))
        );
        assert_eq!(answers.iter().count(), 0, "no yes/no check");
        assert_eq!(
            answers.answer(CandidateCheck::AnswerProbe, candidate().answer_key()),
            Some(probability(0.05)),
            "the sample's answer key is A"
        );
        assert_eq!(answers.get(CandidateCheck::AnswerProbe), None);
    }

    #[tokio::test]
    async fn an_answer_probe_not_over_exactly_the_options_or_not_of_probabilities_is_an_invalid_reply()
     {
        for answer in [
            &[("A", 0.5), ("B", 0.5)][..],
            &[("A", 0.4), ("B", 0.3), ("D", 0.3)],
            &[("A", 0.4), ("B", 0.3), ("C", 0.2), ("D", 0.1)],
            &[("a", 0.4), ("b", 0.3), ("c", 0.3)],
            &[("A", 1.2), ("B", -0.1), ("C", -0.1)],
        ] {
            assert_eq!(
                question_check_answers(
                    &candidate(),
                    QuestionCheckCall::AnswerProbe,
                    &probed(answer).await
                ),
                Err(AiError::InvalidReply),
                "{answer:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_answer_that_is_not_a_probability_in_any_call_is_an_invalid_reply() {
        for (call, name) in [
            (QuestionCheckCall::OptionRatings, "option_b"),
            (QuestionCheckCall::Hint, "hint_hides_answer_key"),
            (QuestionCheckCall::Candidate, "why_it_matters"),
        ] {
            let mut reply = recorded(call).await;
            reply
                .answers
                .insert(name.to_owned(), JevAnswer::YesNo { yes: 1.2 });

            assert_eq!(
                question_check_answers(&candidate(), call, &reply),
                Err(AiError::InvalidReply),
                "{name}"
            );
        }
    }
}
