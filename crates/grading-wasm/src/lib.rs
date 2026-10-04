//! C-ABI bindings that expose `grading-core` to JavaScript through WebAssembly.
//! Every hash, grade, tree and check comes from grading-core itself; this crate
//! only moves bytes in and out of linear memory. The only intended caller is
//! JavaScript passing buffers it allocated with `q_alloc`.
//!
//! All integers are little-endian. Every field the canonical encoding commits
//! to is carried, so commitments and sheet hashes match those of any exam:
//!
//! Key:   [exam_id u64][num_choices u8][policy u8: 0 full credit, 1 redistribute][n u32]
//!        then per question [question_id u32][weight u32][cancelled u8][k u8][k accepted choices]
//! Sheet: [exam_id u64][pseudonym 32 bytes][n u32][n answers, 0xFF = blank]

// Raw pointers are the ABI: every one comes from JavaScript, pointing into
// buffers it allocated with `q_alloc` and sized for the call.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use grading_core::{
    check_batch_inclusion, check_public_values, commit_answer_key, decode_public_values,
    encode_batch_public_values, encode_public_values, grade_batch, hash_answer_sheet, leaf_hash,
    score, AnswerKey, AnswerSheet, CancelPolicy, KeyEntry, MerklePath, VerifyError,
};

fn input<'a>(p: *const u8, len: usize) -> &'a [u8] {
    unsafe { core::slice::from_raw_parts(p, len) }
}

fn output<'a>(p: *mut u8, len: usize) -> &'a mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(p, len) }
}

fn arr32(p: *const u8) -> [u8; 32] {
    input(p, 32).try_into().unwrap()
}

/// A bounds-checked little-endian reader: malformed input yields `None`, never a panic.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(head)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    }
    fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|b| u64::from_le_bytes(b.try_into().unwrap()))
    }
    fn done(&self) -> bool {
        self.0.is_empty()
    }
}

fn parse_key(b: &[u8]) -> Option<AnswerKey> {
    let mut r = Reader(b);
    let exam_id = r.u64()?;
    let num_choices = r.u8()?;
    let cancel_policy = match r.u8()? {
        0 => CancelPolicy::FullCredit,
        1 => CancelPolicy::Redistribute,
        _ => return None,
    };
    let n = r.u32()? as usize;
    let mut entries = Vec::with_capacity(n.min(b.len()));
    for _ in 0..n {
        let question_id = r.u32()?;
        let weight = r.u32()?;
        let cancelled = r.u8()? != 0;
        let k = r.u8()? as usize;
        let accepted = r.take(k)?.to_vec();
        entries.push(KeyEntry {
            question_id,
            weight,
            accepted,
            cancelled,
        });
    }
    r.done().then_some(AnswerKey {
        exam_id,
        num_choices,
        cancel_policy,
        entries,
    })
}

fn parse_sheet(b: &[u8]) -> Option<AnswerSheet> {
    let mut r = Reader(b);
    let exam_id = r.u64()?;
    let student_pseudonym = r.take(32)?.try_into().unwrap();
    let n = r.u32()? as usize;
    let answers = r
        .take(n)?
        .iter()
        .map(|&a| if a == 0xFF { None } else { Some(a) })
        .collect();
    r.done().then_some(AnswerSheet {
        exam_id,
        student_pseudonym,
        answers,
    })
}

fn verify_code(e: VerifyError) -> i32 {
    match e {
        VerifyError::Malformed(_) => 1,
        VerifyError::CommitmentMismatch { .. } => 2,
        VerifyError::SheetHashMismatch { .. } => 3,
        VerifyError::ScoreOutOfRange { .. } => 4,
        VerifyError::ExamIdMismatch { .. } => 5,
        VerifyError::NotInBatch { .. } => 6,
    }
}

#[no_mangle]
pub extern "C" fn q_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len);
    let p = v.as_mut_ptr();
    core::mem::forget(v);
    p
}

#[no_mangle]
pub extern "C" fn q_free(p: *mut u8, len: usize) {
    unsafe { drop(Vec::from_raw_parts(p, 0, len)) }
}

/// commit_answer_key(key, salt) -> 32 bytes at `out`.
#[no_mangle]
pub extern "C" fn q_commit(key: *const u8, key_len: usize, salt: *const u8, out: *mut u8) -> i32 {
    let Some(key) = parse_key(input(key, key_len)) else { return -1 };
    output(out, 32).copy_from_slice(&commit_answer_key(&key, &arr32(salt)));
    0
}

/// hash_answer_sheet(sheet) -> 32 bytes at `out`.
#[no_mangle]
pub extern "C" fn q_sheet_hash(sheet: *const u8, sheet_len: usize, out: *mut u8) -> i32 {
    let Some(sheet) = parse_sheet(input(sheet, sheet_len)) else { return -1 };
    output(out, 32).copy_from_slice(&hash_answer_sheet(&sheet));
    0
}

/// The single-sheet guest's computation: score() and its 92 public-value bytes.
#[no_mangle]
pub extern "C" fn q_grade(
    key: *const u8,
    key_len: usize,
    salt: *const u8,
    sheet: *const u8,
    sheet_len: usize,
    out: *mut u8,
) -> i32 {
    let (Some(key), Some(sheet)) = (parse_key(input(key, key_len)), parse_sheet(input(sheet, sheet_len)))
    else {
        return -1;
    };
    match score(&key, &arr32(salt), &sheet) {
        Ok(r) => {
            output(out, 92).copy_from_slice(&encode_public_values(&r));
            0
        }
        Err(_) => -2,
    }
}

/// The batch guest's computation: grade_batch() over `n` sheets of `sheet_len` bytes each.
/// Writes: 76-byte batch public values | n x 92 report bytes | n x 32 leaves |
/// per candidate [sibling count][siblings x 32]. Returns bytes written, or < 0.
#[no_mangle]
pub extern "C" fn q_grade_batch(
    key: *const u8,
    key_len: usize,
    salt: *const u8,
    sheets: *const u8,
    sheet_len: usize,
    n: usize,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    let Some(key) = parse_key(input(key, key_len)) else { return -1 };
    let raw = input(sheets, sheet_len * n);
    let Some(sheets) = raw.chunks(sheet_len).map(parse_sheet).collect::<Option<Vec<_>>>() else {
        return -1;
    };
    let Ok(outcome) = grade_batch(&key, &arr32(salt), &sheets) else { return -2 };
    let Ok(paths) = outcome.paths() else { return -3 };

    let mut buf = Vec::new();
    buf.extend_from_slice(&encode_batch_public_values(&outcome.public_values()));
    for r in &outcome.reports {
        buf.extend_from_slice(&encode_public_values(r));
    }
    for l in &outcome.leaves {
        buf.extend_from_slice(l);
    }
    for p in &paths {
        buf.push(p.siblings.len() as u8);
        for s in &p.siblings {
            buf.extend_from_slice(s);
        }
    }
    if buf.len() > out_cap {
        return -4;
    }
    output(out, buf.len()).copy_from_slice(&buf);
    buf.len() as i32
}

/// leaf_hash() of a report given as its 92 public-value bytes.
#[no_mangle]
pub extern "C" fn q_leaf_hash(report_pv: *const u8, out: *mut u8) -> i32 {
    let Ok(r) = decode_public_values(input(report_pv, 92)) else { return -1 };
    output(out, 32).copy_from_slice(&leaf_hash(&r));
    0
}

/// check_public_values(): 0 = accepted, otherwise the VerifyError code.
#[no_mangle]
pub extern "C" fn q_check_pv(pv: *const u8, commitment: *const u8, sheet_hash: *const u8) -> i32 {
    let expected_sheet = (!sheet_hash.is_null()).then(|| arr32(sheet_hash));
    match check_public_values(input(pv, 92), &arr32(commitment), expected_sheet.as_ref()) {
        Ok(_) => 0,
        Err(e) => verify_code(e),
    }
}

/// check_batch_inclusion(): the whole student-side check for one row of a
/// sitting. 0 = accepted, otherwise the VerifyError code.
#[no_mangle]
pub extern "C" fn q_check_inclusion(
    batch_pv: *const u8,
    commitment: *const u8,
    report_pv: *const u8,
    index: u32,
    siblings: *const u8,
    n_siblings: usize,
    sheet_hash: *const u8,
) -> i32 {
    let Ok(report) = decode_public_values(input(report_pv, 92)) else { return 1 };
    let path = MerklePath {
        index,
        siblings: input(siblings, 32 * n_siblings)
            .chunks(32)
            .map(|c| c.try_into().unwrap())
            .collect(),
    };
    let expected_sheet = (!sheet_hash.is_null()).then(|| arr32(sheet_hash));
    match check_batch_inclusion(
        input(batch_pv, 76),
        &arr32(commitment),
        &report,
        &path,
        expected_sheet.as_ref(),
    ) {
        Ok(_) => 0,
        Err(e) => verify_code(e),
    }
}
