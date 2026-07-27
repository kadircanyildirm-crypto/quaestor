//! Tests for the public-values ABI and the claim checks a verifier runs on it.
//!
//! Two distinct jobs here. The layout tests pin bytes: published proofs are
//! parsed by verifiers built from *later* commits, so an accidental field
//! reshuffle would silently reinterpret every proof already in the wild. The
//! verifier tests pin behaviour: rejecting a proof that is cryptographically
//! valid but about the wrong key or the wrong sheet is the entire product.

use grading_core::{
    check_batch_public_values, check_public_values, commit_answer_key, decode_batch_public_values,
    decode_public_values, encode_answer_sheet, encode_batch_public_values, encode_public_values,
    hash_answer_sheet, score, AnswerKey, AnswerSheet, BatchPublicValues, CancelPolicy, KeyEntry,
    PublicValuesError, ScoreReport, VerifyError, BATCH_PUBLIC_VALUES_LEN, PUBLIC_VALUES_LEN,
};
use proptest::prelude::*;

fn demo_key() -> AnswerKey {
    AnswerKey {
        exam_id: 20260701,
        num_choices: 5,
        cancel_policy: CancelPolicy::FullCredit,
        entries: vec![
            KeyEntry {
                question_id: 1,
                weight: 20,
                accepted: vec![2],
                cancelled: false,
            },
            KeyEntry {
                question_id: 2,
                weight: 20,
                accepted: vec![0],
                cancelled: false,
            },
        ],
    }
}

fn demo_sheet(answers: Vec<Option<u8>>) -> AnswerSheet {
    AnswerSheet {
        exam_id: 20260701,
        student_pseudonym: [7u8; 32],
        answers,
    }
}

const SALT: [u8; 32] = [1u8; 32];

fn graded() -> ScoreReport {
    score(&demo_key(), &SALT, &demo_sheet(vec![Some(2), Some(0)])).unwrap()
}

#[test]
fn layout_matches_the_documented_offsets() {
    let report = ScoreReport {
        exam_id: 0x0102_0304_0506_0708,
        key_commitment: [0xAA; 32],
        sheet_hash: [0xBB; 32],
        score_bp: 8_250,
        correct: 3,
        wrong: 1,
        blank: 0,
        cancelled: 1,
    };
    let b = encode_public_values(&report);

    assert_eq!(b.len(), PUBLIC_VALUES_LEN);
    assert_eq!(&b[0..32], &[0xAA; 32]);
    assert_eq!(&b[32..64], &[0xBB; 32]);
    assert_eq!(&b[64..72], &0x0102_0304_0506_0708u64.to_le_bytes());
    assert_eq!(&b[72..76], &8_250u32.to_le_bytes());
    assert_eq!(&b[76..80], &3u32.to_le_bytes());
    assert_eq!(&b[80..84], &1u32.to_le_bytes());
    assert_eq!(&b[84..88], &0u32.to_le_bytes());
    assert_eq!(&b[88..92], &1u32.to_le_bytes());
}

#[test]
fn decoding_rejects_any_length_but_92() {
    for len in [0usize, 91, 93, 200] {
        assert_eq!(
            decode_public_values(&vec![0u8; len]),
            Err(PublicValuesError::WrongLength {
                found: len,
                expected: PUBLIC_VALUES_LEN,
            })
        );
    }
}

#[test]
fn batch_layout_matches_the_documented_offsets() {
    let pv = BatchPublicValues {
        key_commitment: [0xAA; 32],
        batch_root: [0xCC; 32],
        exam_id: 0x0102_0304_0506_0708,
        leaf_count: 250_000,
    };
    let b = encode_batch_public_values(&pv);

    assert_eq!(b.len(), BATCH_PUBLIC_VALUES_LEN);
    assert_eq!(&b[0..32], &[0xAA; 32]);
    assert_eq!(&b[32..64], &[0xCC; 32]);
    assert_eq!(&b[64..72], &0x0102_0304_0506_0708u64.to_le_bytes());
    assert_eq!(&b[72..76], &250_000u32.to_le_bytes());
}

#[test]
fn batch_decoding_rejects_any_length_but_76() {
    for len in [0usize, 75, 77, 92, 200] {
        assert_eq!(
            decode_batch_public_values(&vec![0u8; len]),
            Err(PublicValuesError::WrongLength {
                found: len,
                expected: BATCH_PUBLIC_VALUES_LEN,
            })
        );
    }
}

/// The layouts share offset 0..32 (the key commitment) and then diverge: at
/// offset 32 one holds a sheet hash and the other a Merkle root. A decoder that
/// accepted the wrong width would read one as the other and report a sitting's
/// root as some student's answers. Length is what keeps them apart, so the fact
/// that the two widths differ is itself a tested property.
#[test]
fn the_two_layouts_cannot_be_parsed_as_each_other() {
    assert_ne!(PUBLIC_VALUES_LEN, BATCH_PUBLIC_VALUES_LEN);

    let single = encode_public_values(&graded());
    let batch = encode_batch_public_values(&BatchPublicValues {
        key_commitment: [0xAA; 32],
        batch_root: [0xCC; 32],
        exam_id: 1,
        leaf_count: 3,
    });

    assert!(decode_batch_public_values(&single).is_err());
    assert!(decode_public_values(&batch).is_err());
}

#[test]
fn batch_verifier_rejects_a_sitting_proven_under_a_different_key() {
    let bytes = encode_batch_public_values(&BatchPublicValues {
        key_commitment: [0xAA; 32],
        batch_root: [0xCC; 32],
        exam_id: 20260701,
        leaf_count: 3,
    });
    let published = [0xAB; 32];

    let err = check_batch_public_values(&bytes, &published).unwrap_err();
    assert_eq!(
        err,
        VerifyError::CommitmentMismatch {
            expected: published,
            found: [0xAA; 32],
        }
    );
    assert!(check_batch_public_values(&bytes, &[0xAA; 32]).is_ok());
}

#[test]
fn verifier_accepts_a_proof_about_the_committed_key_and_the_students_sheet() {
    let report = graded();
    let bytes = encode_public_values(&report);
    let commitment = commit_answer_key(&demo_key(), &SALT);
    let sheet_hash = hash_answer_sheet(&demo_sheet(vec![Some(2), Some(0)]));

    let checked = check_public_values(&bytes, &commitment, Some(&sheet_hash)).unwrap();
    assert_eq!(checked, report);
    assert_eq!(checked.score_bp, 10_000);
}

#[test]
fn verifier_rejects_a_proof_produced_under_a_different_key() {
    // The tamper case: the institution grades against a key it never committed
    // to, then publishes the proof. The proof itself is perfectly valid.
    let mut swapped = demo_key();
    swapped.entries[0].accepted = vec![4];
    let report = score(&swapped, &SALT, &demo_sheet(vec![Some(2), Some(0)])).unwrap();

    let published = commit_answer_key(&demo_key(), &SALT);
    let err = check_public_values(&encode_public_values(&report), &published, None).unwrap_err();
    assert!(matches!(err, VerifyError::CommitmentMismatch { .. }));
}

#[test]
fn verifier_rejects_a_proof_about_someone_elses_sheet() {
    let report = graded();
    let commitment = commit_answer_key(&demo_key(), &SALT);
    let other_sheet = hash_answer_sheet(&demo_sheet(vec![Some(1), Some(1)]));

    let err = check_public_values(
        &encode_public_values(&report),
        &commitment,
        Some(&other_sheet),
    )
    .unwrap_err();
    assert!(matches!(err, VerifyError::SheetHashMismatch { .. }));
}

#[test]
fn verifier_rejects_an_impossible_score() {
    let mut report = graded();
    report.score_bp = 10_001;
    let commitment = commit_answer_key(&demo_key(), &SALT);
    let err = check_public_values(&encode_public_values(&report), &commitment, None).unwrap_err();
    assert_eq!(err, VerifyError::ScoreOutOfRange { score_bp: 10_001 });
}

/// The one place the sheet encoding is not injective, pinned deliberately.
///
/// `Some(0xFF)` and `None` share an encoding because `0xFF` is the blank
/// marker. That is safe only because no exam can accept choice `0xFF`, so the
/// ambiguous sheet is ungradeable and never reaches a hash binding. If a future
/// change makes it gradeable, this test fails and the layout must gain a tag.
#[test]
fn sheet_sentinel_collision_is_unreachable_through_scoring() {
    let ambiguous = demo_sheet(vec![Some(0xFF), Some(0)]);
    let blank = demo_sheet(vec![None, Some(0)]);
    assert_eq!(
        encode_answer_sheet(&ambiguous),
        encode_answer_sheet(&blank),
        "0xFF is the blank marker, so these must collide"
    );
    assert!(
        score(&demo_key(), &SALT, &ambiguous).is_err(),
        "the ambiguous sheet must be ungradeable, which is what makes the collision harmless"
    );
}

fn arb_report() -> impl Strategy<Value = ScoreReport> {
    (
        any::<u64>(),
        any::<[u8; 32]>(),
        any::<[u8; 32]>(),
        0u32..=10_000,
        any::<u32>(),
        any::<u32>(),
        any::<u32>(),
        any::<u32>(),
    )
        .prop_map(
            |(exam_id, key_commitment, sheet_hash, score_bp, correct, wrong, blank, cancelled)| {
                ScoreReport {
                    exam_id,
                    key_commitment,
                    sheet_hash,
                    score_bp,
                    correct,
                    wrong,
                    blank,
                    cancelled,
                }
            },
        )
}

fn arb_batch_public_values() -> impl Strategy<Value = BatchPublicValues> {
    (
        any::<[u8; 32]>(),
        any::<[u8; 32]>(),
        any::<u64>(),
        any::<u32>(),
    )
        .prop_map(
            |(key_commitment, batch_root, exam_id, leaf_count)| BatchPublicValues {
                key_commitment,
                batch_root,
                exam_id,
                leaf_count,
            },
        )
}

proptest! {
    #[test]
    fn public_values_roundtrip(report in arb_report()) {
        let bytes = encode_public_values(&report);
        prop_assert_eq!(decode_public_values(&bytes).unwrap(), report);
    }

    #[test]
    fn batch_public_values_roundtrip(pv in arb_batch_public_values()) {
        let bytes = encode_batch_public_values(&pv);
        prop_assert_eq!(decode_batch_public_values(&bytes).unwrap(), pv);
    }

    #[test]
    fn every_bit_of_the_batch_layout_is_load_bearing(
        pv in arb_batch_public_values(),
        byte in 0usize..BATCH_PUBLIC_VALUES_LEN,
        bit in 0u32..8,
    ) {
        let original = encode_batch_public_values(&pv);
        let mut mutated = original;
        mutated[byte] ^= 1 << bit;
        prop_assert_ne!(
            decode_batch_public_values(&original).unwrap(),
            decode_batch_public_values(&mutated).unwrap()
        );
    }

    /// Every byte carries meaning: flipping any single bit must change what a
    /// verifier sees, or that byte is dead space a prover could vary freely.
    #[test]
    fn every_bit_of_the_layout_is_load_bearing(
        report in arb_report(),
        byte in 0usize..PUBLIC_VALUES_LEN,
        bit in 0u32..8,
    ) {
        let original = encode_public_values(&report);
        let mut mutated = original;
        mutated[byte] ^= 1 << bit;
        prop_assert_ne!(
            decode_public_values(&original).unwrap(),
            decode_public_values(&mutated).unwrap()
        );
    }
}
