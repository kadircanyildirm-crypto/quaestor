//! Canonical byte encoding for everything that gets hashed.
//!
//! Hand-rolled on purpose: commitment security depends on this encoding being
//! injective and stable forever, so we do not want a serde backend's choices
//! (or version bumps) in the trusted base. Layout rules:
//!
//! - integers are little-endian, fixed width
//! - sequences are prefixed with their `u32` length
//! - every top-level encoding starts with a domain tag and a format version

use alloc::vec::Vec;

use crate::model::{AnswerKey, AnswerSheet, BLANK_MARKER};

/// Bumped only if the encoding layout itself changes (which changes every
/// commitment, so: effectively never within a deployment).
pub const ENCODING_VERSION: u8 = 0;

pub(crate) const KEY_DOMAIN: &[u8] = b"ispat/answer-key";
pub(crate) const SHEET_DOMAIN: &[u8] = b"ispat/answer-sheet";

pub(crate) struct Encoder {
    buf: Vec<u8>,
}

impl Encoder {
    pub fn with_domain(domain: &[u8]) -> Self {
        let mut e = Encoder { buf: Vec::new() };
        e.bytes(domain);
        e.u8(ENCODING_VERSION);
        e
    }

    pub fn finish(self) -> Vec<u8> {
        self.buf
    }

    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Length-prefixed byte sequence.
    pub fn bytes(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.buf.extend_from_slice(v);
    }

    /// Fixed-width bytes, no length prefix (width is part of the schema).
    pub fn fixed(&mut self, v: &[u8]) {
        self.buf.extend_from_slice(v);
    }
}

pub(crate) fn encode_answer_key(key: &AnswerKey) -> Vec<u8> {
    let mut e = Encoder::with_domain(KEY_DOMAIN);
    e.u64(key.exam_id);
    e.u8(key.num_choices);
    e.u8(key.cancel_policy as u8);
    e.u32(key.entries.len() as u32);
    for entry in &key.entries {
        e.u32(entry.question_id);
        e.u32(entry.weight);
        e.u8(entry.cancelled as u8);
        e.bytes(&entry.accepted);
    }
    e.finish()
}

pub(crate) fn encode_answer_sheet(sheet: &AnswerSheet) -> Vec<u8> {
    let mut e = Encoder::with_domain(SHEET_DOMAIN);
    e.u64(sheet.exam_id);
    e.fixed(&sheet.student_pseudonym);
    e.u32(sheet.answers.len() as u32);
    for answer in &sheet.answers {
        e.u8(answer.unwrap_or(BLANK_MARKER));
    }
    e.finish()
}
