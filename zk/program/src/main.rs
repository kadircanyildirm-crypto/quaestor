//! SP1 guest: the exact grading computation whose execution is proven.
//!
//! Trusted computing base by design: this file + `grading-core` and nothing
//! else. No serde, no JSON — inputs arrive as quaestor's canonical bytes and are
//! decoded by the same ~100 lines the commitments are defined over.
//!
//! Any panic here (malformed input, failed validation) aborts proving — a
//! proof over bad inputs must be impossible, not merely unlikely.

#![no_main]

sp1_zkvm::entrypoint!(main);

use grading_core::{decode_answer_key, decode_answer_sheet, score};

pub fn main() {
    // Private witness: canonical key bytes + salt.
    // The sheet is private to the proof but publicly bound via its hash.
    let key_bytes = sp1_zkvm::io::read_vec();
    let salt_bytes = sp1_zkvm::io::read_vec();
    let sheet_bytes = sp1_zkvm::io::read_vec();

    let salt: [u8; 32] = salt_bytes
        .as_slice()
        .try_into()
        .expect("salt must be exactly 32 bytes");
    let key = decode_answer_key(&key_bytes).expect("malformed answer key");
    let sheet = decode_answer_sheet(&sheet_bytes).expect("malformed answer sheet");

    let report = score(&key, &salt, &sheet).expect("scoring rejected inputs");

    // Public values — fixed 92-byte layout, documented in zk/README.md.
    // The verifier checks key_commitment against the pre-exam publication and
    // sheet_hash against the student's own copy of their answers.
    sp1_zkvm::io::commit_slice(&report.key_commitment);
    sp1_zkvm::io::commit_slice(&report.sheet_hash);
    sp1_zkvm::io::commit_slice(&report.exam_id.to_le_bytes());
    sp1_zkvm::io::commit_slice(&report.score_bp.to_le_bytes());
    sp1_zkvm::io::commit_slice(&report.correct.to_le_bytes());
    sp1_zkvm::io::commit_slice(&report.wrong.to_le_bytes());
    sp1_zkvm::io::commit_slice(&report.blank.to_le_bytes());
    sp1_zkvm::io::commit_slice(&report.cancelled.to_le_bytes());
}
