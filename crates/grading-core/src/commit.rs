//! Commitments and hashes over canonical encodings.
//!
//! The answer-key commitment is `SHA-256(canonical(key) || salt)`. The salt is
//! a 32-byte secret chosen by the institution; it makes the commitment hiding
//! (without it, a small answer key could be brute-forced from its hash). The
//! commitment is published before the exam; the (key, salt) pair stays private
//! and is later fed to the prover as witness.

use sha2::{Digest, Sha256};

use crate::encode::{encode_answer_key, encode_answer_sheet};
use crate::model::{AnswerKey, AnswerSheet};

/// Blinding salt for the answer-key commitment. Must be sampled uniformly at
/// random by the committer and kept secret alongside the key.
pub type Salt = [u8; 32];

/// Commitment published before the exam. Binding: any change to the key
/// (an accepted answer, a weight, a cancellation flag) changes this hash.
pub fn commit_answer_key(key: &AnswerKey, salt: &Salt) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(encode_answer_key(key));
    hasher.update(salt);
    hasher.finalize().into()
}

/// Canonical hash of one answer sheet. Unsalted: sheets are low-entropy but
/// they are inputs the student themselves knows and can recompute, and the
/// hash's job is integrity (did the institution grade *my* answers?), not
/// hiding.
pub fn hash_answer_sheet(sheet: &AnswerSheet) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(encode_answer_sheet(sheet));
    hasher.finalize().into()
}
