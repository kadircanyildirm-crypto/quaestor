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

use crate::model::{AnswerKey, AnswerSheet, CancelPolicy, KeyEntry, BLANK_MARKER};

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

/// Canonical bytes of an answer key — the exact preimage (minus salt) of the
/// published commitment, and the wire format between prover host and zkVM guest.
pub fn encode_answer_key(key: &AnswerKey) -> Vec<u8> {
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

/// Canonical bytes of an answer sheet — preimage of the sheet hash and the
/// wire format between prover host and zkVM guest.
pub fn encode_answer_sheet(sheet: &AnswerSheet) -> Vec<u8> {
    let mut e = Encoder::with_domain(SHEET_DOMAIN);
    e.u64(sheet.exam_id);
    e.fixed(&sheet.student_pseudonym);
    e.u32(sheet.answers.len() as u32);
    for answer in &sheet.answers {
        e.u8(answer.unwrap_or(BLANK_MARKER));
    }
    e.finish()
}

/// Why a decoded value was rejected. Decoding checks *structure* only;
/// semantic key validity (sorted accepted sets, unique ids, ...) is
/// [`crate::score::validate_key`]'s job and runs inside scoring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    UnexpectedEof,
    WrongDomain,
    UnsupportedVersion(u8),
    /// Input continues past the end of the encoded value — never canonical.
    TrailingBytes,
    InvalidCancelPolicy(u8),
    InvalidBool(u8),
}

struct Decoder<'a> {
    rest: &'a [u8],
}

impl<'a> Decoder<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Decoder { rest: buf }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if self.rest.len() < n {
            return Err(DecodeError::UnexpectedEof);
        }
        let (head, tail) = self.rest.split_at(n);
        self.rest = tail;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn bytes(&mut self) -> Result<&'a [u8], DecodeError> {
        let n = self.u32()? as usize;
        self.take(n)
    }

    fn fixed32(&mut self) -> Result<[u8; 32], DecodeError> {
        Ok(self.take(32)?.try_into().unwrap())
    }

    fn bool(&mut self) -> Result<bool, DecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            b => Err(DecodeError::InvalidBool(b)),
        }
    }

    fn expect_header(&mut self, domain: &[u8]) -> Result<(), DecodeError> {
        if self.bytes()? != domain {
            return Err(DecodeError::WrongDomain);
        }
        match self.u8()? {
            ENCODING_VERSION => Ok(()),
            v => Err(DecodeError::UnsupportedVersion(v)),
        }
    }

    fn finish(self) -> Result<(), DecodeError> {
        if self.rest.is_empty() {
            Ok(())
        } else {
            Err(DecodeError::TrailingBytes)
        }
    }
}

/// Inverse of [`encode_answer_key`]. Total: `decode(encode(k)) == k` for every
/// key, and decoding never panics on arbitrary input.
pub fn decode_answer_key(buf: &[u8]) -> Result<AnswerKey, DecodeError> {
    let mut d = Decoder::new(buf);
    d.expect_header(KEY_DOMAIN)?;
    let exam_id = d.u64()?;
    let num_choices = d.u8()?;
    let cancel_policy = match d.u8()? {
        0 => CancelPolicy::FullCredit,
        1 => CancelPolicy::Redistribute,
        p => return Err(DecodeError::InvalidCancelPolicy(p)),
    };
    let n = d.u32()? as usize;
    // Capacity is bounded by what the buffer can actually hold, so a hostile
    // length prefix cannot trigger a huge allocation before EOF is detected.
    let mut entries = Vec::with_capacity(n.min(d.rest.len() / 10 + 1));
    for _ in 0..n {
        let question_id = d.u32()?;
        let weight = d.u32()?;
        let cancelled = d.bool()?;
        let accepted = d.bytes()?.to_vec();
        entries.push(KeyEntry {
            question_id,
            weight,
            accepted,
            cancelled,
        });
    }
    d.finish()?;
    Ok(AnswerKey {
        exam_id,
        num_choices,
        cancel_policy,
        entries,
    })
}

/// Inverse of [`encode_answer_sheet`].
pub fn decode_answer_sheet(buf: &[u8]) -> Result<AnswerSheet, DecodeError> {
    let mut d = Decoder::new(buf);
    d.expect_header(SHEET_DOMAIN)?;
    let exam_id = d.u64()?;
    let student_pseudonym = d.fixed32()?;
    let n = d.u32()? as usize;
    let mut answers = Vec::with_capacity(n.min(d.rest.len() + 1));
    for _ in 0..n {
        answers.push(match d.u8()? {
            BLANK_MARKER => None,
            c => Some(c),
        });
    }
    d.finish()?;
    Ok(AnswerSheet {
        exam_id,
        student_pseudonym,
        answers,
    })
}
