//! Batch commitments: one proof per exam *sitting* instead of one per student.
//!
//! Per-sheet proving is what makes the v0 demo legible, and it is also what
//! makes it undeployable: a national sitting is 10^5–10^6 sheets, and a proof
//! each is not a cost curve anyone will pay. Batching moves the cost to
//! `O(1) proofs` per sitting: the guest grades every sheet in one execution and
//! commits a single Merkle root over the resulting reports. A student verifies
//! the one proof, then checks a `log2(n)`-sized path showing *their* report is
//! in the tree — around 20 hashes for a million candidates, cheap enough to run
//! in a browser tab.
//!
//! ## Why the tree is built the way it is
//!
//! Two standard Merkle footguns are closed here rather than documented away:
//!
//! - **Leaf/node confusion.** Leaves and internal nodes are hashed under
//!   distinct one-byte tags, so no leaf preimage can ever be reinterpreted as
//!   an internal node (the second-preimage attack that plain `H(l ‖ r)` trees
//!   are open to).
//! - **Odd-level duplication.** An odd node is promoted unchanged rather than
//!   paired with a copy of itself, and the final root additionally binds the
//!   leaf *count*. Without a count binding, an `n`-leaf tree and an
//!   `n+1`-leaf tree whose last leaf is duplicated can share a root — the
//!   Bitcoin CVE-2012-2459 shape — which would let a prover claim a candidate
//!   was in a sitting they never sat.

use alloc::vec::Vec;
use core::fmt;

use sha2::{Digest, Sha256};

use crate::commit::{commit_answer_key, Salt};
use crate::model::{AnswerKey, AnswerSheet, ScoreReport};
use crate::public_values::{
    check_batch_public_values, encode_public_values, BatchPublicValues, VerifyError,
};
use crate::score::{score_validated, validate_key, KeyError, ScoreError, FULL_SCORE_BP};

/// Upper bound on sheets per batch. Exists so the level arithmetic below can
/// never overflow a `u32` index, and sits far above any real sitting.
pub const MAX_BATCH: usize = 1 << 20;

const LEAF_TAG: u8 = 0x00;
const NODE_TAG: u8 = 0x01;
const ROOT_DOMAIN: &[u8] = b"quaestor/batch-root";

/// Hash of one graded report as it appears in the tree. Defined over the same
/// canonical public-values bytes a single-sheet proof commits, so a batched
/// report and a standalone one are the same object.
pub fn leaf_hash(report: &ScoreReport) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update([LEAF_TAG]);
    h.update(encode_public_values(report));
    h.finalize().into()
}

fn node_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update([NODE_TAG]);
    h.update(left);
    h.update(right);
    h.finalize().into()
}

/// Fold the leaf count into the tree root. This is what makes the root a
/// commitment to a *specific sitting size* and closes the duplicate-leaf
/// ambiguity described in the module docs.
fn bind_count(inner: &[u8; 32], leaf_count: usize) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(ROOT_DOMAIN);
    h.update((leaf_count as u32).to_le_bytes());
    h.update(inner);
    h.finalize().into()
}

/// Width of each level above the leaves, given a leaf count.
fn next_width(width: usize) -> usize {
    width.div_ceil(2)
}

/// Whether the node at `idx` in a level of `width` has no sibling — the lone
/// tail of an odd level, which [`batch_root`] promotes unchanged.
///
/// Path construction and path verification must agree on this exactly, so they
/// share the predicate rather than restating it; a divergence here would make
/// honest paths for odd-sized sittings unverifiable.
fn is_promoted(idx: usize, width: usize) -> bool {
    !width.is_multiple_of(2) && idx == width - 1
}

/// Merkle root over the batch. `[0u8; 32]`-seeded for an empty batch, which the
/// count binding then makes distinct from every non-empty root.
pub fn batch_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    let mut level: Vec<[u8; 32]> = leaves.to_vec();
    if level.is_empty() {
        return bind_count(&[0u8; 32], 0);
    }
    while level.len() > 1 {
        let mut next = Vec::with_capacity(next_width(level.len()));
        for pair in level.chunks(2) {
            next.push(match pair {
                [l, r] => node_hash(l, r),
                // Lone tail node is promoted, never self-paired.
                [l] => *l,
                _ => unreachable!("chunks(2) yields 1 or 2 elements"),
            });
        }
        level = next;
    }
    bind_count(&level[0], leaves.len())
}

/// One student's inclusion path. Deliberately does not carry the leaf: a path
/// is only meaningful next to the report it is claimed to be for, and keeping
/// them separate stops a verifier from checking a path against itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MerklePath {
    /// Position of the student's report among the batch's leaves.
    pub index: u32,
    /// Sibling hashes bottom-up. Levels where the node was promoted contribute
    /// nothing, so the length is not always `ceil(log2(n))`; the verifier
    /// derives which levels those are from `index` and `leaf_count`.
    pub siblings: Vec<[u8; 32]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchError {
    Empty,
    TooLarge { count: usize, max: usize },
    IndexOutOfRange { index: usize, count: usize },
}

/// Build the inclusion path for one leaf.
pub fn merkle_path(leaves: &[[u8; 32]], index: usize) -> Result<MerklePath, BatchError> {
    if leaves.is_empty() {
        return Err(BatchError::Empty);
    }
    if leaves.len() > MAX_BATCH {
        return Err(BatchError::TooLarge {
            count: leaves.len(),
            max: MAX_BATCH,
        });
    }
    if index >= leaves.len() {
        return Err(BatchError::IndexOutOfRange {
            index,
            count: leaves.len(),
        });
    }

    let mut siblings = Vec::new();
    let mut level: Vec<[u8; 32]> = leaves.to_vec();
    let mut idx = index;
    while level.len() > 1 {
        // A promoted node has no sibling to record; the verifier recomputes the
        // same condition from `index` and `leaf_count` alone.
        if !is_promoted(idx, level.len()) {
            siblings.push(level[idx ^ 1]);
        }
        let mut next = Vec::with_capacity(next_width(level.len()));
        for pair in level.chunks(2) {
            next.push(match pair {
                [l, r] => node_hash(l, r),
                [l] => *l,
                _ => unreachable!("chunks(2) yields 1 or 2 elements"),
            });
        }
        level = next;
        idx /= 2;
    }

    Ok(MerklePath {
        index: index as u32,
        siblings,
    })
}

/// Build the inclusion path for *every* leaf, in one pass.
///
/// [`merkle_path`] rebuilds the whole tree per call, which is the right shape
/// for one lookup and the wrong one for a sitting: handing paths to `n`
/// candidates that way is `n` tree builds, i.e. `O(n²)` hashing on the exact
/// code path batching exists to make cheap. Measured on the reference machine,
/// per-candidate cutting takes 27 s at 16k leaves and extrapolates to roughly a
/// day at the 10^6 this crate is aimed at; one pass that keeps its levels does
/// the same work in `O(n log n)`.
///
/// Costs `~2n` hashes of memory for the retained levels. That is a host-side
/// price only — the guest commits the root and never cuts a path.
pub fn merkle_paths(leaves: &[[u8; 32]]) -> Result<Vec<MerklePath>, BatchError> {
    if leaves.is_empty() {
        return Err(BatchError::Empty);
    }
    if leaves.len() > MAX_BATCH {
        return Err(BatchError::TooLarge {
            count: leaves.len(),
            max: MAX_BATCH,
        });
    }

    // Every level the single-path walk would have visited, kept instead of
    // recomputed. The final one-node level is not retained: it contributes no
    // sibling, exactly as the `while level.len() > 1` walk never reaches it.
    let mut levels: Vec<Vec<[u8; 32]>> = Vec::new();
    let mut level: Vec<[u8; 32]> = leaves.to_vec();
    while level.len() > 1 {
        let mut next = Vec::with_capacity(next_width(level.len()));
        for pair in level.chunks(2) {
            next.push(match pair {
                [l, r] => node_hash(l, r),
                [l] => *l,
                _ => unreachable!("chunks(2) yields 1 or 2 elements"),
            });
        }
        levels.push(level);
        level = next;
    }

    Ok((0..leaves.len())
        .map(|index| {
            let mut idx = index;
            let mut siblings = Vec::new();
            for level in &levels {
                // Same predicate, same order as `merkle_path`; the two are
                // pinned against each other by `bulk_paths_match_cutting_them_one_at_a_time`.
                if !is_promoted(idx, level.len()) {
                    siblings.push(level[idx ^ 1]);
                }
                idx /= 2;
            }
            MerklePath {
                index: index as u32,
                siblings,
            }
        })
        .collect())
}

/// Check that `leaf` sits at `path.index` of a `leaf_count`-leaf batch whose
/// root is `root`.
///
/// This is the whole student-side check for a batched sitting, and it needs no
/// proof system: `no_std`, allocation-free, a few dozen SHA-256 compressions.
pub fn verify_merkle_path(
    leaf: &[u8; 32],
    path: &MerklePath,
    leaf_count: usize,
    root: &[u8; 32],
) -> bool {
    if leaf_count == 0 || leaf_count > MAX_BATCH || path.index as usize >= leaf_count {
        return false;
    }

    let mut hash = *leaf;
    let mut idx = path.index as usize;
    let mut width = leaf_count;
    let mut siblings = path.siblings.iter();

    while width > 1 {
        if !is_promoted(idx, width) {
            let Some(sibling) = siblings.next() else {
                return false;
            };
            hash = if idx.is_multiple_of(2) {
                node_hash(&hash, sibling)
            } else {
                node_hash(sibling, &hash)
            };
        }
        idx /= 2;
        width = next_width(width);
    }

    // A path carrying more siblings than the tree shape calls for is rejected
    // rather than ignored: unconsumed data is free room for a second valid
    // encoding of the same claim.
    siblings.next().is_none() && bind_count(&hash, leaf_count) == *root
}

/// Everything one execution of the batch guest produces.
///
/// The guest publishes only [`BatchOutcome::public_values`]; the rest is what
/// the institution hands each candidate afterwards (their report, and the path
/// that ties it to the proven root). None of it is secret — the point is that
/// none of it has to be *trusted* either.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchOutcome {
    /// One report per input sheet, in input order. Index `i` is leaf `i`.
    pub reports: Vec<ScoreReport>,
    /// The leaf hashes, kept so the host can cut inclusion paths without
    /// re-grading the sitting.
    pub leaves: Vec<[u8; 32]>,
    pub root: [u8; 32],
    pub key_commitment: [u8; 32],
    pub exam_id: u64,
}

impl BatchOutcome {
    /// The exact bytes the guest commits.
    ///
    /// Exists so the guest and the host build them from one function: the host
    /// re-grades the sitting locally to publish the manifest, and if its idea
    /// of the committed bytes could drift from the guest's, every downstream
    /// inclusion check would be verifying the wrong root.
    pub fn public_values(&self) -> BatchPublicValues {
        BatchPublicValues {
            key_commitment: self.key_commitment,
            batch_root: self.root,
            exam_id: self.exam_id,
            leaf_count: self.reports.len() as u32,
        }
    }

    /// Inclusion path for one candidate, by their position in the sitting.
    ///
    /// For a single lookup. To hand a path to *everyone* — which is what
    /// publishing a sitting means — use [`BatchOutcome::paths`]: calling this
    /// in a loop rebuilds the tree once per candidate.
    pub fn path(&self, index: usize) -> Result<MerklePath, BatchError> {
        merkle_path(&self.leaves, index)
    }

    /// Inclusion paths for the whole sitting, in leaf order, in one pass.
    pub fn paths(&self) -> Result<Vec<MerklePath>, BatchError> {
        merkle_paths(&self.leaves)
    }
}

/// Why a sitting could not be graded as a batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BatchGradeError {
    /// The sitting itself is not a batch this construction accepts.
    Batch(BatchError),
    /// The answer key is malformed — checked once for the whole sitting.
    Key(KeyError),
    /// One candidate's sheet was rejected. Carries the position, because with
    /// 10^5 sheets "scoring rejected inputs" is not a usable diagnosis.
    Sheet { index: u32, error: ScoreError },
    /// Two sheets carry the same pseudonym. A candidate sits once; a second
    /// sheet under the same pseudonym would give the institution two proven
    /// results for one person to choose between, and the candidate's own check
    /// (which finds their row by sheet hash) would never see the other one.
    DuplicateCandidate { first: u32, second: u32 },
}

impl fmt::Display for BatchGradeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BatchGradeError::Batch(BatchError::Empty) => {
                f.write_str("the sitting has no candidates")
            }
            BatchGradeError::Batch(BatchError::TooLarge { count, max }) => write!(
                f,
                "a sitting of {count} exceeds the {max}-candidate batch limit: \
                 shard it and aggregate the shard proofs"
            ),
            BatchGradeError::Batch(e) => write!(f, "{e:?}"),
            BatchGradeError::Key(e) => write!(f, "the answer key is malformed: {e:?}"),
            BatchGradeError::Sheet { index, error } => {
                write!(f, "candidate at position {index}: {error:?}")
            }
            BatchGradeError::DuplicateCandidate { first, second } => write!(
                f,
                "candidates at positions {first} and {second} share a pseudonym: \
                 each candidate can appear only once in a sitting"
            ),
        }
    }
}

/// Grade a whole sitting and commit to it with one Merkle root.
///
/// This is the batch guest's entire computation. It is not a loop over
/// [`crate::score`]: key validation and the key commitment are hoisted out, so
/// a sitting costs `O(sheets + key)` instead of `O(sheets * key)` — inside a
/// zkVM, where every SHA-256 compression is proven, that is the difference
/// between batching paying off and not. The per-sheet result is identical
/// either way, which `batch_reports_match_scoring_each_sheet_alone` pins.
///
/// Sheet order is the caller's, and it *is* the leaf order: candidate `i`'s
/// path is only meaningful against the same ordering the root was built from,
/// so a host that re-grades a sitting must feed the sheets in the same order.
pub fn grade_batch(
    key: &AnswerKey,
    salt: &Salt,
    sheets: &[AnswerSheet],
) -> Result<BatchOutcome, BatchGradeError> {
    if sheets.is_empty() {
        return Err(BatchGradeError::Batch(BatchError::Empty));
    }
    if sheets.len() > MAX_BATCH {
        return Err(BatchGradeError::Batch(BatchError::TooLarge {
            count: sheets.len(),
            max: MAX_BATCH,
        }));
    }
    validate_key(key).map_err(BatchGradeError::Key)?;
    reject_duplicate_candidates(sheets)?;
    let key_commitment = commit_answer_key(key, salt);

    let mut reports = Vec::with_capacity(sheets.len());
    let mut leaves = Vec::with_capacity(sheets.len());
    for (index, sheet) in sheets.iter().enumerate() {
        let report = score_validated(key, &key_commitment, sheet).map_err(|error| {
            BatchGradeError::Sheet {
                index: index as u32,
                error,
            }
        })?;
        leaves.push(leaf_hash(&report));
        reports.push(report);
    }

    let root = batch_root(&leaves);
    Ok(BatchOutcome {
        reports,
        leaves,
        root,
        key_commitment,
        exam_id: key.exam_id,
    })
}

/// Every pseudonym in a sitting must be distinct.
///
/// Runs inside the guest, so a valid batch proof attests it: no sitting with a
/// second sheet under a real candidate's pseudonym can be proven at all. This
/// cannot be left to the candidate. Their check finds their row by sheet hash,
/// so it confirms the row holding their real answers and never sees a second
/// row under their name, and an auditor cannot see names at all.
///
/// Sorting a copy costs O(n log n) guest cycles; a quadratic scan would be
/// cheaper for a classroom and ruinous for a national sitting.
fn reject_duplicate_candidates(sheets: &[AnswerSheet]) -> Result<(), BatchGradeError> {
    let mut ids: Vec<(&[u8; 32], u32)> = sheets
        .iter()
        .enumerate()
        .map(|(i, s)| (&s.student_pseudonym, i as u32))
        .collect();
    ids.sort_unstable();
    match ids.windows(2).find(|w| w[0].0 == w[1].0) {
        Some(w) => Err(BatchGradeError::DuplicateCandidate {
            first: w[0].1,
            second: w[1].1,
        }),
        None => Ok(()),
    }
}

/// The whole student-side check for a batched sitting, in one call.
///
/// A batch proof on its own says "*some* sitting was graded under *some* key".
/// These are the steps that turn it into "*my* score, from the key committed
/// before the exam": the sitting is under the published commitment, the report
/// is for that sitting, it is about the answers I actually submitted, and it
/// is genuinely one of the leaves the proof committed to — not a number typed
/// into a results page next to a proof of something else.
///
/// Needs no proof system and no allocation beyond the path: `no_std`, a few
/// dozen SHA-256 compressions, and therefore runnable in a browser tab next to
/// the proof verification itself.
///
/// `expected_sheet_hash` is optional for the same reason it is in
/// [`crate::check_public_values`]: an auditor may check that a published
/// results list is exactly the proven sitting without holding anyone's answers.
/// A *student* always passes it — without it, nothing distinguishes their
/// report from any other candidate's.
pub fn check_batch_inclusion(
    batch_public_values: &[u8],
    expected_commitment: &[u8; 32],
    report: &ScoreReport,
    path: &MerklePath,
    expected_sheet_hash: Option<&[u8; 32]>,
) -> Result<BatchPublicValues, VerifyError> {
    let pv = check_batch_public_values(batch_public_values, expected_commitment)?;

    // The report carries its own copy of the commitment (it is hashed into the
    // leaf); a mismatch here means the published report and the proven sitting
    // disagree about which key was used, whatever the proof says.
    if report.key_commitment != *expected_commitment {
        return Err(VerifyError::CommitmentMismatch {
            expected: *expected_commitment,
            found: report.key_commitment,
        });
    }
    if report.exam_id != pv.exam_id {
        return Err(VerifyError::ExamIdMismatch {
            expected: pv.exam_id,
            found: report.exam_id,
        });
    }
    if report.score_bp > FULL_SCORE_BP {
        return Err(VerifyError::ScoreOutOfRange {
            score_bp: report.score_bp,
        });
    }
    // Checked before inclusion so a student whose answers do not match gets
    // told *that*, rather than the far more alarming "you are not in this
    // sitting" — which would also be true, and useless.
    if let Some(expected) = expected_sheet_hash {
        if report.sheet_hash != *expected {
            return Err(VerifyError::SheetHashMismatch {
                expected: *expected,
                found: report.sheet_hash,
            });
        }
    }
    if !verify_merkle_path(
        &leaf_hash(report),
        path,
        pv.leaf_count as usize,
        &pv.batch_root,
    ) {
        return Err(VerifyError::NotInBatch {
            index: path.index,
            leaf_count: pv.leaf_count,
        });
    }
    Ok(pv)
}
