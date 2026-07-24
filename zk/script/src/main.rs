//! ispat host CLI: produce and verify grading proofs.
//!
//! Requires the SP1 toolchain (Linux / WSL2) — see zk/README.md.
//!
//! ```text
//! ispat-cli execute --key key.json --salt <64-hex> --sheet sheet.json
//! ispat-cli prove   --key key.json --salt <64-hex> --sheet sheet.json --out proof.bin
//! ispat-cli verify  --proof proof.bin
//! ```
//!
//! JSON here is a host-side convenience only; what enters the guest (and what
//! commitments are defined over) is always the canonical byte encoding.

use std::fs;
use std::process::ExitCode;

use serde::Deserialize;
use sp1_sdk::{include_elf, ProverClient, SP1ProofWithPublicValues, SP1Stdin};

use grading_core::{
    commit_answer_key, encode_answer_key, encode_answer_sheet, hash_answer_sheet, AnswerKey,
    AnswerSheet, CancelPolicy, KeyEntry,
};

const ELF: &[u8] = include_elf!("ispat-program");

#[derive(Deserialize)]
struct KeyFile {
    exam_id: u64,
    num_choices: u8,
    /// "full_credit" | "redistribute"
    cancel_policy: String,
    questions: Vec<QuestionFile>,
}

#[derive(Deserialize)]
struct QuestionFile {
    id: u32,
    weight: u32,
    accepted: Vec<u8>,
    #[serde(default)]
    cancelled: bool,
}

#[derive(Deserialize)]
struct SheetFile {
    exam_id: u64,
    /// 64 hex chars — an opaque pseudonym, never direct PII.
    student_pseudonym: String,
    answers: Vec<Option<u8>>,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("execute") => run(&args, Mode::Execute),
        Some("prove") => run(&args, Mode::Prove),
        Some("verify") => verify(&args),
        _ => Err("usage: ispat-cli <execute|prove|verify> [--key --salt --sheet --out --proof]".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

enum Mode {
    /// Run the guest without proving — fast sanity check of inputs & outputs.
    Execute,
    /// Produce a real proof and save it.
    Prove,
}

fn run(args: &[String], mode: Mode) -> Result<(), String> {
    let key = load_key(&flag(args, "--key")?)?;
    let salt = parse_hex32(&flag(args, "--salt")?)?;
    let sheet = load_sheet(&flag(args, "--sheet")?)?;

    let expected_commitment = commit_answer_key(&key, &salt);
    let expected_sheet_hash = hash_answer_sheet(&sheet);
    println!("key commitment : {}", hex::encode(expected_commitment));
    println!("sheet hash     : {}", hex::encode(expected_sheet_hash));

    let mut stdin = SP1Stdin::new();
    stdin.write_vec(encode_answer_key(&key));
    stdin.write_vec(salt.to_vec());
    stdin.write_vec(encode_answer_sheet(&sheet));

    let client = ProverClient::from_env();
    match mode {
        Mode::Execute => {
            let (public_values, report) = client
                .execute(ELF, &stdin)
                .run()
                .map_err(|e| format!("execution failed: {e}"))?;
            println!("cycles         : {}", report.total_instruction_count());
            print_public_values(public_values.as_slice())?;
        }
        Mode::Prove => {
            let (pk, vk) = client.setup(ELF);
            let proof = client
                .prove(&pk, &stdin)
                .run()
                .map_err(|e| format!("proving failed: {e}"))?;
            client
                .verify(&proof, &vk)
                .map_err(|e| format!("self-verification failed: {e}"))?;
            print_public_values(proof.public_values.as_slice())?;
            let out = flag(args, "--out").unwrap_or_else(|_| "proof.bin".into());
            proof.save(&out).map_err(|e| format!("saving proof: {e}"))?;
            println!("proof saved    : {out} (self-verified ✓)");
        }
    }
    Ok(())
}

fn verify(args: &[String]) -> Result<(), String> {
    let path = flag(args, "--proof")?;
    let proof = SP1ProofWithPublicValues::load(&path).map_err(|e| format!("loading proof: {e}"))?;
    let client = ProverClient::from_env();
    let (_, vk) = client.setup(ELF);
    client
        .verify(&proof, &vk)
        .map_err(|e| format!("VERIFICATION FAILED: {e}"))?;
    println!("proof valid ✓");
    print_public_values(proof.public_values.as_slice())
}

/// Fixed public-values layout produced by the guest (see zk/README.md):
/// 32B key commitment ‖ 32B sheet hash ‖ 8B exam id ‖ 5×4B counters.
fn print_public_values(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() != 92 {
        return Err(format!("unexpected public values length {}", bytes.len()));
    }
    let u32_at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    println!("── public values ──────────────────────────────");
    println!("key commitment : {}", hex::encode(&bytes[0..32]));
    println!("sheet hash     : {}", hex::encode(&bytes[32..64]));
    println!(
        "exam id        : {}",
        u64::from_le_bytes(bytes[64..72].try_into().unwrap())
    );
    let score_bp = u32_at(72);
    println!("score          : {}.{:02}%", score_bp / 100, score_bp % 100);
    println!(
        "correct/wrong/blank/cancelled: {}/{}/{}/{}",
        u32_at(76),
        u32_at(80),
        u32_at(84),
        u32_at(88)
    );
    Ok(())
}

fn flag(args: &[String], name: &str) -> Result<String, String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .ok_or_else(|| format!("missing {name} <value>"))
}

fn parse_hex32(s: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(s).map_err(|e| format!("salt is not hex: {e}"))?;
    bytes
        .try_into()
        .map_err(|_| "salt must be exactly 32 bytes (64 hex chars)".into())
}

fn load_key(path: &str) -> Result<AnswerKey, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("reading {path}: {e}"))?;
    let file: KeyFile = serde_json::from_str(&raw).map_err(|e| format!("parsing {path}: {e}"))?;
    let cancel_policy = match file.cancel_policy.as_str() {
        "full_credit" => CancelPolicy::FullCredit,
        "redistribute" => CancelPolicy::Redistribute,
        other => return Err(format!("unknown cancel_policy '{other}'")),
    };
    Ok(AnswerKey {
        exam_id: file.exam_id,
        num_choices: file.num_choices,
        cancel_policy,
        entries: file
            .questions
            .into_iter()
            .map(|q| KeyEntry {
                question_id: q.id,
                weight: q.weight,
                accepted: q.accepted,
                cancelled: q.cancelled,
            })
            .collect(),
    })
}

fn load_sheet(path: &str) -> Result<AnswerSheet, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("reading {path}: {e}"))?;
    let file: SheetFile = serde_json::from_str(&raw).map_err(|e| format!("parsing {path}: {e}"))?;
    Ok(AnswerSheet {
        exam_id: file.exam_id,
        student_pseudonym: parse_hex32(&file.student_pseudonym)?,
        answers: file.answers,
    })
}
