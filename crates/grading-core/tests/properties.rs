//! Property-based tests: invariants that must hold for *every* well-formed
//! exam, not just the hand-picked cases in `scoring.rs`. These are the
//! guarantees the zk proof story leans on — e.g. "any change to the key
//! changes the commitment" is exactly what makes the pre-exam commitment
//! meaningful.

use grading_core::{
    commit_answer_key, hash_answer_sheet, score, AnswerKey, AnswerSheet, CancelPolicy, KeyEntry,
};
use proptest::collection::vec as pvec;
use proptest::prelude::*;

const MAX_Q: usize = 40;

#[derive(Clone, Debug)]
struct Case {
    key: AnswerKey,
    salt: [u8; 32],
    sheet: AnswerSheet,
}

fn arb_entry(num_choices: u8, question_id: u32) -> impl Strategy<Value = KeyEntry> {
    let all_choices: Vec<u8> = (0..num_choices).collect();
    (
        proptest::sample::subsequence(all_choices, 1..num_choices as usize),
        0u32..1_000,
        any::<bool>(),
    )
        .prop_map(move |(accepted, weight, cancelled)| KeyEntry {
            question_id,
            weight,
            accepted, // subsequence of a sorted source stays sorted & unique
            cancelled,
        })
}

fn arb_case() -> impl Strategy<Value = Case> {
    (2u8..=6, 1usize..=MAX_Q).prop_flat_map(|(num_choices, n)| {
        let entries: Vec<_> = (0..n).map(|i| arb_entry(num_choices, i as u32)).collect();
        let policy = prop_oneof![
            Just(CancelPolicy::FullCredit),
            Just(CancelPolicy::Redistribute)
        ];
        (
            entries,
            policy,
            pvec(proptest::option::of(0..num_choices), n),
            any::<[u8; 32]>(),
            any::<u64>(),
            any::<[u8; 32]>(),
        )
            .prop_map(
                move |(entries, cancel_policy, answers, salt, exam_id, pseudonym)| Case {
                    key: AnswerKey {
                        exam_id,
                        num_choices,
                        cancel_policy,
                        entries,
                    },
                    salt,
                    sheet: AnswerSheet {
                        exam_id,
                        student_pseudonym: pseudonym,
                        answers,
                    },
                },
            )
    })
}

proptest! {
    #[test]
    fn score_never_exceeds_full(case in arb_case()) {
        let r = score(&case.key, &case.salt, &case.sheet).unwrap();
        prop_assert!(r.score_bp <= 10_000);
    }

    #[test]
    fn counts_partition_the_questions(case in arb_case()) {
        let r = score(&case.key, &case.salt, &case.sheet).unwrap();
        let total = r.correct + r.wrong + r.blank + r.cancelled;
        prop_assert_eq!(total as usize, case.key.entries.len());
    }

    #[test]
    fn scoring_is_a_pure_function(case in arb_case()) {
        let r1 = score(&case.key, &case.salt, &case.sheet).unwrap();
        let r2 = score(&case.key, &case.salt, &case.sheet).unwrap();
        prop_assert_eq!(r1, r2);
    }

    #[test]
    fn answering_everything_correctly_scores_full(case in arb_case()) {
        let mut sheet = case.sheet.clone();
        sheet.answers = case
            .key
            .entries
            .iter()
            .map(|e| e.accepted.first().copied())
            .collect();
        let r = score(&case.key, &case.salt, &sheet).unwrap();
        prop_assert_eq!(r.score_bp, 10_000);
    }

    #[test]
    fn fixing_one_answer_never_lowers_the_score(case in arb_case(), pick in any::<prop::sample::Index>()) {
        let gradeable: Vec<usize> = case
            .key
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.cancelled)
            .map(|(i, _)| i)
            .collect();
        prop_assume!(!gradeable.is_empty());
        let i = gradeable[pick.index(gradeable.len())];

        let before = score(&case.key, &case.salt, &case.sheet).unwrap();
        let mut improved = case.sheet.clone();
        improved.answers[i] = case.key.entries[i].accepted.first().copied();
        let after = score(&case.key, &case.salt, &improved).unwrap();
        prop_assert!(after.score_bp >= before.score_bp);
    }

    #[test]
    fn commitment_binds_to_weights(case in arb_case(), pick in any::<prop::sample::Index>()) {
        let i = pick.index(case.key.entries.len());
        let mut tampered = case.key.clone();
        tampered.entries[i].weight += 1;
        prop_assert_ne!(
            commit_answer_key(&case.key, &case.salt),
            commit_answer_key(&tampered, &case.salt)
        );
    }

    #[test]
    fn commitment_binds_to_cancellation_flags(case in arb_case(), pick in any::<prop::sample::Index>()) {
        let i = pick.index(case.key.entries.len());
        let mut tampered = case.key.clone();
        tampered.entries[i].cancelled = !tampered.entries[i].cancelled;
        prop_assert_ne!(
            commit_answer_key(&case.key, &case.salt),
            commit_answer_key(&tampered, &case.salt)
        );
    }

    #[test]
    fn commitment_binds_to_accepted_answers(case in arb_case(), pick in any::<prop::sample::Index>()) {
        let i = pick.index(case.key.entries.len());
        let mut tampered = case.key.clone();
        // Replace the accepted set with a different valid one: rotate the
        // first accepted choice to the next unaccepted choice if possible.
        let current = &case.key.entries[i].accepted;
        let replacement: Vec<u8> = (0..case.key.num_choices)
            .filter(|c| !current.contains(c))
            .take(1)
            .collect();
        prop_assume!(!replacement.is_empty());
        tampered.entries[i].accepted = replacement;
        prop_assert_ne!(
            commit_answer_key(&case.key, &case.salt),
            commit_answer_key(&tampered, &case.salt)
        );
    }

    #[test]
    fn different_salts_hide_identical_keys(case in arb_case(), other_salt in any::<[u8; 32]>()) {
        prop_assume!(case.salt != other_salt);
        prop_assert_ne!(
            commit_answer_key(&case.key, &case.salt),
            commit_answer_key(&case.key, &other_salt)
        );
    }

    #[test]
    fn sheet_hash_binds_to_every_answer(case in arb_case(), pick in any::<prop::sample::Index>()) {
        let i = pick.index(case.sheet.answers.len());
        let mut tampered = case.sheet.clone();
        tampered.answers[i] = match tampered.answers[i] {
            None => Some(0),
            Some(_) => None,
        };
        prop_assert_ne!(hash_answer_sheet(&case.sheet), hash_answer_sheet(&tampered));
    }

    #[test]
    fn report_always_carries_matching_bindings(case in arb_case()) {
        let r = score(&case.key, &case.salt, &case.sheet).unwrap();
        prop_assert_eq!(r.key_commitment, commit_answer_key(&case.key, &case.salt));
        prop_assert_eq!(r.sheet_hash, hash_answer_sheet(&case.sheet));
        prop_assert_eq!(r.exam_id, case.key.exam_id);
    }
}
