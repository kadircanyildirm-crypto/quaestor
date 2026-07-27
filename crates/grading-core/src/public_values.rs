//! The public-values ABI — the exact bytes a zkVM guest commits, and the only
//! thing a verifier ever reads.
//!
//! This module is the single source of truth for that layout. The guest writes
//! it with [`encode_public_values`]; every verifier (the host CLI today, a
//! browser WASM verifier tomorrow) reads it with [`decode_public_values`].
//! Previously the layout lived twice — as a sequence of `commit_slice` calls in
//! the guest and as hand-written byte offsets in the host — which is a layout
//! that can drift silently in exactly the place where drift is unfalsifiable.
//!
//! Deliberately zkVM-agnostic and `no_std`: checking a proof's *claims* needs
//! no proof system, so this code compiles to WASM for in-browser verification.
//!
//! There are two layouts, one per guest. The single-sheet guest commits one
//! report; the batch guest grades a whole sitting and commits only a Merkle
//! root over the reports (see [`crate::batch`]).
//!
//! **Single sheet — 92 bytes:**
//!
//! | offset | size | field |
//! |---|---|---|
//! | 0 | 32 | answer-key commitment |
//! | 32 | 32 | answer-sheet hash |
//! | 64 | 8 | exam id (LE) |
//! | 72 | 4 | score in basis points (LE) |
//! | 76 | 4 | correct count (LE) |
//! | 80 | 4 | wrong count (LE) |
//! | 84 | 4 | blank count (LE) |
//! | 88 | 4 | cancelled count (LE) |
//!
//! **Batch — 76 bytes:**
//!
//! | offset | size | field |
//! |---|---|---|
//! | 0 | 32 | answer-key commitment |
//! | 32 | 32 | batch root |
//! | 64 | 8 | exam id (LE) |
//! | 72 | 4 | leaf count (LE) |
//!
//! The two are kept unconfusable at three levels, because a verifier that
//! parsed one as the other would read a Merkle root as a sheet hash and accept
//! a claim nobody proved: the guests are separate programs with separate
//! verifying keys, the decoders demand an exact length, and the two lengths
//! differ — the last of which is enforced at compile time below rather than
//! left to whoever edits a field width next.

use core::fmt;

use crate::model::ScoreReport;
use crate::score::FULL_SCORE_BP;

/// Fixed width of the single-sheet guest's committed public values.
pub const PUBLIC_VALUES_LEN: usize = 92;

/// Fixed width of the batch guest's committed public values.
pub const BATCH_PUBLIC_VALUES_LEN: usize = 76;

const _: () = assert!(
    PUBLIC_VALUES_LEN != BATCH_PUBLIC_VALUES_LEN,
    "the two public-values layouts must differ in length: equal lengths would let a \
     verifier parse a batch root as a sheet hash"
);

const OFF_KEY_COMMITMENT: usize = 0;
const OFF_SHEET_HASH: usize = 32;
const OFF_EXAM_ID: usize = 64;
const OFF_SCORE_BP: usize = 72;
const OFF_CORRECT: usize = 76;
const OFF_WRONG: usize = 80;
const OFF_BLANK: usize = 84;
const OFF_CANCELLED: usize = 88;

const BOFF_KEY_COMMITMENT: usize = 0;
const BOFF_BATCH_ROOT: usize = 32;
const BOFF_EXAM_ID: usize = 64;
const BOFF_LEAF_COUNT: usize = 72;

/// Serialize a report into the guest's public-values layout.
pub fn encode_public_values(report: &ScoreReport) -> [u8; PUBLIC_VALUES_LEN] {
    let mut out = [0u8; PUBLIC_VALUES_LEN];
    let mut put32 = |off: usize, v: u32| out[off..off + 4].copy_from_slice(&v.to_le_bytes());
    put32(OFF_SCORE_BP, report.score_bp);
    put32(OFF_CORRECT, report.correct);
    put32(OFF_WRONG, report.wrong);
    put32(OFF_BLANK, report.blank);
    put32(OFF_CANCELLED, report.cancelled);
    out[OFF_KEY_COMMITMENT..OFF_KEY_COMMITMENT + 32].copy_from_slice(&report.key_commitment);
    out[OFF_SHEET_HASH..OFF_SHEET_HASH + 32].copy_from_slice(&report.sheet_hash);
    out[OFF_EXAM_ID..OFF_EXAM_ID + 8].copy_from_slice(&report.exam_id.to_le_bytes());
    out
}

/// Parse public values emitted by the guest. Total over all 92-byte inputs.
pub fn decode_public_values(bytes: &[u8]) -> Result<ScoreReport, PublicValuesError> {
    if bytes.len() != PUBLIC_VALUES_LEN {
        return Err(PublicValuesError::WrongLength {
            found: bytes.len(),
            expected: PUBLIC_VALUES_LEN,
        });
    }
    let at32 = |off: usize| u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
    let fixed32 = |off: usize| -> [u8; 32] { bytes[off..off + 32].try_into().unwrap() };
    Ok(ScoreReport {
        exam_id: u64::from_le_bytes(bytes[OFF_EXAM_ID..OFF_EXAM_ID + 8].try_into().unwrap()),
        key_commitment: fixed32(OFF_KEY_COMMITMENT),
        sheet_hash: fixed32(OFF_SHEET_HASH),
        score_bp: at32(OFF_SCORE_BP),
        correct: at32(OFF_CORRECT),
        wrong: at32(OFF_WRONG),
        blank: at32(OFF_BLANK),
        cancelled: at32(OFF_CANCELLED),
    })
}

/// Everything the batch guest makes public about a whole sitting.
///
/// Note what is *not* here: no score, no sheet hash, nothing about any
/// individual. One sitting of any size commits to these 76 bytes, and a
/// candidate turns them into a statement about themselves with an inclusion
/// path ([`crate::batch::check_batch_inclusion`]). That is the entire cost
/// argument for batching — the proof stops growing with the sitting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchPublicValues {
    /// Commitment to the single answer key every sheet in the batch was
    /// graded against; must equal the institution's pre-exam publication.
    pub key_commitment: [u8; 32],
    /// Merkle root over the sitting's report leaves.
    pub batch_root: [u8; 32],
    pub exam_id: u64,
    /// Number of candidates in the sitting. Part of the root's preimage, and
    /// repeated here so a verifier can check a path without trusting the
    /// institution's word for how many people sat the exam.
    pub leaf_count: u32,
}

/// Serialize a sitting's public values into the batch guest's layout.
pub fn encode_batch_public_values(pv: &BatchPublicValues) -> [u8; BATCH_PUBLIC_VALUES_LEN] {
    let mut out = [0u8; BATCH_PUBLIC_VALUES_LEN];
    out[BOFF_KEY_COMMITMENT..BOFF_KEY_COMMITMENT + 32].copy_from_slice(&pv.key_commitment);
    out[BOFF_BATCH_ROOT..BOFF_BATCH_ROOT + 32].copy_from_slice(&pv.batch_root);
    out[BOFF_EXAM_ID..BOFF_EXAM_ID + 8].copy_from_slice(&pv.exam_id.to_le_bytes());
    out[BOFF_LEAF_COUNT..BOFF_LEAF_COUNT + 4].copy_from_slice(&pv.leaf_count.to_le_bytes());
    out
}

/// Parse public values emitted by the batch guest. Total over all 76-byte
/// inputs.
pub fn decode_batch_public_values(bytes: &[u8]) -> Result<BatchPublicValues, PublicValuesError> {
    if bytes.len() != BATCH_PUBLIC_VALUES_LEN {
        return Err(PublicValuesError::WrongLength {
            found: bytes.len(),
            expected: BATCH_PUBLIC_VALUES_LEN,
        });
    }
    let fixed32 = |off: usize| -> [u8; 32] { bytes[off..off + 32].try_into().unwrap() };
    Ok(BatchPublicValues {
        key_commitment: fixed32(BOFF_KEY_COMMITMENT),
        batch_root: fixed32(BOFF_BATCH_ROOT),
        exam_id: u64::from_le_bytes(bytes[BOFF_EXAM_ID..BOFF_EXAM_ID + 8].try_into().unwrap()),
        leaf_count: u32::from_le_bytes(
            bytes[BOFF_LEAF_COUNT..BOFF_LEAF_COUNT + 4]
                .try_into()
                .unwrap(),
        ),
    })
}

/// The batch counterpart of [`check_public_values`]: parse a sitting's public
/// values and check they are about the key published before the exam.
///
/// Stops there on purpose. This is the half an auditor can run holding nothing
/// but public data — it establishes *which sitting under which key*, and says
/// nothing yet about any candidate. Binding it to a person is
/// [`crate::batch::check_batch_inclusion`], which needs that person's report.
pub fn check_batch_public_values(
    bytes: &[u8],
    expected_commitment: &[u8; 32],
) -> Result<BatchPublicValues, VerifyError> {
    let pv = decode_batch_public_values(bytes).map_err(VerifyError::Malformed)?;
    if pv.key_commitment != *expected_commitment {
        return Err(VerifyError::CommitmentMismatch {
            expected: *expected_commitment,
            found: pv.key_commitment,
        });
    }
    Ok(pv)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicValuesError {
    WrongLength { found: usize, expected: usize },
}

impl fmt::Display for PublicValuesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PublicValuesError::WrongLength { found, expected } => {
                write!(f, "public values are {found} bytes, expected {expected}")
            }
        }
    }
}

/// Why a verifier rejected an otherwise cryptographically valid proof.
///
/// A proof that verifies only says "*some* sheet was graded under *some* key".
/// These are the checks that turn it into "*my* sheet was graded under the key
/// committed before the exam" — without them the proof means nothing to the
/// student, so they are modeled as hard errors, never warnings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyError {
    Malformed(PublicValuesError),
    /// The proof is about a different answer key than the one published
    /// before the exam — the tamper case the whole system exists to catch.
    CommitmentMismatch {
        expected: [u8; 32],
        found: [u8; 32],
    },
    /// The proof is about a different answer sheet than the student's.
    SheetHashMismatch {
        expected: [u8; 32],
        found: [u8; 32],
    },
    /// Structurally impossible report; a correct guest can never emit one, so
    /// seeing it means the proof came from a different program.
    ScoreOutOfRange {
        score_bp: u32,
    },
    /// The report is for a different exam than the sitting the proof is about.
    ExamIdMismatch {
        expected: u64,
        found: u64,
    },
    /// The report does not sit at the claimed position in the proven sitting.
    /// The batched form of "this grade was never proven": the number reached
    /// the student, but nothing binds it to the computation that was attested.
    NotInBatch {
        index: u32,
        leaf_count: u32,
    },
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::Malformed(e) => write!(f, "{e}"),
            VerifyError::CommitmentMismatch { .. } => f.write_str(
                "key commitment does not match the pre-exam publication: this proof \
                 was produced under a different answer key",
            ),
            VerifyError::SheetHashMismatch { .. } => f.write_str(
                "answer-sheet hash does not match your answers: this proof is about \
                 a different sheet",
            ),
            VerifyError::ScoreOutOfRange { score_bp } => {
                write!(f, "score {score_bp} basis points is out of range 0..=10000")
            }
            VerifyError::ExamIdMismatch { expected, found } => write!(
                f,
                "this report is for exam {found}, but the proof is about exam {expected}"
            ),
            VerifyError::NotInBatch { index, leaf_count } => write!(
                f,
                "no inclusion path places this report at position {index} of the \
                 {leaf_count}-candidate sitting this proof commits to: the score \
                 was published, but never proven"
            ),
        }
    }
}

/// The claim checks a verifier performs *after* the proof system has accepted
/// the proof. Separated from proof verification on purpose: this half needs no
/// zkVM, so it is unit-testable on any platform and reusable in a browser.
///
/// `expected_sheet_hash` is optional because an auditor may legitimately check
/// a proof against the published commitment without holding anyone's answers;
/// a *student* should always pass it, which is what binds the proof to them.
pub fn check_public_values(
    bytes: &[u8],
    expected_commitment: &[u8; 32],
    expected_sheet_hash: Option<&[u8; 32]>,
) -> Result<ScoreReport, VerifyError> {
    let report = decode_public_values(bytes).map_err(VerifyError::Malformed)?;
    if report.key_commitment != *expected_commitment {
        return Err(VerifyError::CommitmentMismatch {
            expected: *expected_commitment,
            found: report.key_commitment,
        });
    }
    if let Some(expected) = expected_sheet_hash {
        if report.sheet_hash != *expected {
            return Err(VerifyError::SheetHashMismatch {
                expected: *expected,
                found: report.sheet_hash,
            });
        }
    }
    if report.score_bp > FULL_SCORE_BP {
        return Err(VerifyError::ScoreOutOfRange {
            score_bp: report.score_bp,
        });
    }
    Ok(report)
}
