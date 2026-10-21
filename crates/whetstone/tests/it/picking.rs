//! Picking, against a scripted AI: of the questions that pass their checks,
//! Practice shows the deepest first, those that need an understanding of how
//! the change affects the code around it, and spreads them across the chosen
//! areas, so one area never takes every slot. Depth only orders questions:
//! it never passes or fails one.

use std::sync::Arc;

use whetstone_ai::ScriptedAi;
use whetstone_core::{Depth, Probability};

use crate::preparing_questions::{
    DOMAIN, INFRASTRUCTURE, SERVE_OVER_HTTP, area_id, candidate, generate, reasoning_met, stems,
};
use crate::questions_screen::every_stem_shown;
use crate::session::Session;

/// Jev's depth score whose expected value is `expected`, from 0, answerable
/// by reading one line, to 2, needing an understanding of the code around
/// the change: its probability split between the two levels either side.
fn depth(expected: f64) -> Depth {
    let probability = |value: f64| Probability::new(value).expect("from 0 to 1");
    let distribution = if expected <= 1.0 {
        [1.0 - expected, expected, 0.0]
    } else {
        [0.0, 2.0 - expected, expected - 1.0]
    };
    Depth::new(distribution.map(probability), expected).expect("from 0 to 2")
}

/// Scripts `ai` to write one candidate about the area at `area` for each of
/// `depths`, in that order, asking the stems [`stems`] gives for `name`.
/// Each passes every check, Jev scores its depth at that expected value, and
/// its reasoning, once submitted, meets its criterion. Returns their stems.
fn write_at_depths(ai: &ScriptedAi, area: usize, name: &str, depths: &[f64]) -> Vec<String> {
    let stems = stems(name, depths.len());
    ai.on_write(
        &area_id(area),
        Ok(stems
            .iter()
            .map(|stem| candidate(stem, "A", SERVE_OVER_HTTP))
            .collect()),
    );
    for (stem, &expected) in stems.iter().zip(depths) {
        ai.on_check_passing_at(stem, depth(expected));
        reasoning_met(ai, stem);
    }
    stems
}

/// A session that prepared the questions `ai` writes and checks about the
/// areas at `areas` for the change "Serve payments over HTTP".
fn prepared(ai: &Arc<ScriptedAi>, areas: &[usize]) -> Session {
    let mut session = generate(ai, areas, SERVE_OVER_HTTP, 120, 32);
    session.settle();
    session
}

#[test]
fn an_areas_deepest_questions_come_first_and_equally_deep_ones_in_the_order_written() {
    let ai = Arc::new(ScriptedAi::new());
    let stems = write_at_depths(&ai, DOMAIN, "Domain", &[0.4, 1.9, 1.0, 0.1, 1.5, 1.0]);
    let mut session = prepared(&ai, &[DOMAIN]);

    assert_eq!(
        every_stem_shown(&mut session, &stems),
        [
            "Domain question 2?",
            "Domain question 5?",
            "Domain question 3?",
            "Domain question 6?",
        ],
        "1.9, 1.5, then the two at 1.0 as written; 0.4 and 0.1 aren't needed"
    );
}

#[test]
fn depth_never_withholds_a_question_so_the_shallowest_still_fill_the_set() {
    let ai = Arc::new(ScriptedAi::new());
    let stems = write_at_depths(&ai, DOMAIN, "Domain", &[0.0, 2.0, 0.0]);
    let mut session = prepared(&ai, &[DOMAIN]);

    assert_eq!(
        session.question().rows()[1].trim(),
        "3 passed checks, 4 asked for · see why in actions"
    );
    assert_eq!(
        every_stem_shown(&mut session, &stems),
        [
            "Domain question 2?",
            "Domain question 1?",
            "Domain question 3?",
        ],
        "every question that passed, deepest first"
    );
}

#[test]
fn questions_are_taken_in_turn_across_the_areas_by_each_ones_deepest_remaining() {
    let ai = Arc::new(ScriptedAi::new());
    let domain = write_at_depths(&ai, DOMAIN, "Domain", &[0.9, 1.0, 0.8]);
    let infrastructure = write_at_depths(&ai, INFRASTRUCTURE, "Infrastructure", &[0.1, 1.5, 0.2]);
    let mut session = prepared(&ai, &[DOMAIN, INFRASTRUCTURE]);

    assert_eq!(
        every_stem_shown(&mut session, &[domain, infrastructure].concat()),
        [
            // Infrastructure's deepest, 1.5, leads domain's, 1.0.
            "Infrastructure question 2?",
            "Domain question 2?",
            // Then domain's next, 0.9, leads infrastructure's, 0.2.
            "Domain question 1?",
            "Infrastructure question 3?",
        ]
    );
}

#[test]
fn an_area_with_deeper_questions_never_takes_every_slot() {
    let ai = Arc::new(ScriptedAi::new());
    let domain = write_at_depths(&ai, DOMAIN, "Domain", &[2.0, 2.0, 2.0]);
    let infrastructure = write_at_depths(&ai, INFRASTRUCTURE, "Infrastructure", &[0.0, 0.0, 0.0]);
    let mut session = prepared(&ai, &[DOMAIN, INFRASTRUCTURE]);

    assert_eq!(
        every_stem_shown(&mut session, &[domain, infrastructure].concat()),
        [
            "Domain question 1?",
            "Infrastructure question 1?",
            "Domain question 2?",
            "Infrastructure question 2?",
        ],
        "the shallowest area still gets every other slot"
    );
}

#[test]
fn areas_whose_deepest_are_equally_deep_go_in_the_order_the_picker_offers_them() {
    let ai = Arc::new(ScriptedAi::new());
    let domain = write_at_depths(&ai, DOMAIN, "Domain", &[0.5, 1.2, 0.5]);
    let infrastructure = write_at_depths(&ai, INFRASTRUCTURE, "Infrastructure", &[1.2, 0.5, 0.5]);
    let mut session = prepared(&ai, &[DOMAIN, INFRASTRUCTURE]);

    assert_eq!(
        every_stem_shown(&mut session, &[domain, infrastructure].concat()),
        [
            "Domain question 2?",
            "Infrastructure question 1?",
            "Domain question 1?",
            "Infrastructure question 2?",
        ],
        "at 1.2 and then at 0.5, domain before infrastructure, as the picker offers them, \
         and each area's in the order written"
    );
}
