use grading_core::score::{validate_key, KeyError, MAX_QUESTIONS};
use grading_core::{
    commit_answer_key, hash_answer_sheet, score, AnswerKey, AnswerSheet, CancelPolicy, KeyEntry,
    ScoreError,
};

const SALT: [u8; 32] = [7u8; 32];

fn entry(id: u32, weight: u32, accepted: &[u8]) -> KeyEntry {
    KeyEntry { question_id: id, weight, accepted: accepted.to_vec(), cancelled: false }
}

fn cancelled_entry(id: u32, weight: u32) -> KeyEntry {
    KeyEntry { question_id: id, weight, accepted: vec![], cancelled: true }
}

fn key(entries: Vec<KeyEntry>, policy: CancelPolicy) -> AnswerKey {
    AnswerKey { exam_id: 42, num_choices: 5, cancel_policy: policy, entries }
}

fn sheet(answers: Vec<Option<u8>>) -> AnswerSheet {
    AnswerSheet { exam_id: 42, student_pseudonym: [1u8; 32], answers }
}

#[test]
fn perfect_score() {
    let k = key(vec![entry(1, 1, &[0]), entry(2, 1, &[3])], CancelPolicy::FullCredit);
    let r = score(&k, &SALT, &sheet(vec![Some(0), Some(3)])).unwrap();
    assert_eq!(r.score_bp, 10_000);
    assert_eq!((r.correct, r.wrong, r.blank, r.cancelled), (2, 0, 0, 0));
}

#[test]
fn weighted_partial_score_uses_floor_division() {
    // 1 of 3 equal-weight questions correct: floor(1 * 10000 / 3) = 3333.
    let k = key(
        vec![entry(1, 1, &[0]), entry(2, 1, &[1]), entry(3, 1, &[2])],
        CancelPolicy::FullCredit,
    );
    let r = score(&k, &SALT, &sheet(vec![Some(0), Some(0), None])).unwrap();
    assert_eq!(r.score_bp, 3_333);
    assert_eq!((r.correct, r.wrong, r.blank), (1, 1, 1));
}

#[test]
fn multiple_accepted_answers_after_appeal() {
    // Appeals board ruled both A and C correct for question 1.
    let k = key(vec![entry(1, 1, &[0, 2]), entry(2, 1, &[1])], CancelPolicy::FullCredit);
    let a = score(&k, &SALT, &sheet(vec![Some(0), Some(1)])).unwrap();
    let c = score(&k, &SALT, &sheet(vec![Some(2), Some(1)])).unwrap();
    assert_eq!(a.score_bp, 10_000);
    assert_eq!(c.score_bp, 10_000);
}

#[test]
fn cancelled_full_credit_pays_everyone() {
    let k = key(vec![entry(1, 1, &[0]), cancelled_entry(2, 1)], CancelPolicy::FullCredit);
    // Student got question 1 wrong; cancelled question still credits its weight.
    let r = score(&k, &SALT, &sheet(vec![Some(4), None])).unwrap();
    assert_eq!(r.score_bp, 5_000);
    assert_eq!(r.cancelled, 1);
}

#[test]
fn cancelled_redistribute_renormalizes() {
    let k = key(vec![entry(1, 1, &[0]), cancelled_entry(2, 1)], CancelPolicy::Redistribute);
    // Only question 1 counts; getting it right is now a full score.
    let r = score(&k, &SALT, &sheet(vec![Some(0), None])).unwrap();
    assert_eq!(r.score_bp, 10_000);
}

#[test]
fn all_cancelled_redistribute_gives_full_credit() {
    let k = key(vec![cancelled_entry(1, 1), cancelled_entry(2, 1)], CancelPolicy::Redistribute);
    let r = score(&k, &SALT, &sheet(vec![None, None])).unwrap();
    assert_eq!(r.score_bp, 10_000);
}

#[test]
fn commitment_binds_to_key_content() {
    let k1 = key(vec![entry(1, 1, &[0])], CancelPolicy::FullCredit);
    let mut k2 = k1.clone();
    k2.entries[0].accepted = vec![1]; // quietly "fix" the answer key
    assert_ne!(commit_answer_key(&k1, &SALT), commit_answer_key(&k2, &SALT));

    let mut k3 = k1.clone();
    k3.entries[0].cancelled = true;
    assert_ne!(commit_answer_key(&k1, &SALT), commit_answer_key(&k3, &SALT));
}

#[test]
fn commitment_is_hiding_under_different_salts() {
    let k = key(vec![entry(1, 1, &[0])], CancelPolicy::FullCredit);
    assert_ne!(commit_answer_key(&k, &[0u8; 32]), commit_answer_key(&k, &[1u8; 32]));
}

#[test]
fn report_is_bound_to_commitment_and_sheet_hash() {
    let k = key(vec![entry(1, 1, &[0])], CancelPolicy::FullCredit);
    let s = sheet(vec![Some(0)]);
    let r = score(&k, &SALT, &s).unwrap();
    assert_eq!(r.key_commitment, commit_answer_key(&k, &SALT));
    assert_eq!(r.sheet_hash, hash_answer_sheet(&s));
}

#[test]
fn blank_and_answered_sheets_hash_differently() {
    let blank = sheet(vec![None, Some(1)]);
    let answered = sheet(vec![Some(0), Some(1)]);
    assert_ne!(hash_answer_sheet(&blank), hash_answer_sheet(&answered));
}

#[test]
fn rejects_exam_id_mismatch() {
    let k = key(vec![entry(1, 1, &[0])], CancelPolicy::FullCredit);
    let mut s = sheet(vec![Some(0)]);
    s.exam_id = 99;
    assert_eq!(
        score(&k, &SALT, &s),
        Err(ScoreError::ExamIdMismatch { key: 42, sheet: 99 })
    );
}

#[test]
fn rejects_length_mismatch() {
    let k = key(vec![entry(1, 1, &[0])], CancelPolicy::FullCredit);
    let s = sheet(vec![Some(0), Some(1)]);
    assert_eq!(score(&k, &SALT, &s), Err(ScoreError::LengthMismatch { key: 1, sheet: 2 }));
}

#[test]
fn rejects_out_of_range_choice() {
    let k = key(vec![entry(1, 1, &[0])], CancelPolicy::FullCredit);
    let s = sheet(vec![Some(5)]); // num_choices = 5, valid are 0..=4
    assert_eq!(
        score(&k, &SALT, &s),
        Err(ScoreError::ChoiceOutOfRange { question_id: 1, choice: 5 })
    );
}

#[test]
fn key_validation_catches_structural_problems() {
    let dup = key(vec![entry(1, 1, &[0]), entry(1, 1, &[1])], CancelPolicy::FullCredit);
    assert_eq!(validate_key(&dup), Err(KeyError::DuplicateQuestionId { question_id: 1 }));

    let empty = key(vec![entry(1, 1, &[])], CancelPolicy::FullCredit);
    assert_eq!(validate_key(&empty), Err(KeyError::NoAcceptedChoice { question_id: 1 }));

    let unsorted = key(vec![entry(1, 1, &[2, 0])], CancelPolicy::FullCredit);
    assert_eq!(validate_key(&unsorted), Err(KeyError::MalformedAccepted { question_id: 1 }));

    let out_of_range = key(vec![entry(1, 1, &[7])], CancelPolicy::FullCredit);
    assert_eq!(validate_key(&out_of_range), Err(KeyError::MalformedAccepted { question_id: 1 }));

    let mut too_many = key(vec![], CancelPolicy::FullCredit);
    too_many.entries = (0..=MAX_QUESTIONS as u32)
        .map(|i| entry(i, 1, &[0]))
        .collect();
    assert_eq!(
        validate_key(&too_many),
        Err(KeyError::TooManyQuestions { count: MAX_QUESTIONS + 1 })
    );
}

#[test]
fn scoring_is_deterministic_across_runs() {
    let k = key(
        vec![entry(1, 3, &[0]), entry(2, 2, &[1, 2]), cancelled_entry(3, 5)],
        CancelPolicy::Redistribute,
    );
    let s = sheet(vec![Some(0), Some(2), Some(4)]);
    let r1 = score(&k, &SALT, &s).unwrap();
    let r2 = score(&k, &SALT, &s).unwrap();
    assert_eq!(r1, r2);
}
