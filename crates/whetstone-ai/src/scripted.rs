//! A scripted AI for tests: each request gets the reply a test chose for it,
//! at once, and every request is logged exactly as it was asked.
//!
//! Jev's relevance gate is scripted per area, or to fail. The generator's
//! writing is scripted per area, and Jev's question check and reasoning
//! evaluation per question, by its stem. A check is scripted as exact
//! answers, or as answers that pass the show rule for whichever candidate
//! is asked, with any checks a test answers otherwise, or at a depth a test
//! chooses. Each reply scripted for one of them answers one request, in the
//! order scripted, so a test scripts a Retry's reply as a second one.
//!
//! The gate lets through, clear yes, any area nothing was scripted for, so
//! a test about writing or checking needn't script it. Any other request
//! nothing was scripted for is a mistake in the test, which panics as the
//! request is made.

use std::collections::{HashMap, VecDeque};
use std::future;
use std::sync::{Mutex, MutexGuard};

use whetstone_core::{
    Area, AreaId, Candidate, CandidateCheck, Criterion, Depth, Evidence, ModelId, Probability,
    Provenance, QuestionCheckAnswers, Role, WrittenCandidate,
};

use crate::gate::gate_request;
use crate::jev::JevRequest;
use crate::port::{Ai, CandidateCount, Pending, Reply, RequestFailure, Saved};
use crate::reasoning::QuestionWithoutKey;

/// An [`Ai`] whose replies a test chooses, and which logs what it was asked.
#[derive(Debug, Default)]
pub struct ScriptedAi {
    script: Mutex<Script>,
}

/// What the scripted AI will reply, and what it was asked so far.
#[derive(Debug, Default)]
struct Script {
    /// Jev's next answers in the relevance gate for each area.
    gate: HashMap<AreaId, VecDeque<Probability>>,
    /// The next relevance gates that fail, and how.
    gate_failures: VecDeque<RequestFailure>,
    /// The generator's next replies for each area.
    writes: HashMap<AreaId, VecDeque<Result<Vec<WrittenCandidate>, RequestFailure>>>,
    /// Jev's next replies for each candidate, by its stem.
    checks: HashMap<String, VecDeque<CheckReply>>,
    /// Jev's next replies to the reasoning written for each question, by
    /// its stem.
    assessments: HashMap<String, VecDeque<Result<Vec<Probability>, RequestFailure>>>,
    asked: Vec<Asked>,
}

/// One scripted reply to a question check.
#[derive(Debug)]
enum CheckReply {
    /// These answers, or this failure, whatever the candidate.
    Exactly(Result<QuestionCheckAnswers, RequestFailure>),
    /// Answers that pass the show rule for the candidate asked, at this
    /// depth, but for `except`, which answer their checks instead.
    Passing {
        except: Vec<(CandidateCheck, Probability)>,
        depth: Depth,
    },
}

/// One request the scripted AI was asked, exactly as asked.
#[derive(Debug, Clone, PartialEq)]
pub enum Asked {
    /// Jev was asked the relevance gate.
    CheckAreas {
        /// The evidence it was given.
        evidence: Evidence,
        /// The chosen areas it was asked about, in order.
        areas: Vec<Area>,
        /// Exactly what Jev is sent for them: the request the real AI
        /// builds, which holds each area's name and description, never its
        /// area prompt.
        sent: JevRequest,
    },
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
    /// Jev was asked to assess a response's reasoning. It holds all the
    /// request held, which has no place for the chosen option or the
    /// answer key.
    AssessReasoning {
        /// The session store's acknowledgement it was asked with, as its
        /// `Debug` output shows it.
        saved: String,
        /// The evidence it was given.
        evidence: Evidence,
        /// The question's stem.
        stem: String,
        /// Options A, B and C's text, in that order.
        options: [String; 3],
        /// The rubric's criteria, in order.
        rubric: Vec<Criterion>,
        /// The learner's reasoning, as the acknowledgement gives it.
        reasoning: String,
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

    /// Scripts Jev's answer for the area `area` in the next relevance gate
    /// that doesn't fail: the probability that the change is worth asking
    /// about in it. An area with no answer scripted gets 0.9, clear yes.
    pub fn on_gate(&self, area: &AreaId, answer: Probability) {
        self.script()
            .gate
            .entry(area.clone())
            .or_default()
            .push_back(answer);
    }

    /// Scripts the next relevance gate to fail with `failure`, before any
    /// answer scripted with [`on_gate`](Self::on_gate) is used.
    pub fn on_gate_failing(&self, failure: RequestFailure) {
        self.script().gate_failures.push_back(failure);
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
    /// options that aren't its answer key clear no, at 0.1. Its depth is
    /// needs reading the change, at 0.8, so its expected value is 1.
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
            CheckReply::Passing {
                except: answers.into_iter().collect(),
                depth: default_depth(),
            },
        );
    }

    /// The same as [`on_check_passing`](Self::on_check_passing), but Jev
    /// scores the candidate's depth as `depth`.
    pub fn on_check_passing_at(&self, stem: &str, depth: Depth) {
        self.script_check(
            stem,
            CheckReply::Passing {
                except: Vec::new(),
                depth,
            },
        );
    }

    /// Scripts Jev's reply to the next request to assess the reasoning
    /// written for the question whose stem is `stem`: a probability for
    /// each criterion, in rubric order, or a failure.
    pub fn on_assess(&self, stem: &str, reply: Result<Vec<Probability>, RequestFailure>) {
        self.script()
            .assessments
            .entry(stem.to_owned())
            .or_default()
            .push_back(reply);
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
    fn check_areas(
        &self,
        evidence: &Evidence,
        areas: &[Area],
    ) -> Pending<Vec<(AreaId, Probability)>> {
        let mut script = self.script();
        script.asked.push(Asked::CheckAreas {
            evidence: evidence.clone(),
            areas: areas.to_vec(),
            sent: gate_request(&ModelId::new(JEV_MODEL.to_owned()), evidence, areas),
        });
        let reply = match script.gate_failures.pop_front() {
            Some(failure) => Err(failure),
            None => Ok(areas
                .iter()
                .map(|area| {
                    let answer = script
                        .gate
                        .get_mut(area.id())
                        .and_then(VecDeque::pop_front)
                        .unwrap_or_else(worth_asking);
                    (area.id().clone(), answer)
                })
                .collect()),
        };
        reply_at_once(reply, Role::Jev, JEV_MODEL)
    }

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

    #[expect(
        clippy::panic,
        reason = "a request a test didn't script is a mistake in the test, which fails at once"
    )]
    fn assess_reasoning(
        &self,
        saved: &dyn Saved,
        evidence: &Evidence,
        question: QuestionWithoutKey<'_>,
    ) -> Pending<Vec<Probability>> {
        let mut script = self.script();
        script.asked.push(Asked::AssessReasoning {
            saved: format!("{saved:?}"),
            evidence: evidence.clone(),
            stem: question.stem().to_owned(),
            options: question.options().map(str::to_owned),
            rubric: question.rubric().to_vec(),
            reasoning: saved.reasoning().to_owned(),
        });
        let reply = script
            .assessments
            .get_mut(question.stem())
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| panic!("no assessment scripted for {:?}", question.stem()));
        reply_at_once(reply, Role::Jev, JEV_MODEL)
    }
}

impl CheckReply {
    /// The reply to the question check of `candidate`.
    fn into_reply(self, candidate: &Candidate) -> Result<QuestionCheckAnswers, RequestFailure> {
        match self {
            Self::Exactly(reply) => reply,
            Self::Passing { except, depth } => {
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
                Ok(QuestionCheckAnswers::new(passing.chain(except), depth))
            }
        }
    }
}

/// The gate's answer for an area a test scripted none for: 0.9, clear yes.
fn worth_asking() -> Probability {
    Probability::new(0.9).expect("0.9 is a probability")
}

/// The depth a check scripted to pass scores unless a test scripts one:
/// needs reading the change, at 0.8, so its expected value is 1.
fn default_depth() -> Depth {
    let probability = |value| Probability::new(value).expect("0.1 and 0.8 are probabilities");
    Depth::new([probability(0.1), probability(0.8), probability(0.1)], 1.0)
        .expect("1 is an expected depth")
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
        Area, AssessmentPolicy, CandidateCheck, Change, ConnectedCodeLimit, Connections, Depth,
        Evidence, ExcludePatterns, OptionLetter, Probability, Question, QuestionCount,
        QuestionType, ReviewChoices, Role, TouchedFile, WrittenCandidate, WrittenCitation,
        WrittenCriterion, WrittenOption, plain_checks, show_rule,
    };

    use super::*;
    use crate::port::{Ai, CandidateCount, RequestFailure};
    use crate::samples::Acknowledged;

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

    fn probability(value: f64) -> Probability {
        Probability::new(value).expect("from 0 to 1")
    }

    /// A depth whose levels are at `one_line`, `the_change` and
    /// `code_around`, and whose expected value is `expected`.
    fn depth(one_line: f64, the_change: f64, code_around: f64, expected: f64) -> Depth {
        Depth::new(
            [one_line, the_change, code_around].map(probability),
            expected,
        )
        .expect("from 0 to 2")
    }

    fn answers(a: f64, b: f64, c: f64) -> QuestionCheckAnswers {
        QuestionCheckAnswers::new(
            OptionLetter::ALL
                .into_iter()
                .zip([a, b, c])
                .map(|(letter, value)| (CandidateCheck::Option(letter), probability(value))),
            depth(0.2, 0.3, 0.5, 1.3),
        )
    }

    /// [`candidate`] asking `stem`, once its question check passes.
    fn question(stem: &str) -> Question {
        let candidate = candidate(stem);
        let answers = CheckReply::Passing {
            except: Vec::new(),
            depth: default_depth(),
        }
        .into_reply(&candidate)
        .expect("scripted to pass");
        show_rule(candidate, answers, AssessmentPolicy::current()).expect("every check passes")
    }

    fn probabilities(values: &[f64]) -> Vec<Probability> {
        values
            .iter()
            .map(|&value| Probability::new(value).expect("from 0 to 1"))
            .collect()
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

    /// Every piece of text in `request`: its state and each question's
    /// instructions and meanings.
    fn text_of(request: &JevRequest) -> String {
        std::iter::once(request.state.to_string())
            .chain(
                request
                    .questions
                    .values()
                    .map(|question| format!("{question:?}")),
            )
            .collect()
    }

    #[test]
    fn jev_answers_the_gate_as_scripted_for_each_area_and_lets_every_other_area_through() {
        let ai = ScriptedAi::new();
        ai.on_gate(area(1).id(), probability(0.1));
        ai.on_gate(area(2).id(), probability(0.5));
        let areas = [area(0), area(1), area(2)];

        let gate = at_once(ai.check_areas(&evidence(), &areas)).expect("scripted to answer");

        assert_eq!(
            gate.content,
            [
                (area(0).id().clone(), probability(0.9)),
                (area(1).id().clone(), probability(0.1)),
                (area(2).id().clone(), probability(0.5)),
            ],
            "the answers scripted, and clear yes for an area with none"
        );
        assert_eq!(gate.provenance.role(), Role::Jev);
        let asked = ai.asked();
        let [
            Asked::CheckAreas {
                evidence: given,
                areas: asked_about,
                sent,
            },
        ] = asked.as_slice()
        else {
            panic!("one gate: {asked:#?}");
        };
        assert_eq!(given, &evidence());
        assert_eq!(asked_about, &areas);
        assert_eq!(
            sent.questions
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            areas
                .iter()
                .map(|area| area.id().as_str())
                .collect::<Vec<_>>(),
            "Jev is sent one question per area, named by its ID"
        );
        let sent = text_of(sent);
        for area in &areas {
            assert!(sent.contains(area.description()), "{}", area.id());
            assert!(!sent.contains(area.prompt().trim()), "{}", area.id());
        }
    }

    #[test]
    fn a_gate_scripted_to_fail_fails_once_and_the_next_gate_is_answered() {
        let ai = ScriptedAi::new();
        ai.on_gate_failing(RequestFailure::TimedOut);
        ai.on_gate(area(0).id(), probability(0.2));
        let areas = [area(0), area(1)];

        let failed = at_once(ai.check_areas(&evidence(), &areas));
        let again = at_once(ai.check_areas(&evidence(), &areas));

        assert_eq!(failed, Err(RequestFailure::TimedOut));
        assert_eq!(
            again.map(|reply| reply.content),
            Ok(vec![
                (area(0).id().clone(), probability(0.2)),
                (area(1).id().clone(), probability(0.9)),
            ])
        );
        assert_eq!(ai.asked().len(), 2, "each gate is logged");
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
        let unclear = probability(0.5);
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
                    .map(|&(check, value)| (check, probability(value))),
                depth(0.1, 0.8, 0.1, 1.0),
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
            "every check clear yes, but the options that aren't the answer key clear no, \
             and needs reading the change"
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
    fn a_check_scripted_to_pass_at_a_depth_scores_that_depth() {
        let ai = ScriptedAi::new();
        let deepest = depth(0.05, 0.15, 0.8, 1.75);
        ai.on_check_passing_at("Why retry?", deepest);
        ai.on_check_passing("Why retry?");
        let candidate = candidate("Why retry?");

        let deep = at_once(ai.check_question(&evidence(), &candidate)).expect("scripted to pass");
        let usual = at_once(ai.check_question(&evidence(), &candidate)).expect("scripted to pass");

        assert_eq!(deep.content.depth(), deepest, "the depth scripted");
        assert_eq!(
            deep.content.iter().collect::<Vec<_>>(),
            usual.content.iter().collect::<Vec<_>>(),
            "every other answer passes, as without a depth"
        );
    }

    #[test]
    fn jev_assesses_the_reasoning_scripted_for_each_question_by_its_stem_and_logs_what_it_was_given()
     {
        let ai = ScriptedAi::new();
        ai.on_assess("Why retry?", Ok(probabilities(&[0.9])));
        ai.on_assess("Why retry?", Err(RequestFailure::Cancelled));
        let asked = question("Why retry?");

        let first = at_once(ai.assess_reasoning(
            &Acknowledged("It retries."),
            &evidence(),
            QuestionWithoutKey::from(&asked),
        ));
        let again = at_once(ai.assess_reasoning(
            &Acknowledged("It retries once."),
            &evidence(),
            QuestionWithoutKey::from(&asked),
        ));

        let first = first.expect("scripted to answer");
        assert_eq!(first.content, probabilities(&[0.9]), "as scripted");
        assert_eq!(first.provenance.role(), Role::Jev);
        assert_eq!(again, Err(RequestFailure::Cancelled));
        let logged = |reasoning: &str| Asked::AssessReasoning {
            saved: format!("Acknowledged({reasoning:?})"),
            evidence: evidence(),
            stem: "Why retry?".to_owned(),
            options: ["An option"; 3].map(str::to_owned),
            rubric: asked.rubric().to_vec(),
            reasoning: reasoning.to_owned(),
        };
        assert_eq!(
            ai.asked(),
            [logged("It retries."), logged("It retries once.")]
        );
    }

    #[test]
    #[should_panic(expected = "no write scripted")]
    fn a_request_nothing_was_scripted_for_fails_the_test_as_it_is_made() {
        drop(ScriptedAi::new().write_candidates(&evidence(), &area(0), count()));
    }
}
