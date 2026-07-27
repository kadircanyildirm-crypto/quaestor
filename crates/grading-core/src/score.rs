//! The scoring function whose execution the zk proof will attest to.
//!
//! Everything here is integer arithmetic. The final score is
//! `credited_weight * 10_000 / total_weight` with u64 intermediates (no
//! overflow: weights are u32, so the sum of ≤ 2^32 entries fits u64 only if we
//! also bound the entry count — we cap exams at [`MAX_QUESTIONS`], far above
//! any real exam, which keeps `sum(weights) <= 2^32 * 2^16 = 2^48` and the
//! product under `2^62`).

use crate::commit::{commit_answer_key, hash_answer_sheet, Salt};
use crate::model::{AnswerKey, AnswerSheet, CancelPolicy, ScoreReport};

/// Generous upper bound on questions per exam; exists only to make overflow
/// reasoning trivial.
pub const MAX_QUESTIONS: usize = 65_536;

/// A perfect score, in basis points. Also the verifier-side upper bound: a
/// report claiming more than this cannot have come from this program.
pub const FULL_SCORE_BP: u32 = 10_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScoreError {
    /// Key and sheet reference different exams.
    ExamIdMismatch { key: u64, sheet: u64 },
    /// Sheet has a different number of answers than the key has questions.
    LengthMismatch { key: usize, sheet: usize },
    /// The key is malformed (validated before any scoring).
    InvalidKey(KeyError),
    /// A submitted answer is not a valid choice for this exam.
    ChoiceOutOfRange { question_id: u32, choice: u8 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyError {
    TooManyQuestions {
        count: usize,
    },
    /// `num_choices` must be at least 2 for a multiple-choice exam.
    TooFewChoices {
        num_choices: u8,
    },
    DuplicateQuestionId {
        question_id: u32,
    },
    /// A non-cancelled question must accept at least one choice.
    NoAcceptedChoice {
        question_id: u32,
    },
    /// Accepted choices must be sorted, unique, and within `0..num_choices`.
    MalformedAccepted {
        question_id: u32,
    },
}

/// Structural validation of an answer key, independent of any sheet.
/// The zk guest runs this too: a proof over a malformed key must be
/// impossible, not merely unlikely.
pub fn validate_key(key: &AnswerKey) -> Result<(), KeyError> {
    if key.entries.len() > MAX_QUESTIONS {
        return Err(KeyError::TooManyQuestions {
            count: key.entries.len(),
        });
    }
    if key.num_choices < 2 {
        return Err(KeyError::TooFewChoices {
            num_choices: key.num_choices,
        });
    }
    for (i, entry) in key.entries.iter().enumerate() {
        // O(n^2) id-uniqueness scan: n <= 65_536 and this runs inside a zkVM
        // guest where a HashSet is unavailable and sorting a copy costs more
        // cycles than it saves at real exam sizes (n ~ 100).
        if key.entries[..i]
            .iter()
            .any(|e| e.question_id == entry.question_id)
        {
            return Err(KeyError::DuplicateQuestionId {
                question_id: entry.question_id,
            });
        }
        if !entry.cancelled && entry.accepted.is_empty() {
            return Err(KeyError::NoAcceptedChoice {
                question_id: entry.question_id,
            });
        }
        let sorted_unique_in_range = entry.accepted.windows(2).all(|w| w[0] < w[1])
            && entry.accepted.last().is_none_or(|&c| c < key.num_choices);
        if !sorted_unique_in_range {
            return Err(KeyError::MalformedAccepted {
                question_id: entry.question_id,
            });
        }
    }
    Ok(())
}

/// Grade one sheet against one key and produce the full public report,
/// including the commitment and sheet hash it is bound to.
///
/// This is the exact function the zkVM guest executes: private inputs
/// `(key, salt)`, public inputs derived here.
pub fn score(key: &AnswerKey, salt: &Salt, sheet: &AnswerSheet) -> Result<ScoreReport, ScoreError> {
    validate_key(key).map_err(ScoreError::InvalidKey)?;
    score_validated(key, &commit_answer_key(key, salt), sheet)
}

/// The per-sheet half of [`score`], split out so a whole sitting pays for key
/// validation and the key commitment once instead of once per candidate.
///
/// The two hoisted steps are the expensive ones and neither depends on the
/// sheet: [`validate_key`] is a quadratic id scan, and `commit_answer_key`
/// hashes the entire encoded key. Inside a zkVM that is the difference between
/// a batch costing `O(sheets * key)` and `O(sheets + key)` — which is the whole
/// argument for batching in the first place.
///
/// Not public: the caller carries the obligation that `key` passed
/// [`validate_key`] and that `key_commitment` is `commit_answer_key(key, salt)`
/// for the salt the report will be published under. `grade_batch` and [`score`]
/// are the only callers, and `batch_reports_match_scoring_each_sheet_alone`
/// pins the two against each other.
pub(crate) fn score_validated(
    key: &AnswerKey,
    key_commitment: &[u8; 32],
    sheet: &AnswerSheet,
) -> Result<ScoreReport, ScoreError> {
    if key.exam_id != sheet.exam_id {
        return Err(ScoreError::ExamIdMismatch {
            key: key.exam_id,
            sheet: sheet.exam_id,
        });
    }
    if key.entries.len() != sheet.answers.len() {
        return Err(ScoreError::LengthMismatch {
            key: key.entries.len(),
            sheet: sheet.answers.len(),
        });
    }

    let mut total_weight: u64 = 0;
    let mut credited_weight: u64 = 0;
    let (mut correct, mut wrong, mut blank, mut cancelled) = (0u32, 0u32, 0u32, 0u32);

    for (entry, answer) in key.entries.iter().zip(&sheet.answers) {
        if let Some(choice) = answer {
            if *choice >= key.num_choices {
                return Err(ScoreError::ChoiceOutOfRange {
                    question_id: entry.question_id,
                    choice: *choice,
                });
            }
        }

        if entry.cancelled {
            cancelled += 1;
            match key.cancel_policy {
                CancelPolicy::FullCredit => {
                    total_weight += entry.weight as u64;
                    credited_weight += entry.weight as u64;
                }
                CancelPolicy::Redistribute => {} // excluded from both sums
            }
            continue;
        }

        total_weight += entry.weight as u64;
        match answer {
            None => blank += 1,
            Some(choice) if entry.accepted.binary_search(choice).is_ok() => {
                correct += 1;
                credited_weight += entry.weight as u64;
            }
            Some(_) => wrong += 1,
        }
    }

    // Division by zero here means an all-cancelled (or zero-weight) exam:
    // 0/0. Real-world convention is full credit — the institution, not the
    // student, destroyed the denominator.
    let score_bp = (credited_weight * FULL_SCORE_BP as u64)
        .checked_div(total_weight)
        .map_or(FULL_SCORE_BP, |v| v as u32);

    Ok(ScoreReport {
        exam_id: key.exam_id,
        key_commitment: *key_commitment,
        sheet_hash: hash_answer_sheet(sheet),
        score_bp,
        correct,
        wrong,
        blank,
        cancelled,
    })
}
