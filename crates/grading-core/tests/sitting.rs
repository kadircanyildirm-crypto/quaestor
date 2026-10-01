//! Tests for grading a whole sitting as one batch, and for the check a student
//! runs against the resulting proof.
//!
//! Batching changes what a grade *is*. With per-sheet proofs a student holds a
//! proof of their own score; with one proof per sitting they hold a proof about
//! 200,000 people plus a claim that one leaf of it is theirs. Everything that
//! can go wrong moves into that claim, so most of what follows is forged
//! inclusion: right sitting wrong candidate, right candidate wrong score, right
//! everything under a key that was never committed.

use grading_core::{
    check_batch_inclusion, commit_answer_key, encode_batch_public_values, grade_batch,
    hash_answer_sheet, score, AnswerKey, AnswerSheet, BatchError, BatchGradeError, BatchOutcome,
    CancelPolicy, KeyEntry, KeyError, MerklePath, ScoreError, VerifyError,
};

const SALT: [u8; 32] = [1u8; 32];
const EXAM_ID: u64 = 20260701;

/// The demo exam: five equally weighted questions, one with two accepted
/// answers (a post-appeal ruling), one cancelled under full-credit policy.
fn sitting_key() -> AnswerKey {
    let q = |question_id: u32, accepted: Vec<u8>, cancelled: bool| KeyEntry {
        question_id,
        weight: 20,
        accepted,
        cancelled,
    };
    AnswerKey {
        exam_id: EXAM_ID,
        num_choices: 5,
        cancel_policy: CancelPolicy::FullCredit,
        entries: vec![
            q(1, vec![2], false),
            q(2, vec![0], false),
            q(3, vec![1, 3], false),
            q(4, vec![4], false),
            q(5, vec![], true),
        ],
    }
}

fn sheet(pseudonym: u8, answers: Vec<Option<u8>>) -> AnswerSheet {
    AnswerSheet {
        exam_id: EXAM_ID,
        student_pseudonym: [pseudonym; 32],
        answers,
    }
}

/// Three candidates — deliberately odd, so the tree's promotion path is
/// exercised by the ordinary case and not only by a corner test.
fn candidates() -> Vec<AnswerSheet> {
    vec![
        sheet(0xA1, vec![Some(2), Some(0), Some(3), Some(1), None]), // 80%: 3 right, 1 wrong
        sheet(0xB2, vec![Some(2), Some(1), Some(3), Some(1), None]), // 60%: 2 right, 2 wrong
        sheet(0xC3, vec![Some(2), Some(0), Some(1), Some(4), None]), // 100%
    ]
}

fn sitting() -> BatchOutcome {
    grade_batch(&sitting_key(), &SALT, &candidates()).unwrap()
}

fn published_commitment() -> [u8; 32] {
    commit_answer_key(&sitting_key(), &SALT)
}

/// What the institution publishes: the proof's public values. Everything a
/// student checks is derived from these bytes plus their own answers.
fn public_values(outcome: &BatchOutcome) -> [u8; 76] {
    encode_batch_public_values(&outcome.public_values())
}

/// The invariant that licenses the whole optimisation. `grade_batch` hoists key
/// validation and the key commitment out of the per-sheet loop, which is only
/// sound if a candidate's report is bit-identical to what they would have got
/// from a standalone proof. If this ever fails, batching silently becomes a
/// different grading system wearing the same name.
#[test]
fn batch_reports_match_scoring_each_sheet_alone() {
    let key = sitting_key();
    let outcome = sitting();

    for (report, sheet) in outcome.reports.iter().zip(candidates()) {
        assert_eq!(*report, score(&key, &SALT, &sheet).unwrap());
    }
    assert_eq!(outcome.reports[0].score_bp, 8_000);
    assert_eq!(outcome.reports[1].score_bp, 6_000);
    assert_eq!(outcome.reports[2].score_bp, 10_000);
}

#[test]
fn the_sitting_publishes_one_root_for_every_candidate() {
    let outcome = sitting();
    let pv = outcome.public_values();

    assert_eq!(pv.leaf_count, 3);
    assert_eq!(pv.exam_id, EXAM_ID);
    assert_eq!(pv.key_commitment, published_commitment());
    assert_eq!(pv.batch_root, outcome.root);
    // Every report carries the same commitment: one key graded the sitting.
    assert!(outcome
        .reports
        .iter()
        .all(|r| r.key_commitment == published_commitment()));
}

#[test]
fn every_candidate_can_check_their_own_score_from_public_data_alone() {
    let outcome = sitting();
    let bytes = public_values(&outcome);

    for (index, sheet) in candidates().iter().enumerate() {
        let mine = hash_answer_sheet(sheet);
        let checked = check_batch_inclusion(
            &bytes,
            &published_commitment(),
            &outcome.reports[index],
            &outcome.path(index).unwrap(),
            Some(&mine),
        )
        .expect("candidate could not verify their own inclusion");
        assert_eq!(checked.leaf_count, 3);
    }
}

/// An auditor holds no answer sheets and can still establish that a published
/// results row belongs to the proven sitting — just not whose it is.
#[test]
fn an_auditor_can_check_inclusion_without_holding_anyones_answers() {
    let outcome = sitting();
    assert!(check_batch_inclusion(
        &public_values(&outcome),
        &published_commitment(),
        &outcome.reports[1],
        &outcome.path(1).unwrap(),
        None,
    )
    .is_ok());
}

#[test]
fn a_score_raised_after_proving_has_no_inclusion_path() {
    let outcome = sitting();
    // The institution proves the sitting honestly, then edits one row of the
    // published results. The proof still verifies; this must not.
    let mut inflated = outcome.reports[0].clone();
    inflated.score_bp = 10_000;

    let err = check_batch_inclusion(
        &public_values(&outcome),
        &published_commitment(),
        &inflated,
        &outcome.path(0).unwrap(),
        Some(&hash_answer_sheet(&candidates()[0])),
    )
    .unwrap_err();
    assert_eq!(
        err,
        VerifyError::NotInBatch {
            index: 0,
            leaf_count: 3
        }
    );
}

#[test]
fn one_candidates_path_does_not_open_another_candidates_report() {
    let outcome = sitting();
    let bytes = public_values(&outcome);

    let err = check_batch_inclusion(
        &bytes,
        &published_commitment(),
        &outcome.reports[2],
        &outcome.path(0).unwrap(),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, VerifyError::NotInBatch { .. }));
}

#[test]
fn a_report_from_a_different_sitting_is_not_in_this_one() {
    let outcome = sitting();
    // A genuine report, honestly graded under the same committed key — for a
    // candidate who was not in this batch.
    let absent = score(
        &sitting_key(),
        &SALT,
        &sheet(0xD4, vec![Some(2), Some(0), Some(1), Some(4), None]),
    )
    .unwrap();

    let err = check_batch_inclusion(
        &public_values(&outcome),
        &published_commitment(),
        &absent,
        &outcome.path(2).unwrap(),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, VerifyError::NotInBatch { .. }));
}

#[test]
fn a_sitting_proven_under_an_uncommitted_key_is_rejected() {
    // The institution grades against a key it swapped in after the exam. The
    // batch proof is genuine; the commitment it was produced under is not the
    // one published beforehand.
    let mut swapped = sitting_key();
    swapped.entries[3].accepted = vec![1];
    let outcome = grade_batch(&swapped, &SALT, &candidates()).unwrap();

    let err = check_batch_inclusion(
        &public_values(&outcome),
        &published_commitment(),
        &outcome.reports[0],
        &outcome.path(0).unwrap(),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, VerifyError::CommitmentMismatch { .. }));
}

#[test]
fn a_student_checking_against_the_wrong_row_is_told_so() {
    let outcome = sitting();
    // Candidate 0 looks up candidate 1's row: the report is genuinely in the
    // sitting, it is simply not about their answers.
    let err = check_batch_inclusion(
        &public_values(&outcome),
        &published_commitment(),
        &outcome.reports[1],
        &outcome.path(1).unwrap(),
        Some(&hash_answer_sheet(&candidates()[0])),
    )
    .unwrap_err();
    assert!(matches!(err, VerifyError::SheetHashMismatch { .. }));
}

#[test]
fn a_report_for_another_exam_is_rejected_even_if_its_key_matches() {
    let outcome = sitting();
    let mut foreign = outcome.reports[0].clone();
    foreign.exam_id = EXAM_ID + 1;

    let err = check_batch_inclusion(
        &public_values(&outcome),
        &published_commitment(),
        &foreign,
        &outcome.path(0).unwrap(),
        None,
    )
    .unwrap_err();
    assert_eq!(
        err,
        VerifyError::ExamIdMismatch {
            expected: EXAM_ID,
            found: EXAM_ID + 1,
        }
    );
}

#[test]
fn a_restated_sitting_size_does_not_open_a_path() {
    let outcome = sitting();
    // Claim the sitting had four candidates. The count is inside the root's
    // preimage, so no path opens against the restated public values.
    let mut pv = outcome.public_values();
    pv.leaf_count = 4;

    let err = check_batch_inclusion(
        &encode_batch_public_values(&pv),
        &published_commitment(),
        &outcome.reports[0],
        &outcome.path(0).unwrap(),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, VerifyError::NotInBatch { .. }));
}

#[test]
fn a_path_padded_with_an_extra_sibling_is_rejected() {
    let outcome = sitting();
    let mut path = outcome.path(1).unwrap();
    path.siblings.push([0x42; 32]);

    assert!(check_batch_inclusion(
        &public_values(&outcome),
        &published_commitment(),
        &outcome.reports[1],
        &path,
        None,
    )
    .is_err());
}

#[test]
fn an_empty_path_claiming_a_lone_candidate_is_rejected() {
    let outcome = sitting();
    let empty = MerklePath {
        index: 0,
        siblings: vec![],
    };
    assert!(check_batch_inclusion(
        &public_values(&outcome),
        &published_commitment(),
        &outcome.reports[0],
        &empty,
        None,
    )
    .is_err());
}

#[test]
fn malformed_public_values_are_rejected_before_anything_else() {
    let outcome = sitting();
    let truncated = &public_values(&outcome)[..75];

    let err = check_batch_inclusion(
        truncated,
        &published_commitment(),
        &outcome.reports[0],
        &outcome.path(0).unwrap(),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, VerifyError::Malformed(_)));
}

/// Two candidates with the same score are still distinct leaves, because a
/// leaf is the whole report — sheet hash included. Without this, a student
/// could open the sitting with a same-scoring stranger's path and learn
/// nothing about whether their own answers were the ones graded.
#[test]
fn equal_scores_do_not_collapse_into_one_leaf() {
    let tied = vec![
        sheet(0xA1, vec![Some(2), Some(0), Some(3), Some(1), None]), // 80%: 3 right, 1 wrong
        sheet(0xB2, vec![Some(2), Some(0), None, Some(4), None]),    // 80%: 3 right, 1 blank
    ];
    let outcome = grade_batch(&sitting_key(), &SALT, &tied).unwrap();

    assert_eq!(outcome.reports[0].score_bp, outcome.reports[1].score_bp);
    assert_ne!(outcome.leaves[0], outcome.leaves[1]);
    assert!(check_batch_inclusion(
        &public_values(&outcome),
        &published_commitment(),
        &outcome.reports[0],
        &outcome.path(1).unwrap(),
        None,
    )
    .is_err());
}

#[test]
fn a_sitting_nobody_sat_cannot_be_graded() {
    assert_eq!(
        grade_batch(&sitting_key(), &SALT, &[]),
        Err(BatchGradeError::Batch(BatchError::Empty))
    );
}

/// With 10^5 sheets, "scoring rejected inputs" is not a usable diagnosis: the
/// error has to say which candidate.
#[test]
fn a_rejected_sheet_is_reported_by_position() {
    let mut sheets = candidates();
    sheets[1].answers.pop(); // one candidate's sheet has too few answers

    let err = grade_batch(&sitting_key(), &SALT, &sheets).unwrap_err();
    assert_eq!(
        err,
        BatchGradeError::Sheet {
            index: 1,
            error: ScoreError::LengthMismatch { key: 5, sheet: 4 },
        }
    );
}

#[test]
fn a_malformed_key_stops_the_sitting_before_any_grading() {
    let mut broken = sitting_key();
    broken.entries[1].question_id = 1; // duplicate id

    assert_eq!(
        grade_batch(&broken, &SALT, &candidates()),
        Err(BatchGradeError::Key(KeyError::DuplicateQuestionId {
            question_id: 1
        }))
    );
}

#[test]
fn a_candidate_from_another_exam_cannot_be_folded_into_the_sitting() {
    let mut sheets = candidates();
    sheets[2].exam_id = EXAM_ID + 1;

    let err = grade_batch(&sitting_key(), &SALT, &sheets).unwrap_err();
    assert_eq!(
        err,
        BatchGradeError::Sheet {
            index: 2,
            error: ScoreError::ExamIdMismatch {
                key: EXAM_ID,
                sheet: EXAM_ID + 1,
            },
        }
    );
}

/// Sheet order is leaf order: re-grading a sitting with the candidates shuffled
/// produces a different root, so a host that reproduces a manifest must feed the
/// sheets in the published order.
#[test]
fn the_root_is_bound_to_the_order_the_sitting_was_graded_in() {
    let mut reordered = candidates();
    reordered.swap(0, 2);

    assert_ne!(
        sitting().root,
        grade_batch(&sitting_key(), &SALT, &reordered).unwrap().root
    );
}
