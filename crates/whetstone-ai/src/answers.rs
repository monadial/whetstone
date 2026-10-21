//! Reading Jev's answers into whetstone-core's types, strictly: an answer
//! that is missing, of another kind, out of range or to a question that
//! wasn't asked makes the whole reply invalid, never coerced (rule 37).

use std::collections::BTreeMap;

use whetstone_core::{Depth, Probability};

use crate::failure::AiError;
use crate::jev::{JevAnswer, JevReply};

/// The answers of one reply not yet read.
pub(crate) struct Answers<'a> {
    unread: BTreeMap<&'a str, &'a JevAnswer>,
}

impl<'a> Answers<'a> {
    /// Every answer in `reply`, unread.
    pub(crate) fn of(reply: &'a JevReply) -> Self {
        Self {
            unread: reply
                .answers
                .iter()
                .map(|(name, answer)| (name.as_str(), answer))
                .collect(),
        }
    }

    /// The probability of yes Jev answered the yes/no question `name` with.
    pub(crate) fn yes_no(&mut self, name: &str) -> Result<Probability, AiError> {
        match self.unread.remove(name) {
            Some(&JevAnswer::YesNo { yes }) => {
                Probability::new(yes).map_err(|_| AiError::InvalidReply)
            }
            _ => Err(AiError::InvalidReply),
        }
    }

    /// The depth Jev scored the score question `name` with: its probability
    /// for each of the three levels and its expected value, as returned.
    pub(crate) fn depth(&mut self, name: &str) -> Result<Depth, AiError> {
        let Some(JevAnswer::Score {
            probabilities,
            expected,
        }) = self.unread.remove(name)
        else {
            return Err(AiError::InvalidReply);
        };
        let [one_line, the_change, code_around] = probabilities.as_slice() else {
            return Err(AiError::InvalidReply);
        };
        let probability = |value: f64| Probability::new(value).map_err(|_| AiError::InvalidReply);
        Depth::new(
            [
                probability(*one_line)?,
                probability(*the_change)?,
                probability(*code_around)?,
            ],
            *expected,
        )
        .map_err(|_| AiError::InvalidReply)
    }

    /// Succeeds when every answer was read: the reply answered nothing it
    /// wasn't asked.
    pub(crate) fn done(self) -> Result<(), AiError> {
        if self.unread.is_empty() {
            Ok(())
        } else {
            Err(AiError::InvalidReply)
        }
    }
}
