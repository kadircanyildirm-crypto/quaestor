//! C-ABI bindings that expose `grading-core` to JavaScript through WebAssembly.
//! Every hash, grade, tree and check comes from grading-core itself; this crate
//! only moves bytes in and out of linear memory. The only intended caller is
//! JavaScript passing buffers it allocated with `q_alloc`.
//!
//! Key input:   [n][num_choices][policy] then per question [weight][cancelled][accepted bitmask]
//! Sheet input: [32-byte pseudonym][n answers, 0xFF = blank]

// Raw pointers are the ABI: every one comes from JavaScript, pointing into
// buffers it allocated with `q_alloc` and sized for the call.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use grading_core::{
    check_batch_inclusion, check_public_values, commit_answer_key, decode_public_values,
    encode_batch_public_values, encode_public_values, grade_batch, hash_answer_sheet, leaf_hash,
    score, AnswerKey, AnswerSheet, CancelPolicy, KeyEntry, MerklePath, VerifyError,
};

const EXAM_ID: u64 = 20260701;

fn input<'a>(p: *const u8, len: usize) -> &'a [u8] {
    unsafe { core::slice::from_raw_parts(p, len) }
}

fn output<'a>(p: *mut u8, len: usize) -> &'a mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(p, len) }
}

fn arr32(p: *const u8) -> [u8; 32] {
    input(p, 32).try_into().unwrap()
}

fn parse_key(b: &[u8]) -> Option<AnswerKey> {
    if b.len() < 3 {
        return None;
    }
    let n = b[0] as usize;
    if b.len() != 3 + 3 * n {
        return None;
    }
    let cancel_policy = match b[2] {
        0 => CancelPolicy::FullCredit,
        1 => CancelPolicy::Redistribute,
        _ => return None,
    };
    let entries = (0..n)
        .map(|i| {
            let q = &b[3 + 3 * i..6 + 3 * i];
            KeyEntry {
                question_id: i as u32 + 1,
                weight: q[0] as u32,
                accepted: (0..8u8).filter(|c| q[2] & (1 << c) != 0).collect(),
                cancelled: q[1] != 0,
            }
        })
        .collect();
    Some(AnswerKey {
        exam_id: EXAM_ID,
        num_choices: b[1],
        cancel_policy,
        entries,
    })
}

fn parse_sheet(b: &[u8]) -> Option<AnswerSheet> {
    if b.len() < 32 {
        return None;
    }
    Some(AnswerSheet {
        exam_id: EXAM_ID,
        student_pseudonym: b[..32].try_into().unwrap(),
        answers: b[32..]
            .iter()
            .map(|&a| if a == 0xFF { None } else { Some(a) })
            .collect(),
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
