//! # grading-core
//!
//! Deterministic grading engine designed to run unchanged in two places:
//! on an ordinary server, and inside a zkVM guest (SP1/RISC Zero) where its
//! execution is proven. That dual life imposes three hard constraints on
//! everything in this crate:
//!
//! 1. **Determinism.** No floating point, no hash-map iteration order, no
//!    randomness, no clocks. Scores are integer basis points (0..=10_000).
//! 2. **`no_std` + `alloc`.** zkVM guests have no OS.
//! 3. **Canonical byte encodings.** Commitments are hashes over a canonical
//!    serialization defined in [`encode`]; two semantically equal values must
//!    always produce identical bytes.
//!
//! The statement a zk proof will ultimately make (see docs/ARCHITECTURE.md):
//!
//! > "There exists an answer key `K` and salt `s` such that
//! > `commit(K, s) = C` (public), and scoring `K` against the answer sheet
//! > with hash `H` (public) yields exactly the score `S` (public)."

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod commit;
pub mod encode;
pub mod model;
pub mod score;

pub use commit::{commit_answer_key, hash_answer_sheet, Salt};
pub use encode::{
    decode_answer_key, decode_answer_sheet, encode_answer_key, encode_answer_sheet, DecodeError,
};
pub use model::{AnswerKey, AnswerSheet, CancelPolicy, Choice, KeyEntry, ScoreReport};
pub use score::{score, ScoreError};
