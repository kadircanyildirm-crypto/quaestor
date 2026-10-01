//! What it costs a candidate to check their own result, as the sitting grows.
//!
//! ```text
//! cargo run --release -p grading-core --example verify_cost
//! cargo run --release -p grading-core --example verify_cost -- --fixtures bench/out/verify-fixtures.json
//! ```
//!
//! Each sitting is graded with `grade_batch`, the function the batch guest runs.
//! The timed work is what a candidate does with the published results: hash
//! their own answer sheet, then run `check_batch_inclusion` on their row. The
//! SP1 proof verification that comes first is not included; it needs a real
//! proof, which has not been generated yet.
//!
//! `--fixtures` writes each sitting's public values, the middle candidate's row
//! and path, and their sheet, so `bench/verify-wasm.mjs` can time the same
//! check in WebAssembly.

use std::fmt::Write as _;
use std::hint::black_box;
use std::time::{Duration, Instant};

use grading_core::{
    check_batch_inclusion, encode_batch_public_values, encode_public_values, grade_batch,
    hash_answer_sheet, AnswerKey, AnswerSheet, CancelPolicy, KeyEntry, MerklePath, ScoreReport,
    BATCH_PUBLIC_VALUES_LEN,
};

const EXAM_ID: u64 = 20260701;
const QUESTIONS: u32 = 100;
const CHOICES: u8 = 5;
const SALT: [u8; 32] = [1; 32];
const SIZES: [usize; 6] = [10, 100, 1_000, 10_000, 100_000, 1_000_000];

/// xorshift64*: deterministic, dependency-free, and plenty for synthetic answers.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) % n
    }
}

/// The same exam shape as `bench/generate-sitting.py`: 100 questions, five
/// choices, the last question cancelled, every 17th with two accepted answers.
fn key() -> AnswerKey {
    let mut rng = Rng::new(1);
    let entries = (1..=QUESTIONS)
        .map(|id| {
            let cancelled = id == QUESTIONS;
            let accepted = if cancelled {
                Vec::new()
            } else if id % 17 == 0 {
                let a = rng.below(CHOICES as u64) as u8;
                let b = (a + 1 + rng.below(CHOICES as u64 - 1) as u8) % CHOICES;
                let mut pair = vec![a, b];
                pair.sort_unstable();
                pair
            } else {
                vec![rng.below(CHOICES as u64) as u8]
            };
            KeyEntry {
                question_id: id,
                weight: 10,
                accepted,
                cancelled,
            }
        })
        .collect();
    AnswerKey {
        exam_id: EXAM_ID,
        num_choices: CHOICES,
        cancel_policy: CancelPolicy::FullCredit,
        entries,
    }
}

/// About 8% blank, about 47% correct, the rest random: the generator's mix.
fn sheet(index: usize, key: &AnswerKey) -> AnswerSheet {
    let mut rng = Rng::new(1_000_000 + index as u64);
    let mut student_pseudonym = [0u8; 32];
    student_pseudonym[24..].copy_from_slice(&(index as u64).to_be_bytes());
    let answers = key
        .entries
        .iter()
        .map(|q| match rng.below(100) {
            0..=7 => None,
            8..=54 if !q.accepted.is_empty() => Some(q.accepted[0]),
            _ => Some(rng.below(CHOICES as u64) as u8),
        })
        .collect();
    AnswerSheet {
        exam_id: EXAM_ID,
        student_pseudonym,
        answers,
    }
}

/// One candidate's view of a graded sitting: everything their check needs.
struct Row {
    n: usize,
    grading: Duration,
    public_values: [u8; BATCH_PUBLIC_VALUES_LEN],
    commitment: [u8; 32],
    report: ScoreReport,
    path: MerklePath,
    mine: AnswerSheet,
    samples: Vec<Duration>,
}

impl Row {
    /// Hash your own sheet, then check your row: the candidate-side claim check.
    fn check(&self) {
        let my_hash = hash_answer_sheet(black_box(&self.mine));
        check_batch_inclusion(
            black_box(&self.public_values),
            &self.commitment,
            &self.report,
            &self.path,
            Some(&my_hash),
        )
        .expect("an honest row verifies");
    }
}

fn grade(n: usize, key: &AnswerKey) -> Row {
    let sheets: Vec<AnswerSheet> = (0..n).map(|i| sheet(i, key)).collect();
    let t = Instant::now();
    let outcome = grade_batch(key, &SALT, &sheets).expect("the synthetic sitting is valid");
    let grading = t.elapsed();
    let me = n / 2;
    Row {
        n,
        grading,
        public_values: encode_batch_public_values(&outcome.public_values()),
        commitment: outcome.key_commitment,
        report: outcome.reports[me].clone(),
        path: outcome.path(me).expect("candidate is in the sitting"),
        mine: sheets[me].clone(),
        samples: Vec::new(),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let fixtures_path = args
        .iter()
        .position(|a| a == "--fixtures")
        .and_then(|i| args.get(i + 1));
    let key = key();
    let mut rows: Vec<Row> = SIZES.iter().map(|&n| grade(n, &key)).collect();

    // Timed round-robin, so drift over the run (CPU clocks, a hybrid CPU moving
    // the thread to an efficiency core) lands on every sitting alike.
    const ITERS: u32 = 20_000;
    for row in &rows {
        (0..ITERS).for_each(|_| row.check());
    }
    for _ in 0..15 {
        for row in &mut rows {
            let t = Instant::now();
            (0..ITERS).for_each(|_| row.check());
            row.samples.push(t.elapsed() / ITERS);
        }
    }

    println!("| Candidates | Inclusion path | Data per candidate | Check (native) | grade_batch (native) |");
    println!("|---:|---:|---:|---:|---:|");
    let mut fixtures = Vec::new();
    for row in &mut rows {
        row.samples.sort_unstable();
        let check = row.samples[row.samples.len() / 2];
        let levels = row.path.siblings.len();
        // What a candidate needs besides the proof: their report and one hash per level.
        let bytes = 92 + 32 * levels;
        println!(
            "| {} | {levels} hashes | {bytes} B | {:.1} µs | {:.2} s |",
            row.n,
            check.as_secs_f64() * 1e6,
            row.grading.as_secs_f64()
        );

        if fixtures_path.is_some() {
            let answers: Vec<String> = row
                .mine
                .answers
                .iter()
                .map(|a| a.map_or("null".into(), |c| c.to_string()))
                .collect();
            let siblings: Vec<String> = row
                .path
                .siblings
                .iter()
                .map(|s| format!("\"{}\"", hex(s)))
                .collect();
            fixtures.push(format!(
                "{{\"n\":{},\"index\":{},\"batch_public_values\":\"{}\",\"commitment\":\"{}\",\"report\":\"{}\",\"siblings\":[{}],\"pseudonym\":\"{}\",\"answers\":[{}]}}",
                row.n,
                row.path.index,
                hex(&row.public_values),
                hex(&row.commitment),
                hex(&encode_public_values(&row.report)),
                siblings.join(","),
                hex(&row.mine.student_pseudonym),
                answers.join(",")
            ));
        }
    }

    if let Some(p) = fixtures_path {
        std::fs::write(
            p,
            format!(
                "[{}]
",
                fixtures.join(
                    ",
"
                )
            ),
        )
        .expect("write fixtures");
        eprintln!("fixtures written to {p}");
    }
}
