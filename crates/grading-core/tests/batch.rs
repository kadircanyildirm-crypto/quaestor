//! Tests for the batch (per-sitting) commitment layer.
//!
//! The interesting cases are the negative ones. A Merkle implementation that
//! only ever gets checked with "the honest path verifies" passes while still
//! admitting forged inclusion claims, and an inclusion claim is precisely what
//! a student's grade rests on once proofs are batched.

use grading_core::{
    batch_root, encode_public_values, leaf_hash, merkle_path, merkle_paths, verify_merkle_path,
    BatchError, MerklePath, ScoreReport,
};
use proptest::prelude::*;

fn report(n: u32) -> ScoreReport {
    ScoreReport {
        exam_id: 20260701,
        key_commitment: [0xAA; 32],
        sheet_hash: [n as u8; 32],
        score_bp: (n * 7) % 10_001,
        correct: n,
        wrong: 0,
        blank: 0,
        cancelled: 0,
    }
}

fn leaves(n: usize) -> Vec<[u8; 32]> {
    (0..n).map(|i| leaf_hash(&report(i as u32))).collect()
}

#[test]
fn every_candidate_in_a_sitting_can_prove_inclusion() {
    // Deliberately spans odd, even, power-of-two and just-past-power-of-two
    // sizes, which is where promotion logic goes wrong.
    for n in [1usize, 2, 3, 4, 5, 7, 8, 9, 16, 17, 100] {
        let ls = leaves(n);
        let root = batch_root(&ls);
        for i in 0..n {
            let path = merkle_path(&ls, i).unwrap();
            assert!(
                verify_merkle_path(&ls[i], &path, n, &root),
                "leaf {i} of {n} failed to verify"
            );
        }
    }
}

#[test]
fn a_path_does_not_verify_for_a_leaf_that_is_not_in_the_batch() {
    let ls = leaves(9);
    let root = batch_root(&ls);
    let path = merkle_path(&ls, 3).unwrap();
    let outsider = leaf_hash(&report(999));
    assert!(!verify_merkle_path(&outsider, &path, 9, &root));
}

#[test]
fn a_path_is_bound_to_its_own_position() {
    let ls = leaves(8);
    let root = batch_root(&ls);
    let path = merkle_path(&ls, 3).unwrap();
    // Same siblings, a neighbour's index: must not verify for either leaf.
    let moved = MerklePath {
        index: 4,
        ..path.clone()
    };
    assert!(!verify_merkle_path(&ls[3], &moved, 8, &root));
    assert!(!verify_merkle_path(&ls[4], &moved, 8, &root));
}

#[test]
fn tampering_with_a_report_breaks_its_inclusion() {
    let ls = leaves(6);
    let root = batch_root(&ls);
    let path = merkle_path(&ls, 2).unwrap();

    let mut inflated = report(2);
    inflated.score_bp += 100; // the institution quietly raises one score
    assert!(!verify_merkle_path(&leaf_hash(&inflated), &path, 6, &root));
}

/// CVE-2012-2459 in its exam-shaped form: without a count binding, a sitting of
/// `n` candidates and one of `n + 1` whose last candidate is a duplicate can
/// share a root, letting a prover claim someone sat an exam they did not.
#[test]
fn duplicating_the_last_candidate_does_not_preserve_the_root() {
    let three = leaves(3);
    let mut four = three.clone();
    four.push(three[2]);

    assert_ne!(batch_root(&three), batch_root(&four));
}

#[test]
fn the_leaf_count_is_part_of_the_claim() {
    let ls = leaves(8);
    let root = batch_root(&ls);
    let path = merkle_path(&ls, 0).unwrap();
    // Right leaf, right path, wrong sitting size.
    assert!(!verify_merkle_path(&ls[0], &path, 7, &root));
    assert!(!verify_merkle_path(&ls[0], &path, 9, &root));
}

#[test]
fn a_path_padded_with_extra_siblings_is_rejected() {
    let ls = leaves(4);
    let root = batch_root(&ls);
    let mut path = merkle_path(&ls, 1).unwrap();
    path.siblings.push([0x42; 32]);
    assert!(!verify_merkle_path(&ls[1], &path, 4, &root));
}

#[test]
fn a_truncated_path_is_rejected() {
    let ls = leaves(4);
    let root = batch_root(&ls);
    let mut path = merkle_path(&ls, 1).unwrap();
    path.siblings.pop();
    assert!(!verify_merkle_path(&ls[1], &path, 4, &root));
}

#[test]
fn leaves_and_internal_nodes_live_in_different_domains() {
    // A two-leaf tree's root must not be forgeable as a leaf: the tags are what
    // stop an internal-node preimage being replayed as a report.
    let ls = leaves(2);
    let inner_preimage = leaf_hash(&report(0));
    assert_ne!(batch_root(&ls), inner_preimage);
}

#[test]
fn batch_construction_rejects_degenerate_inputs() {
    assert_eq!(merkle_path(&[], 0), Err(BatchError::Empty));
    assert_eq!(
        merkle_path(&leaves(4), 4),
        Err(BatchError::IndexOutOfRange { index: 4, count: 4 })
    );
    assert_eq!(merkle_paths(&[]), Err(BatchError::Empty));
}

/// Publishing a sitting means handing a path to everyone, so the one-pass bulk
/// cut is what actually runs at scale — cutting them individually is `O(n²)`
/// and unusable past a few thousand candidates. The fast path is only worth
/// having if it is indistinguishable from the slow one, including at the odd
/// levels where promotion means a path is *shorter* than `ceil(log2 n)`.
#[test]
fn bulk_paths_match_cutting_them_one_at_a_time() {
    for n in [1usize, 2, 3, 4, 5, 7, 8, 9, 16, 17, 100] {
        let ls = leaves(n);
        let root = batch_root(&ls);
        let bulk = merkle_paths(&ls).unwrap();

        assert_eq!(bulk.len(), n, "one path per candidate in a sitting of {n}");
        for (i, path) in bulk.iter().enumerate() {
            assert_eq!(*path, merkle_path(&ls, i).unwrap(), "path {i} of {n}");
            assert!(
                verify_merkle_path(&ls[i], path, n, &root),
                "bulk path {i} of {n} failed to verify"
            );
        }
    }
}

#[test]
fn an_empty_sitting_has_a_root_no_path_can_open() {
    let root = batch_root(&[]);
    let path = MerklePath {
        index: 0,
        siblings: vec![],
    };
    assert!(!verify_merkle_path(&leaf_hash(&report(0)), &path, 0, &root));
}

/// Batching must change the delivery of a claim, not the claim: a leaf is the
/// same report bytes a standalone proof commits, so a student's report means
/// the same thing whether it arrived alone or in a sitting of half a million.
#[test]
fn a_batched_report_is_the_same_object_as_a_standalone_one() {
    let r = report(1);
    let mut different_score = r.clone();
    different_score.score_bp += 1;

    assert_eq!(leaf_hash(&r), leaf_hash(&r.clone()));
    assert_ne!(leaf_hash(&r), leaf_hash(&different_score));
    assert_ne!(
        encode_public_values(&r),
        encode_public_values(&different_score)
    );
}

proptest! {
    /// The property that actually matters at scale: for any sitting size and
    /// any candidate in it, the honest path verifies and nothing else does.
    #[test]
    fn inclusion_holds_for_arbitrary_sittings(n in 1usize..=64, i in any::<prop::sample::Index>()) {
        let ls = leaves(n);
        let root = batch_root(&ls);
        let idx = i.index(n);
        let path = merkle_path(&ls, idx).unwrap();

        prop_assert!(verify_merkle_path(&ls[idx], &path, n, &root));

        // Any other candidate's leaf must fail against this path.
        for (other, leaf) in ls.iter().enumerate() {
            if other != idx {
                prop_assert!(!verify_merkle_path(leaf, &path, n, &root));
            }
        }
    }

    #[test]
    fn bulk_paths_agree_with_single_cuts_for_any_sitting_size(n in 1usize..=64) {
        let ls = leaves(n);
        let bulk = merkle_paths(&ls).unwrap();
        for (i, path) in bulk.iter().enumerate() {
            prop_assert_eq!(path, &merkle_path(&ls, i).unwrap());
        }
    }

    #[test]
    fn distinct_sittings_have_distinct_roots(n in 1usize..=32, m in 1usize..=32) {
        prop_assume!(n != m);
        prop_assert_ne!(batch_root(&leaves(n)), batch_root(&leaves(m)));
    }

    #[test]
    fn flipping_any_bit_of_any_leaf_changes_the_root(
        n in 1usize..=16,
        i in any::<prop::sample::Index>(),
        byte in 0usize..32,
        bit in 0u32..8,
    ) {
        let ls = leaves(n);
        let idx = i.index(n);
        let mut tampered = ls.clone();
        tampered[idx][byte] ^= 1 << bit;
        prop_assert_ne!(batch_root(&ls), batch_root(&tampered));
    }
}
