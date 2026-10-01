//! SP1 batch guest: one execution grades an entire exam sitting.
//!
//! Same trusted computing base as the single-sheet guest — this file plus
//! `grading-core`, no serde, no JSON — and the same grading code. What changes
//! is what reaches the public: instead of one report, the guest commits a
//! Merkle root over every candidate's report. A sitting of 200,000 therefore
//! costs one proof, and a candidate turns it into a statement about themselves
//! with a `log2(n)` inclusion path they can check in a browser.
//!
//! Deliberately a separate program from `zk/program` rather than a mode flag on
//! it. Two programs mean two verifying keys, which is what makes it impossible
//! to present a batch proof where a single-sheet proof is expected: a verifier
//! that only knows one program's key cannot be handed the other's output at
//! all, whatever the bytes look like.
//!
//! Any panic here (malformed input, a sheet from another exam, a bad key)
//! aborts proving: a proof over inputs the grading engine rejects must be
//! impossible, not merely unlikely.

#![no_main]

sp1_zkvm::entrypoint!(main);

use grading_core::{
    decode_answer_key, decode_answer_sheet, encode_batch_public_values, grade_batch, MAX_BATCH,
};

pub fn main() {
    // Private witness: the canonical key bytes and the salt it was committed
    // under. Every sheet in the sitting is graded against this one key, which
    // is exactly the claim the published commitment pins down.
    let key_bytes = sp1_zkvm::io::read_vec();
    let salt_bytes = sp1_zkvm::io::read_vec();
    let count_bytes = sp1_zkvm::io::read_vec();

    let salt: [u8; 32] = salt_bytes
        .as_slice()
        .try_into()
        .expect("salt must be exactly 32 bytes");
    let count = u32::from_le_bytes(
        count_bytes
            .as_slice()
            .try_into()
            .expect("sheet count must be exactly 4 bytes"),
    ) as usize;
    assert!(
        count <= MAX_BATCH,
        "sitting is larger than one batch: shard it and aggregate"
    );

    let key = decode_answer_key(&key_bytes).expect("malformed answer key");

    // Sheet order is leaf order, and the host must reproduce it to hand out
    // inclusion paths. Nothing here trusts `count`: it only says how many
    // sheets to read, and the committed leaf count below is the number of
    // reports actually produced.
    let mut sheets = Vec::with_capacity(count);
    for _ in 0..count {
        let sheet_bytes = sp1_zkvm::io::read_vec();
        sheets.push(decode_answer_sheet(&sheet_bytes).expect("malformed answer sheet"));
    }

    let outcome = grade_batch(&key, &salt, &sheets).expect("batch grading rejected inputs");

    // Public values — the 76-byte batch layout owned by `grading_core`, built
    // by the same function the host uses to publish the manifest, so the root
    // students check their paths against and the root the proof commits to
    // cannot drift apart.
    sp1_zkvm::io::commit_slice(&encode_batch_public_values(&outcome.public_values()));
}
