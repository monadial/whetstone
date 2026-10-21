//! A scripted AI for tests: each request gets the reply a test chose for it,
//! at once, and every request is logged exactly as it was asked.
//!
//! The generator's writing is scripted per area, and Jev's question check
//! per candidate, by its stem. A check is scripted as exact answers, or as
//! answers that pass the show rule for whichever candidate is asked, with
//! any checks a test answers otherwise. Each reply scripted for one of them
//! answers one request, in the order scripted, so a test scripts a Retry's
//! reply as a second one. A request nothing was scripted for is a mistake
//! in the test, which panics as the request is made.

use std::collections::{HashMap, VecDeque};
use std::future;
use std::sync::{Mutex, MutexGuard};

use whetstone_core::{
    Area, AreaId, Candidate, CandidateCheck, Evidence, ModelId, Probability, Provenance,
    QuestionCheckAnswers, Role, WrittenCandidate,
};

use crate::port::{Ai, CandidateCount, Pending, Reply, RequestFailure};

/// An [`Ai`] whose replies a test chooses, and which logs what it was asked.
#[derive(Debug, Default)]
pub struct ScriptedAi {
    script: Mutex<Script>,
}

/// What the scripted AI will reply, and what it was asked so far.
#[derive(Debug, Default)]
struct Script {
    /// The generator's next replies for each area.
    writes: HashMap<AreaId, VecDeque<Result<Vec<WrittenCandidate>, RequestFailure>>>,
    /// Jev's next replies for each candidate, by its stem.
    checks: HashMap<String, VecDeque<CheckReply>>,
    asked: Vec<Asked>,
}

/// One scripted reply to a question check.
#[derive(Debug)]
enum CheckReply {
    /// These answers, or this failure, whatever the candidate.
    Exactly(Result<QuestionCheckAnswers, RequestFailure>),
    /// Answers that pass the show rule for the candidate asked, but for
    /// these, which answer their checks instead.
    PassingExcept(Vec<(CandidateCheck, Probability)>),
}

/// One request the scripted AI was asked, exactly as asked.
#[derive(Debug, Clone, PartialEq)]
pub enum Asked {
    /// The generator was asked to write candidates.
    WriteCandidates {
        /// The evidence it was given.
        evidence: Evidence,
        /// The area to write about.
        area: Area,
        /// How many to write.
        count: CandidateCount,
    },
    /// Jev was asked to check a candidate.
    CheckQuestion {
        /// The evidence it was given.
        evidence: Evidence,
        /// The candidate to check.
        candidate: Candidate,
    },
}

/// The model the scripted generator says it is.
const GENERATOR_MODEL: &str = "scripted/generator";
/// The model the scripted Jev says it is.
const JEV_MODEL: &str = "scripted/jev";

impl ScriptedAi {
    /// A scripted AI with no replies scripted yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Scripts the generator's reply to the next request to write about the
    /// area `area`: these candidates, or a failure.
    pub fn on_write(&self, area: &AreaId, reply: Result<Vec<WrittenCandidate>, RequestFailure>) {
        self.script()
            .writes
            .entry(area.clone())
            .or_default()
            .push_back(reply);
    }

    /// Scripts Jev's reply to the next request to check the candidate whose
    /// stem is `stem`: exactly these answers, or a failure.
    pub fn on_check(&self, stem: &str, reply: Result<QuestionCheckAnswers, RequestFailure>) {
        self.script_check(stem, CheckReply::Exactly(reply));
    }

    /// Scripts Jev's reply to the next request to check the candidate whose
    /// stem is `stem` as answers the show rule passes, made for the
    /// candidate asked: every one of its checks clear yes, at 0.9, but the
    /// options that aren't its answer key clear no, at 0.1.
    pub fn on_check_passing(&self, stem: &str) {
        self.on_check_passing_except(stem, []);
    }

    /// The same as [`on_check_passing`](Self::on_check_passing), but Jev
    /// gives `answers` to their checks instead.
    pub fn on_check_passing_except(
        &self,
        stem: &str,
        answers: impl IntoIterator<Item = (CandidateCheck, Probability)>,
    ) {
        self.script_check(
            stem,
            CheckReply::PassingExcept(answers.into_iter().collect()),
        );
    }

    fn script_check(&self, stem: &str, reply: CheckReply) {
        self.script()
            .checks
            .entry(stem.to_owned())
            .or_default()
            .push_back(reply);
    }

    /// Every request it was asked so far, in order.
    #[must_use]
    pub fn asked(&self) -> Vec<Asked> {
        self.script().asked.clone()
    }

    fn script(&self) -> MutexGuard<'_, Script> {
        // A test that panicked while holding it has failed already.
        self.script
            .lock()
            .expect("the script is only locked between a test's own steps")
    }
}

impl Ai for ScriptedAi {
    #[expect(
        clippy::panic,
        reason = "a request a test didn't script is a mistake in the test, which fails at once"
    )]
    fn write_candidates(
        &self,
        evidence: &Evidence,
        area: &Area,
        count: CandidateCount,
    ) -> Pending<Vec<WrittenCandidate>> {
        let mut script = self.script();
        script.asked.push(Asked::WriteCandidates {
            evidence: evidence.clone(),
            area: area.clone(),
            count,
        });
        let reply = script
            .writes
            .get_mut(area.id())
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| panic!("no write scripted for the area {}", area.id()));
        reply_at_once(reply, Role::Generator, GENERATOR_MODEL)
    }

    #[expect(
        clippy::panic,
        reason = "a request a test didn't script is a mistake in the test, which fails at once"
    )]
    fn check_question(
        &self,
        evidence: &Evidence,
        candidate: &Candidate,
    ) -> Pending<QuestionCheckAnswers> {
        let mut script = self.script();
        script.asked.push(Asked::CheckQuestion {
            evidence: evidence.clone(),
            candidate: candidate.clone(),
        });
        let reply = script
            .checks
            .get_mut(candidate.stem())
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| panic!("no check scripted for {:?}", candidate.stem()));
        reply_at_once(reply.into_reply(candidate), Role::Jev, JEV_MODEL)
    }
}

impl CheckReply {
    /// The reply to the question check of `candidate`.
    fn into_reply(self, candidate: &Candidate) -> Result<QuestionCheckAnswers, RequestFailure> {
        match self {
            Self::Exactly(reply) => reply,
            Self::PassingExcept(answers) => {
                let passing = candidate.checks().into_iter().map(|check| {
                    let answer = match check {
                        CandidateCheck::Option(letter) if letter != candidate.answer_key() => 0.1,
                        _ => 0.9,
                    };
                    (
                        check,
                        Probability::new(answer).expect("0.1 and 0.9 are probabilities"),
                    )
                });
                // A check answered twice keeps its last answer.
                Ok(QuestionCheckAnswers::new(passing.chain(answers)))
            }
        }
    }
}

/// `reply` as a request that has it at once, from `model` in `role`.
fn reply_at_once<T: Send + 'static>(
    reply: Result<T, RequestFailure>,
    role: Role,
    model: &str,
) -> Pending<T> {
    let model = ModelId::new(model.to_owned());
    let reply = reply.map(|content| Reply {
        content,
        provenance: Provenance::new(role, model.clone()).with_reported_model(model),
    });
    Box::pin(future::ready(reply))
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    use whetstone_core::{
        Area, CandidateCheck, Change, ConnectedCodeLimit, Connections, Evidence, ExcludePatterns,
        OptionLetter, Probability, QuestionCount, QuestionType, ReviewChoices, Role, TouchedFile,
        WrittenCandidate, WrittenCitation, WrittenCriterion, WrittenOption, plain_checks,
    };

    use super::*;
    use crate::port::{Ai, CandidateCount, RequestFailure};

    /// `request`'s outcome, which the scripted AI has at once.
    fn at_once<T>(request: impl Future<Output = T>) -> T {
        match pin!(request).poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(outcome) => outcome,
            Poll::Pending => panic!("the scripted AI replies at once"),
        }
    }

    fn evidence() -> Evidence {
        Evidence::build(
            &Change::new(
                "Retry failed payments".to_owned(),
                Vec::new(),
                "diff --git a/src/pay.rs b/src/pay.rs\n+retry\n".to_owned(),
                vec![TouchedFile::modified(
                    "src/pay.rs".to_owned(),
                    "pay\n".to_owned(),
                    "pay\nretry\n".to_owned(),
                )],
            ),
            &ExcludePatterns::default(),
            &Area::defaults(),
            &Connections::NoneFound,
            ConnectedCodeLimit::effective(None, None),
            &ReviewChoices::default(),
        )
    }

    fn area(index: usize) -> Area {
        Area::defaults().swap_remove(index)
    }

    fn written(stem: &str) -> WrittenCandidate {
        WrittenCandidate {
            question_type: QuestionType::SingleChoice,
            stem: stem.to_owned(),
            options: Vec::new(),
            answer_key: "A".to_owned(),
            why_it_matters: String::new(),
            rubric: Vec::new(),
            hint: String::new(),
            citations: Vec::new(),
        }
    }

    /// A well-formed candidate about the first area, asking `stem`, whose
    /// answer key is A and whose rubric has one criterion.
    fn candidate(stem: &str) -> Candidate {
        candidate_with(stem, "A", 1)
    }

    /// A well-formed candidate about the first area, asking `stem`, whose
    /// answer key is `answer_key` and whose rubric has `criteria` criteria.
    fn candidate_with(stem: &str, answer_key: &str, criteria: usize) -> Candidate {
        let text = str::to_owned;
        let written = WrittenCandidate {
            options: ["A", "B", "C"]
                .map(|letter| WrittenOption {
                    letter: text(letter),
                    text: text("An option"),
                    rationale: text("Why"),
                })
                .into(),
            answer_key: text(answer_key),
            why_it_matters: text("It matters"),
            rubric: vec![
                WrittenCriterion {
                    statement: text("Names the retry"),
                    met: text("It does"),
                    not_met: text("It doesn't"),
                };
                criteria
            ],
            hint: text("Look at pay.rs"),
            citations: vec![WrittenCitation {
                path: text("src/pay.rs"),
                first_line: 1,
                last_line: 2,
            }],
            ..written(stem)
        };
        plain_checks(area(0).id(), vec![written], &evidence())
            .into_passed()
            .pop()
            .expect("it passes the plain checks")
    }

    fn answers(a: f64, b: f64, c: f64) -> QuestionCheckAnswers {
        QuestionCheckAnswers::new(OptionLetter::ALL.into_iter().zip([a, b, c]).map(
            |(letter, value)| {
                (
                    CandidateCheck::Option(letter),
                    Probability::new(value).expect("from 0 to 1"),
                )
            },
        ))
    }

    fn count() -> CandidateCount {
        CandidateCount::per_area(QuestionCount::DEFAULT, 2)
    }

    #[test]
    fn the_generator_writes_what_is_scripted_for_each_area_at_once_and_each_request_is_logged() {
        let ai = ScriptedAi::new();
        ai.on_write(
            area(0).id(),
            Ok(vec![written("First?"), written("Second?")]),
        );
        ai.on_write(area(1).id(), Err(RequestFailure::RateLimited));

        let first = at_once(ai.write_candidates(&evidence(), &area(0), count()));
        let second = at_once(ai.write_candidates(&evidence(), &area(1), count()));

        let first = first.expect("scripted to write");
        assert_eq!(
            first.content,
            [written("First?"), written("Second?")],
            "the candidates, as scripted"
        );
        assert_eq!(first.provenance.role(), Role::Generator);
        assert_eq!(second, Err(RequestFailure::RateLimited));
        assert_eq!(
            ai.asked(),
            [
                Asked::WriteCandidates {
                    evidence: evidence(),
                    area: area(0),
                    count: count(),
                },
                Asked::WriteCandidates {
                    evidence: evidence(),
                    area: area(1),
                    count: count(),
                },
            ]
        );
    }

    #[test]
    fn jev_answers_what_is_scripted_for_each_candidate_by_its_stem_in_the_order_scripted() {
        let ai = ScriptedAi::new();
        ai.on_check("Why retry?", Ok(answers(0.9, 0.1, 0.05)));
        ai.on_check("Why retry?", Err(RequestFailure::TimedOut));
        ai.on_check("Why log?", Ok(answers(0.5, 0.5, 0.5)));

        let first = at_once(ai.check_question(&evidence(), &candidate("Why retry?")));
        let other = at_once(ai.check_question(&evidence(), &candidate("Why log?")));
        let again = at_once(ai.check_question(&evidence(), &candidate("Why retry?")));

        let first = first.expect("scripted to answer");
        assert_eq!(first.content, answers(0.9, 0.1, 0.05), "as scripted");
        assert_eq!(first.provenance.role(), Role::Jev);
        assert_eq!(other.map(|reply| reply.content), Ok(answers(0.5, 0.5, 0.5)));
        assert_eq!(again, Err(RequestFailure::TimedOut));
        assert_eq!(
            ai.asked(),
            ["Why retry?", "Why log?", "Why retry?"].map(|stem| Asked::CheckQuestion {
                evidence: evidence(),
                candidate: candidate(stem),
            })
        );
    }

    #[test]
    fn a_check_scripted_to_pass_answers_every_check_of_the_candidate_asked_but_those_given() {
        use CandidateCheck::*;
        let ai = ScriptedAi::new();
        let unclear = Probability::new(0.5).expect("from 0 to 1");
        ai.on_check_passing("Why retry?");
        ai.on_check_passing_except(
            "Why retry?",
            [(NeedsChange, unclear), (Option(OptionLetter::C), unclear)],
        );
        let candidate = candidate_with("Why retry?", "B", 2);

        let passing = at_once(ai.check_question(&evidence(), &candidate));
        let except = at_once(ai.check_question(&evidence(), &candidate));

        let answers = |answers: &[(CandidateCheck, f64)]| {
            QuestionCheckAnswers::new(
                answers
                    .iter()
                    .map(|&(check, value)| (check, Probability::new(value).expect("from 0 to 1"))),
            )
        };
        assert_eq!(
            passing.map(|reply| reply.content),
            Ok(answers(&[
                (Answerable, 0.9),
                (AboutArea, 0.9),
                (NeedsChange, 0.9),
                (NoGiveaway, 0.9),
                (Criterion(0), 0.9),
                (Criterion(1), 0.9),
                (Option(OptionLetter::A), 0.1),
                (Option(OptionLetter::B), 0.9),
                (Option(OptionLetter::C), 0.1),
                (Rationale(OptionLetter::A), 0.9),
                (Rationale(OptionLetter::B), 0.9),
                (Rationale(OptionLetter::C), 0.9),
                (WhyItMatters, 0.9),
                (HintPointsToEvidence, 0.9),
                (HintHidesAnswerKey, 0.9),
            ])),
            "every check clear yes, but the options that aren't the answer key clear no"
        );
        assert_eq!(
            except.map(|reply| reply.content),
            Ok(answers(&[
                (Answerable, 0.9),
                (AboutArea, 0.9),
                (NeedsChange, 0.5),
                (NoGiveaway, 0.9),
                (Criterion(0), 0.9),
                (Criterion(1), 0.9),
                (Option(OptionLetter::A), 0.1),
                (Option(OptionLetter::B), 0.9),
                (Option(OptionLetter::C), 0.5),
                (Rationale(OptionLetter::A), 0.9),
                (Rationale(OptionLetter::B), 0.9),
                (Rationale(OptionLetter::C), 0.9),
                (WhyItMatters, 0.9),
                (HintPointsToEvidence, 0.9),
                (HintHidesAnswerKey, 0.9),
            ])),
            "the same, but the answers given"
        );
    }

    #[test]
    #[should_panic(expected = "no write scripted")]
    fn a_request_nothing_was_scripted_for_fails_the_test_as_it_is_made() {
        drop(ScriptedAi::new().write_candidates(&evidence(), &area(0), count()));
    }
}
