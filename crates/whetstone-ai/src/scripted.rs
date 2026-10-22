//! A scripted AI for tests: each request gets the reply a test chose for it,
//! at once, and every request is logged exactly as it was asked.
//!
//! Jev's relevance gate is scripted per area, or to fail. The generator's
//! writing is scripted per area, and Jev's question check and reasoning
//! evaluation, the generator's explanation and Jev's check of it per
//! question, by its stem. A check is scripted as exact answers, or as
//! answers that pass the show rule for whichever candidate is asked, with
//! any checks a test answers otherwise, or at a depth a test chooses. Each
//! reply scripted for one of them answers one request, in the order
//! scripted, so a test scripts a Retry's reply as a second one. A question
//! check's reply answers its three calls: each call takes the answers to
//! the checks it asks, and a failure fails all three. A test can also script
//! one call of a check to fail on its own. A reply reports no cost unless a
//! test scripts one for its role.
//!
//! The gate lets through, clear yes, any area nothing was scripted for, so
//! a test about writing or checking needn't script it. Any other request
//! nothing was scripted for is a mistake in the test, which panics as the
//! request is made.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::future;
use std::sync::{Mutex, MutexGuard};

use whetstone_core::{
    Area, AreaId, Candidate, CandidateCheck, Cost, Criterion, CriterionOutcome, Depth, DepthLevel,
    Evidence, GenerationId, JevExchange, ModelId, Probability, Provenance, Question,
    QuestionCheckAnswers, Role, WrittenCandidate,
};

use crate::exchanges::jev_exchanges;
use crate::explanation::{
    ParagraphCheck, WrittenParagraph, explanation_check_request, paragraph_check_names,
};
use crate::gate::gate_request;
use crate::jev::{JevAnswer, JevReply, JevRequest};
use crate::port::{Ai, CandidateCount, Pending, Reply, RequestFailure, Saved, Stamp};
use crate::question_check::{
    self, CheckCallAnswers, DEPTH, QuestionCheckCall, question_check_request,
};
use crate::reasoning::{QuestionWithoutKey, reasoning_request};
use crate::usage::Usage;

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
    /// Jev's next replies for each candidate, by its stem, each of which
    /// answers the three calls of a check.
    checks: HashMap<String, VecDeque<CheckReply>>,
    /// Jev's next replies to each call of the check of each candidate, by
    /// its stem: a call's share of a check's reply, or a failure scripted
    /// for the call alone.
    calls: HashMap<(String, QuestionCheckCall), VecDeque<Result<CheckCallAnswers, RequestFailure>>>,
    /// Jev's next replies to the reasoning written for each question, by
    /// its stem.
    assessments: HashMap<String, VecDeque<Result<Vec<Probability>, RequestFailure>>>,
    /// The generator's next explanations of the reasoning written for each
    /// question, by its stem.
    explanations: HashMap<String, VecDeque<Result<Vec<WrittenParagraph>, RequestFailure>>>,
    /// Jev's next checks of the explanation of each question, by its stem.
    explanation_checks: HashMap<String, VecDeque<Result<Vec<ParagraphCheck>, RequestFailure>>>,
    /// The cost each reply from a role reports: none for a role not in it.
    costs: HashMap<Role, Cost>,
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
    /// Jev was asked one call of a candidate's question check.
    CheckQuestion {
        /// The call.
        call: QuestionCheckCall,
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
    /// The generator was asked to explain what a response's reasoning
    /// missed.
    Explain {
        /// The session store's acknowledgement it was asked with, as its
        /// `Debug` output shows it.
        saved: String,
        /// The evidence it was given.
        evidence: Evidence,
        /// The question's stem.
        stem: String,
        /// The learner's reasoning, as the acknowledgement gives it.
        reasoning: String,
        /// The criteria to explain: each one's place in the rubric and its
        /// outcome.
        criteria: Vec<(usize, CriterionOutcome)>,
    },
    /// Jev was asked to check an explanation's paragraphs.
    CheckExplanation {
        /// The session store's acknowledgement it was asked with, as its
        /// `Debug` output shows it.
        saved: String,
        /// The evidence it was given.
        evidence: Evidence,
        /// The question's stem.
        stem: String,
        /// The paragraphs to check, in order.
        paragraphs: Vec<WrittenParagraph>,
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

    /// Scripts Jev's reply to the next check of the candidate whose stem is
    /// `stem`: exactly these answers, each call taking the answers to the
    /// checks it asks, or a failure of all three calls.
    pub fn on_check(&self, stem: &str, reply: Result<QuestionCheckAnswers, RequestFailure>) {
        self.script_check(stem, CheckReply::Exactly(reply));
    }

    /// Scripts the next request of `call` of the check of the candidate
    /// whose stem is `stem` to fail with `failure`, before any reply
    /// scripted for the whole check answers it.
    pub fn on_check_call_failing(
        &self,
        stem: &str,
        call: QuestionCheckCall,
        failure: RequestFailure,
    ) {
        self.script()
            .calls
            .entry((stem.to_owned(), call))
            .or_default()
            .push_back(Err(failure));
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

    /// Scripts the generator's reply to the next request to explain what
    /// the reasoning written for the question whose stem is `stem` missed:
    /// these paragraphs, or a failure.
    pub fn on_explain(&self, stem: &str, reply: Result<Vec<WrittenParagraph>, RequestFailure>) {
        self.script()
            .explanations
            .entry(stem.to_owned())
            .or_default()
            .push_back(reply);
    }

    /// Scripts Jev's reply to the next request to check the explanation of
    /// the question whose stem is `stem`: an answer for each paragraph, in
    /// order, or a failure.
    pub fn on_check_explanation(
        &self,
        stem: &str,
        reply: Result<Vec<ParagraphCheck>, RequestFailure>,
    ) {
        self.script()
            .explanation_checks
            .entry(stem.to_owned())
            .or_default()
            .push_back(reply);
    }

    /// Scripts every later reply from `role` to report `cost`, or no cost
    /// when it is `None`. Until a test scripts one, replies report none.
    pub fn report_cost(&self, role: Role, cost: Option<Cost>) {
        let mut script = self.script();
        match cost {
            Some(cost) => script.costs.insert(role, cost),
            None => script.costs.remove(&role),
        };
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
        stamp: Stamp,
        evidence: &Evidence,
        areas: &[Area],
    ) -> Pending<Vec<(AreaId, Probability)>> {
        let mut script = self.script();
        let sent = gate_request(&jev_model(), evidence, areas);
        script.asked.push(Asked::CheckAreas {
            evidence: evidence.clone(),
            areas: areas.to_vec(),
            sent: sent.clone(),
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
        reply_at_once(stamp, reply, Role::Jev, &script, |answers| {
            kept(
                &sent,
                answers
                    .iter()
                    .map(|(area, answer)| (area.as_str().to_owned(), yes_no(*answer))),
            )
        })
    }

    #[expect(
        clippy::panic,
        reason = "a request a test didn't script is a mistake in the test, which fails at once"
    )]
    fn write_candidates(
        &self,
        stamp: Stamp,
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
        reply_at_once(stamp, reply, Role::Generator, &script, |_| Vec::new())
    }

    #[expect(
        clippy::panic,
        reason = "a request a test didn't script is a mistake in the test, which fails at once"
    )]
    fn check_question(
        &self,
        stamp: Stamp,
        call: QuestionCheckCall,
        evidence: &Evidence,
        candidate: &Candidate,
    ) -> Pending<CheckCallAnswers> {
        let mut script = self.script();
        script.asked.push(Asked::CheckQuestion {
            call,
            evidence: evidence.clone(),
            candidate: candidate.clone(),
        });
        let key = (candidate.stem().to_owned(), call);
        if script.calls.get(&key).is_none_or(VecDeque::is_empty) {
            // A check's reply answers its three calls: each takes its share.
            let reply = script
                .checks
                .get_mut(candidate.stem())
                .and_then(VecDeque::pop_front)
                .unwrap_or_else(|| panic!("no check scripted for {:?}", candidate.stem()))
                .into_reply(candidate);
            for each in QuestionCheckCall::ALL {
                let share = reply
                    .as_ref()
                    .map(|answers| CheckCallAnswers::of_call(each, answers))
                    .map_err(|failure| *failure);
                script
                    .calls
                    .entry((candidate.stem().to_owned(), each))
                    .or_default()
                    .push_back(share);
            }
        }
        let reply = script
            .calls
            .get_mut(&key)
            .and_then(VecDeque::pop_front)
            .expect("a call's share was just scripted, if none was before");
        let sent = question_check_request(&jev_model(), evidence, candidate, call);
        reply_at_once(stamp, reply, Role::Jev, &script, |answers| {
            let depth = answers.depth().map(|depth| {
                (
                    DEPTH.to_owned(),
                    JevAnswer::Score {
                        probabilities: DepthLevel::ALL
                            .map(|level| depth.probability(level).get())
                            .into(),
                        expected: depth.expected(),
                    },
                )
            });
            kept(
                &sent,
                answers
                    .iter()
                    .map(|(check, answer)| (question_check::name(check), yes_no(answer)))
                    .chain(depth),
            )
        })
    }

    #[expect(
        clippy::panic,
        reason = "a request a test didn't script is a mistake in the test, which fails at once"
    )]
    fn assess_reasoning(
        &self,
        stamp: Stamp,
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
        let sent = reasoning_request(&jev_model(), evidence, question, saved.reasoning());
        reply_at_once(stamp, reply, Role::Jev, &script, |answers| {
            kept(
                &sent,
                answers.iter().enumerate().map(|(index, answer)| {
                    (question_check::criterion_name(index), yes_no(*answer))
                }),
            )
        })
    }

    #[expect(
        clippy::panic,
        reason = "a request a test didn't script is a mistake in the test, which fails at once"
    )]
    fn explain(
        &self,
        stamp: Stamp,
        saved: &dyn Saved,
        evidence: &Evidence,
        question: &Question,
        criteria: &[(usize, CriterionOutcome)],
    ) -> Pending<Vec<WrittenParagraph>> {
        let mut script = self.script();
        script.asked.push(Asked::Explain {
            saved: format!("{saved:?}"),
            evidence: evidence.clone(),
            stem: question.stem().to_owned(),
            reasoning: saved.reasoning().to_owned(),
            criteria: criteria.to_vec(),
        });
        let reply = script
            .explanations
            .get_mut(question.stem())
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| panic!("no explanation scripted for {:?}", question.stem()));
        reply_at_once(stamp, reply, Role::Generator, &script, |_| Vec::new())
    }

    #[expect(
        clippy::panic,
        reason = "a request a test didn't script is a mistake in the test, which fails at once"
    )]
    fn check_explanation(
        &self,
        stamp: Stamp,
        saved: &dyn Saved,
        evidence: &Evidence,
        question: &Question,
        paragraphs: &[WrittenParagraph],
    ) -> Pending<Vec<ParagraphCheck>> {
        let mut script = self.script();
        script.asked.push(Asked::CheckExplanation {
            saved: format!("{saved:?}"),
            evidence: evidence.clone(),
            stem: question.stem().to_owned(),
            paragraphs: paragraphs.to_vec(),
        });
        let reply = script
            .explanation_checks
            .get_mut(question.stem())
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| panic!("no explanation check scripted for {:?}", question.stem()));
        let sent = explanation_check_request(&jev_model(), evidence, question, paragraphs);
        let paragraphs = paragraphs.to_vec();
        reply_at_once(stamp, reply, Role::Jev, &script, |checks| {
            kept(
                &sent,
                paragraphs
                    .iter()
                    .zip(checks)
                    .flat_map(|(paragraph, check)| {
                        let [supported, agrees] = paragraph_check_names(paragraph.criterion);
                        [
                            (supported, yes_no(check.supported)),
                            (agrees, yes_no(check.agrees)),
                        ]
                    }),
            )
        })
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

/// The model the scripted Jev is asked for, as the real AI would be.
fn jev_model() -> ModelId {
    ModelId::new(JEV_MODEL.to_owned())
}

/// Jev's scripted `answer` to a yes/no question, as Jev returns it.
fn yes_no(answer: Probability) -> JevAnswer {
    JevAnswer::YesNo { yes: answer.get() }
}

/// What the real AI keeps of a Jev call that sent `request`: each question
/// it sends that `answers` answers, exactly as sent, with that answer, by
/// the name it goes under.
fn kept(
    request: &JevRequest,
    answers: impl IntoIterator<Item = (String, JevAnswer)>,
) -> Vec<JevExchange> {
    let answers: BTreeMap<String, JevAnswer> = answers.into_iter().collect();
    // A test may script fewer answers than the request has questions, as
    // Jev never would: only those answered are kept.
    let mut answered = request.clone();
    answered
        .questions
        .retain(|name, _| answers.contains_key(name));
    let reply = JevReply {
        answers,
        model: JEV_MODEL.to_owned(),
        usage: Usage::default(),
    };
    jev_exchanges(&answered, &reply).expect("each scripted answer is of its question's kind")
}

/// `reply` as a request stamped `stamp` that has it at once, from the
/// scripted model in `role`, keeping what `exchanges` makes of its content,
/// and reporting the cost `script` scripts for `role`, if it scripts one. A
/// reply from the generator carries a generation ID made from the stamp.
fn reply_at_once<T: Send + 'static>(
    stamp: Stamp,
    reply: Result<T, RequestFailure>,
    role: Role,
    script: &Script,
    exchanges: impl FnOnce(&T) -> Vec<JevExchange>,
) -> Pending<T> {
    let model = ModelId::new(
        match role {
            Role::Jev => JEV_MODEL,
            Role::Generator => GENERATOR_MODEL,
        }
        .to_owned(),
    );
    let cost = script.costs.get(&role).copied();
    let reply = reply.map(|content| {
        let generation = GenerationId::new(format!("scripted-generation-{}", stamp.request));
        let provenance = Provenance::new(stamp.request, role, model.clone(), stamp.versions)
            .with_reported_model(model)
            .with_exchanges(exchanges(&content));
        let provenance = match role {
            Role::Jev => provenance,
            Role::Generator => provenance.with_generation(generation),
        };
        Reply {
            content,
            provenance: Box::new(match cost {
                Some(cost) => provenance.with_cost(cost),
                None => provenance,
            }),
        }
    });
    Box::pin(future::ready(reply))
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    use whetstone_core::{
        Area, AssessmentPolicy, CandidateCheck, Change, ConnectedCodeLimit, Connections, Cost,
        CriterionOutcome, Depth, Evidence, ExcludePatterns, OptionLetter, Probability, Question,
        QuestionCount, QuestionType, ReviewChoices, Role, TouchedFile, WrittenCandidate,
        WrittenCitation, WrittenCriterion, WrittenOption, plain_checks, show_rule,
    };

    use whetstone_core::{GenerationId, JevExchange, RequestId};

    use super::*;
    use crate::explanation::explanation_check_request;
    use crate::jev::JevQuestion;
    use crate::port::{Ai, CandidateCount, RequestFailure};
    use crate::question_check::question_check_request;
    use crate::reasoning::reasoning_request;
    use crate::samples::{Acknowledged, stamp};

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

    /// `ai`'s check of `candidate`: its three calls, in order, and their
    /// answers made one, or the first call's failure.
    fn check(
        ai: &ScriptedAi,
        candidate: &Candidate,
    ) -> Result<QuestionCheckAnswers, RequestFailure> {
        let mut answers = Vec::new();
        for call in QuestionCheckCall::ALL {
            answers.push(at_once(ai.check_question(
                stamp(),
                call,
                &evidence(),
                candidate,
            )));
        }
        let answers = answers
            .into_iter()
            .map(|reply| reply.map(|reply| reply.content))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(CheckCallAnswers::merged(answers).expect("each call's answers once"))
    }

    #[test]
    fn the_generator_writes_what_is_scripted_for_each_area_at_once_and_each_request_is_logged() {
        let ai = ScriptedAi::new();
        ai.on_write(
            area(0).id(),
            Ok(vec![written("First?"), written("Second?")]),
        );
        ai.on_write(area(1).id(), Err(RequestFailure::RateLimited));

        let first = at_once(ai.write_candidates(stamp(), &evidence(), &area(0), count()));
        let second = at_once(ai.write_candidates(stamp(), &evidence(), &area(1), count()));

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

        let gate =
            at_once(ai.check_areas(stamp(), &evidence(), &areas)).expect("scripted to answer");

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

        let failed = at_once(ai.check_areas(stamp(), &evidence(), &areas));
        let again = at_once(ai.check_areas(stamp(), &evidence(), &areas));

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

        let first = check(&ai, &candidate("Why retry?"));
        let other = check(&ai, &candidate("Why log?"));
        let again = check(&ai, &candidate("Why retry?"));

        assert_eq!(first, Ok(answers(0.9, 0.1, 0.05)), "as scripted");
        assert_eq!(other, Ok(answers(0.5, 0.5, 0.5)));
        assert_eq!(again, Err(RequestFailure::TimedOut));
        assert_eq!(
            ai.asked(),
            ["Why retry?", "Why log?", "Why retry?"]
                .into_iter()
                .flat_map(
                    |stem| QuestionCheckCall::ALL.map(|call| Asked::CheckQuestion {
                        call,
                        evidence: evidence(),
                        candidate: candidate(stem),
                    })
                )
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn each_call_of_a_check_answers_only_the_checks_it_asks_and_a_failure_fails_every_call() {
        let ai = ScriptedAi::new();
        ai.on_check("Why retry?", Ok(answers(0.9, 0.1, 0.05)));
        ai.on_check("Why retry?", Err(RequestFailure::RateLimited));
        let candidate = candidate("Why retry?");
        let ask = |call| at_once(ai.check_question(stamp(), call, &evidence(), &candidate));

        let ratings = ask(QuestionCheckCall::OptionRatings).expect("scripted to answer");
        let hint = ask(QuestionCheckCall::Hint).expect("scripted to answer");
        let rest = ask(QuestionCheckCall::Candidate).expect("scripted to answer");
        let failed = QuestionCheckCall::ALL.map(|call| ask(call).err());

        assert_eq!(
            ratings.content.iter().collect::<Vec<_>>(),
            answers(0.9, 0.1, 0.05).iter().collect::<Vec<_>>(),
            "the option ratings"
        );
        assert_eq!(ratings.content.depth(), None);
        assert_eq!(hint.content.iter().count(), 0, "none scripted");
        assert_eq!(
            rest.content.depth(),
            Some(depth(0.2, 0.3, 0.5, 1.3)),
            "the depth goes with the candidate call"
        );
        assert_eq!(rest.provenance.role(), Role::Jev);
        assert_eq!(failed, [Some(RequestFailure::RateLimited); 3]);
    }

    #[test]
    fn one_call_of_a_check_scripted_to_fail_fails_alone_and_its_retry_is_answered() {
        let ai = ScriptedAi::new();
        ai.on_check_call_failing(
            "Why retry?",
            QuestionCheckCall::Hint,
            RequestFailure::TimedOut,
        );
        ai.on_check_passing("Why retry?");
        let candidate = candidate("Why retry?");
        let ask = |call| at_once(ai.check_question(stamp(), call, &evidence(), &candidate));

        let first = QuestionCheckCall::ALL.map(|call| ask(call).err());
        let retried = ask(QuestionCheckCall::Hint).expect("the check's own reply");

        assert_eq!(first, [None, Some(RequestFailure::TimedOut), None]);
        assert_eq!(
            retried.content.iter().collect::<Vec<_>>(),
            [(CandidateCheck::HintHidesAnswerKey, probability(0.9))]
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

        let passing = check(&ai, &candidate);
        let except = check(&ai, &candidate);

        let answers = |answers: &[(CandidateCheck, f64)]| {
            QuestionCheckAnswers::new(
                answers
                    .iter()
                    .map(|&(check, value)| (check, probability(value))),
                depth(0.1, 0.8, 0.1, 1.0),
            )
        };
        assert_eq!(
            passing,
            Ok(answers(&[
                (Answerable, 0.9),
                (AboutArea, 0.9),
                (NeedsChange, 0.9),
                (NoGiveaway, 0.9),
                (Criterion(0), 0.9),
                (Criterion(1), 0.9),
                (CriterionMinimum(0), 0.9),
                (CriterionMinimum(1), 0.9),
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
            except,
            Ok(answers(&[
                (Answerable, 0.9),
                (AboutArea, 0.9),
                (NeedsChange, 0.5),
                (NoGiveaway, 0.9),
                (Criterion(0), 0.9),
                (Criterion(1), 0.9),
                (CriterionMinimum(0), 0.9),
                (CriterionMinimum(1), 0.9),
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

        let deep = check(&ai, &candidate).expect("scripted to pass");
        let usual = check(&ai, &candidate).expect("scripted to pass");

        assert_eq!(deep.depth(), deepest, "the depth scripted");
        assert_eq!(
            deep.iter().collect::<Vec<_>>(),
            usual.iter().collect::<Vec<_>>(),
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
            stamp(),
            &Acknowledged("It retries."),
            &evidence(),
            QuestionWithoutKey::from(&asked),
        ));
        let again = at_once(ai.assess_reasoning(
            stamp(),
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
    fn the_explanation_and_its_check_are_what_is_scripted_for_each_question_and_logged() {
        let ai = ScriptedAi::new();
        let paragraph = WrittenParagraph {
            criterion: 0,
            outcome: CriterionOutcome::NotMet,
            text: "You didn't name the retry.".to_owned(),
            citations: vec![WrittenCitation {
                path: "src/pay.rs".to_owned(),
                first_line: 2,
                last_line: 2,
            }],
        };
        let check = ParagraphCheck {
            supported: Probability::new(0.9).expect("from 0 to 1"),
            agrees: Probability::new(0.85).expect("from 0 to 1"),
        };
        ai.on_explain("Why retry?", Ok(vec![paragraph.clone()]));
        ai.on_explain("Why retry?", Err(RequestFailure::TimedOut));
        ai.on_check_explanation("Why retry?", Ok(vec![check]));
        let asked = question("Why retry?");
        let criteria = [(0, CriterionOutcome::NotMet)];

        let explained = at_once(ai.explain(
            stamp(),
            &Acknowledged("It retries."),
            &evidence(),
            &asked,
            &criteria,
        ));
        let checked = at_once(ai.check_explanation(
            stamp(),
            &Acknowledged("It retries."),
            &evidence(),
            &asked,
            std::slice::from_ref(&paragraph),
        ));
        let again = at_once(ai.explain(
            stamp(),
            &Acknowledged("It retries."),
            &evidence(),
            &asked,
            &criteria,
        ));

        let explained = explained.expect("scripted to explain");
        assert_eq!(
            explained.content,
            std::slice::from_ref(&paragraph),
            "as scripted"
        );
        assert_eq!(explained.provenance.role(), Role::Generator);
        let checked = checked.expect("scripted to check");
        assert_eq!(checked.content, [check], "as scripted");
        assert_eq!(checked.provenance.role(), Role::Jev);
        assert_eq!(again, Err(RequestFailure::TimedOut));
        let explain = Asked::Explain {
            saved: "Acknowledged(\"It retries.\")".to_owned(),
            evidence: evidence(),
            stem: "Why retry?".to_owned(),
            reasoning: "It retries.".to_owned(),
            criteria: criteria.to_vec(),
        };
        assert_eq!(
            ai.asked(),
            [
                explain.clone(),
                Asked::CheckExplanation {
                    saved: "Acknowledged(\"It retries.\")".to_owned(),
                    evidence: evidence(),
                    stem: "Why retry?".to_owned(),
                    paragraphs: vec![paragraph],
                },
                explain,
            ]
        );
    }

    /// The cost `reply` reports, if it came and reports one.
    fn reported<T>(reply: Result<Reply<T>, RequestFailure>) -> Option<Cost> {
        reply.ok().and_then(|reply| reply.provenance.cost())
    }

    #[test]
    fn each_reply_reports_the_cost_scripted_for_its_role_from_then_on_and_none_by_default() {
        let ai = ScriptedAi::new();
        ai.on_write(area(0).id(), Ok(Vec::new()));
        ai.on_write(area(0).id(), Ok(Vec::new()));
        let cost = |usd| Cost::new(usd).expect("a cost");

        let unscripted = at_once(ai.check_areas(stamp(), &evidence(), &[area(0)]));
        ai.report_cost(Role::Jev, Some(cost(0.25)));
        ai.report_cost(Role::Generator, Some(cost(1.5)));
        let gate = at_once(ai.check_areas(stamp(), &evidence(), &[area(0)]));
        let write = at_once(ai.write_candidates(stamp(), &evidence(), &area(0), count()));
        ai.report_cost(Role::Generator, None);
        let unreported = at_once(ai.write_candidates(stamp(), &evidence(), &area(0), count()));

        assert_eq!(reported(unscripted), None, "none until a test scripts one");
        assert_eq!(reported(gate), Some(cost(0.25)));
        assert_eq!(reported(write), Some(cost(1.5)));
        assert_eq!(reported(unreported), None);
    }

    /// `stamp()` as request `id`.
    fn stamped(id: u64) -> Stamp {
        Stamp {
            request: RequestId::new(id),
            ..stamp()
        }
    }

    /// Each question `exchanges` keep, by name, with its instructions, and
    /// each yes/no answer.
    fn kept(exchanges: &[JevExchange]) -> Vec<(&str, &str, Option<f64>)> {
        exchanges
            .iter()
            .map(|exchange| match exchange {
                JevExchange::YesNo {
                    name,
                    instructions,
                    answer,
                    ..
                } => (name.as_str(), instructions.as_str(), Some(answer.get())),
                JevExchange::Score {
                    name, instructions, ..
                } => (name.as_str(), instructions.as_str(), None),
            })
            .collect()
    }

    /// Each question `request` sends, by name, with its instructions.
    fn sent(request: &JevRequest) -> Vec<(&str, &str)> {
        request
            .questions
            .iter()
            .map(|(name, question)| match question {
                JevQuestion::YesNo { instructions, .. }
                | JevQuestion::Score { instructions, .. } => (name.as_str(), instructions.as_str()),
            })
            .collect()
    }

    #[test]
    fn each_reply_carries_its_stamp_and_the_generators_a_generation_id() {
        let ai = ScriptedAi::new();
        ai.on_write(area(0).id(), Ok(Vec::new()));

        let gate =
            at_once(ai.check_areas(stamped(7), &evidence(), &[area(0)])).expect("let through");
        let write = at_once(ai.write_candidates(stamped(8), &evidence(), &area(0), count()))
            .expect("scripted to write");

        assert_eq!(gate.provenance.request(), RequestId::new(7));
        assert_eq!(gate.provenance.versions(), &stamp().versions);
        assert_eq!(gate.provenance.generation(), None, "Rig gives Jev's none");
        assert_eq!(write.provenance.request(), RequestId::new(8));
        assert_eq!(
            write.provenance.generation().map(GenerationId::as_str),
            Some("scripted-generation-8")
        );
        assert_eq!(
            write.provenance.exchanges(),
            [],
            "the generator answers no question"
        );
    }

    #[test]
    fn the_gate_and_the_question_check_keep_every_question_the_real_ai_sends_with_its_answer() {
        let ai = ScriptedAi::new();
        let model = ModelId::new(JEV_MODEL.to_owned());
        let areas = [area(0), area(1)];
        ai.on_gate(area(0).id(), probability(0.31));
        let candidate = candidate_with("Why?", "A", 2);
        ai.on_check_passing_at("Why?", depth(0.1, 0.2, 0.7, 1.6));

        let gate = at_once(ai.check_areas(stamp(), &evidence(), &areas)).expect("scripted");
        let checks = QuestionCheckCall::ALL.map(|call| {
            at_once(ai.check_question(stamp(), call, &evidence(), &candidate)).expect("scripted")
        });

        let gate_request = gate_request(&model, &evidence(), &areas);
        let gate_sent = sent(&gate_request);
        assert_eq!(
            kept(gate.provenance.exchanges()),
            [
                (gate_sent[0].0, gate_sent[0].1, Some(0.31)),
                (gate_sent[1].0, gate_sent[1].1, Some(0.9)),
            ]
        );
        for (call, check) in QuestionCheckCall::ALL.into_iter().zip(&checks) {
            let check_request = question_check_request(&model, &evidence(), &candidate, call);
            assert_eq!(
                kept(check.provenance.exchanges())
                    .iter()
                    .map(|&(name, instructions, _)| (name, instructions))
                    .collect::<Vec<_>>(),
                sent(&check_request),
                "every question the {call:?} call sends"
            );
        }
        let exchanges: Vec<_> = checks
            .iter()
            .flat_map(|check| kept(check.provenance.exchanges()))
            .collect();
        let answer = |asked: &str| {
            exchanges
                .iter()
                .find_map(|&(name, _, answer)| (name == asked).then_some(answer))
                .flatten()
        };
        assert_eq!(
            [
                answer("option_a"),
                answer("option_b"),
                answer("criterion_2")
            ],
            [Some(0.9), Some(0.1), Some(0.9)]
        );
        assert_eq!(
            checks[2]
                .provenance
                .exchanges()
                .iter()
                .find_map(|exchange| match exchange {
                    JevExchange::Score { answer, .. } => Some(*answer),
                    JevExchange::YesNo { .. } => None,
                }),
            Some(depth(0.1, 0.2, 0.7, 1.6))
        );
    }

    #[test]
    fn the_reasoning_and_the_explanation_check_keep_every_question_sent_with_its_answer() {
        let ai = ScriptedAi::new();
        let model = ModelId::new(JEV_MODEL.to_owned());
        let question = question("What?");
        ai.on_assess("What?", Ok(probabilities(&[0.62])));
        let paragraphs = vec![WrittenParagraph {
            criterion: 0,
            outcome: CriterionOutcome::NotMet,
            text: "It retries.".to_owned(),
            citations: Vec::new(),
        }];
        ai.on_check_explanation(
            "What?",
            Ok(vec![ParagraphCheck {
                supported: probability(0.84),
                agrees: probability(0.45),
            }]),
        );
        let saved = Acknowledged("It dedupes.");

        let assessed = at_once(ai.assess_reasoning(
            stamp(),
            &saved,
            &evidence(),
            QuestionWithoutKey::from(&question),
        ))
        .expect("scripted");
        let checked =
            at_once(ai.check_explanation(stamp(), &saved, &evidence(), &question, &paragraphs))
                .expect("scripted");

        let reasoning_request = reasoning_request(
            &model,
            &evidence(),
            QuestionWithoutKey::from(&question),
            "It dedupes.",
        );
        assert_eq!(
            kept(assessed.provenance.exchanges()),
            [("criterion_1", sent(&reasoning_request)[0].1, Some(0.62))]
        );
        let check_request = explanation_check_request(&model, &evidence(), &question, &paragraphs);
        let check_sent = sent(&check_request);
        assert_eq!(
            kept(checked.provenance.exchanges()),
            [
                ("criterion_1_agrees", check_sent[0].1, Some(0.45)),
                ("criterion_1_supported", check_sent[1].1, Some(0.84)),
            ]
        );
    }

    #[test]
    #[should_panic(expected = "no write scripted")]
    fn a_request_nothing_was_scripted_for_fails_the_test_as_it_is_made() {
        drop(ScriptedAi::new().write_candidates(stamp(), &evidence(), &area(0), count()));
    }
}
