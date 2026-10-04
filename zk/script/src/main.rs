//! quaestor host CLI: produce and verify grading proofs.
//!
//! Requires the SP1 toolchain (Linux / WSL2) — see zk/README.md.
//!
//! ```text
//! # one sheet, one proof — the minimal reference
//! quaestor-cli execute --key key.json --salt <64-hex> --sheet sheet.json
//! quaestor-cli prove   --key key.json --salt <64-hex> --sheet sheet.json --out proof.bin
//! quaestor-cli verify  --proof proof.bin --commitment <64-hex> [--sheet sheet.json]
//!
//! # one sitting, one proof — what a real exam uses
//! quaestor-cli execute-batch --key key.json --salt <64-hex> --sheet a.json --sheet b.json
//! quaestor-cli prove-batch   --key key.json --salt <64-hex> --sheets dir/ \
//!                            --out batch.bin --manifest sitting.json
//! quaestor-cli verify-batch  --proof batch.bin --manifest sitting.json \
//!                            --commitment <64-hex> [--sheet mine.json]
//! ```
//!
//! JSON here is a host-side convenience only; what enters the guest (and what
//! commitments are defined over) is always the canonical byte encoding.
//!
//! `verify` takes the pre-exam commitment on purpose. A proof that merely
//! verifies says "*some* sheet was graded under *some* key" — a claim a
//! tampering institution can satisfy trivially. The commitment (and, for a
//! student, their own sheet) is what turns it into a statement about them.

use std::fs;
use std::process::ExitCode;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use sp1_sdk::blocking::{ProveRequest, Prover, ProverClient, SP1ProofMode};
use sp1_sdk::{include_elf, Elf, ProvingKey, SP1ProofWithPublicValues, SP1Stdin};

use grading_core::{
    check_batch_inclusion, check_public_values, commit_answer_key, decode_batch_public_values,
    decode_public_values, encode_answer_key, encode_answer_sheet, grade_batch, hash_answer_sheet,
    AnswerKey, AnswerSheet, BatchOutcome, CancelPolicy, KeyEntry, MerklePath, ScoreReport,
};

/// `include_elf!` yields `Elf::Static` over embedded bytes; constructing it
/// per call sidesteps any Clone/Copy assumptions about the `Elf` type.
fn elf() -> Elf {
    include_elf!("quaestor-program")
}

/// The batch guest is a *separate program*, so it has a separate verifying key.
/// That is the property that makes a batch proof unusable where a single-sheet
/// proof is expected, and vice versa — not a flag in the public values that
/// somebody could forget to check.
fn batch_elf() -> Elf {
    include_elf!("quaestor-batch-program")
}

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

const USAGE: &str = "usage:\n  \
     quaestor-cli execute       --key <f> --salt <64-hex> --sheet <f>\n  \
     quaestor-cli prove         --key <f> --salt <64-hex> --sheet <f> [--out proof.bin] [--mode <m>]\n  \
     quaestor-cli verify        --proof <f> --commitment <64-hex> [--sheet <f>]\n  \
     quaestor-cli execute-batch --key <f> --salt <64-hex> (--sheet <f>).. [--sheets <dir>]\n  \
     quaestor-cli prove-batch   --key <f> --salt <64-hex> (--sheet <f>).. [--sheets <dir>]\n                             \
       [--out batch.bin] [--manifest sitting.json] [--mode <m>]\n  \
     quaestor-cli verify-batch  --proof <f> --manifest <f> --commitment <64-hex> [--sheet <f>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("execute") => run(&args, Mode::Execute),
        Some("prove") => run(&args, Mode::Prove),
        Some("verify") => verify(&args),
        Some("execute-batch") => run_batch(&args, Mode::Execute),
        Some("prove-batch") => run_batch(&args, Mode::Prove),
        Some("verify-batch") => verify_batch(&args),
        _ => Err(USAGE.into()),
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
    let salt = parse_hex32(&flag(args, "--salt")?, "salt")?;
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
                .execute(elf(), stdin)
                .run()
                .map_err(|e| format!("execution failed: {e}"))?;
            println!("cycles         : {}", report.total_instruction_count());
            print_public_values(public_values.as_slice())?;
        }
        Mode::Prove => {
            let (kind, kind_name) = proof_mode(args)?;
            let t = Instant::now();
            let pk = client.setup(elf()).map_err(|e| format!("setup failed: {e}"))?;
            println!("setup          : {:.1} s", t.elapsed().as_secs_f64());
            let t = Instant::now();
            let proof = client
                .prove(&pk, stdin)
                .mode(kind)
                .run()
                .map_err(|e| format!("proving failed: {e}"))?;
            println!(
                "proving        : {:.1} s ({kind_name})",
                t.elapsed().as_secs_f64()
            );
            client
                .verify(&proof, pk.verifying_key(), None)
                .map_err(|e| format!("self-verification failed: {e}"))?;
            print_public_values(proof.public_values.as_slice())?;
            let out = flag(args, "--out").unwrap_or_else(|_| "proof.bin".into());
            proof.save(&out).map_err(|e| format!("saving proof: {e}"))?;
            println!(
                "proof saved    : {out}, {} bytes (self-verified ✓)",
                file_len(&out)
            );
        }
    }
    Ok(())
}

/// The student's side of the protocol, and the only command that has to be
/// trustworthy: verify the proof, then check that what it proves is a statement
/// about *the committed key* and *this student's answers*.
fn verify(args: &[String]) -> Result<(), String> {
    let path = flag(args, "--proof")?;
    let expected_commitment = parse_hex32(&flag(args, "--commitment")?, "commitment")?;
    // Optional: an auditor may legitimately check a proof against the published
    // commitment without holding anyone's answers. A student always passes it —
    // that is what binds the proof to them and not to some other candidate.
    let expected_sheet_hash = match flag(args, "--sheet") {
        Ok(p) => Some(hash_answer_sheet(&load_sheet(&p)?)),
        Err(_) => None,
    };

    let proof = SP1ProofWithPublicValues::load(&path).map_err(|e| format!("loading proof: {e}"))?;
    let client = ProverClient::from_env();
    let pk = client.setup(elf()).map_err(|e| format!("setup failed: {e}"))?;
    // Timed alone: the setup above re-derives the proving key and dwarfs this.
    let t = Instant::now();
    client
        .verify(&proof, pk.verifying_key(), None)
        .map_err(|e| format!("PROOF INVALID: {e}"))?;
    println!(
        "verify time    : {:.1} ms (SP1 proof only)",
        t.elapsed().as_secs_f64() * 1e3
    );

    // A cryptographically valid proof of the wrong statement is exactly the
    // attack this system exists to catch, so a mismatch here is a hard failure
    // with a non-zero exit code — never a printed warning.
    let report = check_public_values(
        proof.public_values.as_slice(),
        &expected_commitment,
        expected_sheet_hash.as_ref(),
    )
    .map_err(|e| format!("REJECTED — {e}"))?;

    print_report(&report);
    println!("proof valid                      ✓");
    println!("grades the pre-exam committed key ✓");
    match expected_sheet_hash {
        Some(_) => println!("grades your answer sheet          ✓"),
        None => println!(
            "answer sheet                      — not checked (pass --sheet to bind \
             this proof to a specific student)"
        ),
    }
    Ok(())
}

// ── batch: one sitting, one proof ───────────────────────────────────────────

/// What the institution publishes alongside the batch proof: the results list.
///
/// Nothing here is trusted. The root and the commitment a verifier checks
/// against come out of the *proof*; these copies exist so a mismatch can be
/// reported as "the published list is not the proven sitting" instead of
/// surfacing as an inscrutable inclusion failure.
#[derive(Serialize, Deserialize)]
struct Manifest {
    exam_id: u64,
    key_commitment: String,
    batch_root: String,
    leaf_count: u32,
    entries: Vec<ManifestEntry>,
}

/// One published row: a candidate's report plus the path that ties it to the
/// root. The path is what makes the row checkable rather than merely asserted.
#[derive(Serialize, Deserialize)]
struct ManifestEntry {
    index: u32,
    student_pseudonym: String,
    sheet_hash: String,
    score_bp: u32,
    correct: u32,
    wrong: u32,
    blank: u32,
    cancelled: u32,
    path: Vec<String>,
}

impl ManifestEntry {
    /// Rebuild the report exactly as it was hashed into the tree.
    ///
    /// `exam_id` and `key_commitment` come from the *manifest*, not the proof,
    /// on purpose: a manifest that misstates either produces a report whose
    /// leaf is not in the proven root, and the inclusion check catches it.
    /// Filling these in from the proof instead would launder the lie.
    fn report(&self, exam_id: u64, key_commitment: [u8; 32]) -> Result<ScoreReport, String> {
        Ok(ScoreReport {
            exam_id,
            key_commitment,
            sheet_hash: parse_hex32(&self.sheet_hash, "sheet hash")?,
            score_bp: self.score_bp,
            correct: self.correct,
            wrong: self.wrong,
            blank: self.blank,
            cancelled: self.cancelled,
        })
    }

    fn merkle_path(&self) -> Result<MerklePath, String> {
        Ok(MerklePath {
            index: self.index,
            siblings: self
                .path
                .iter()
                .map(|s| parse_hex32(s, "path sibling"))
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

fn build_manifest(
    outcome: &BatchOutcome,
    sheets: &[(String, AnswerSheet)],
) -> Result<Manifest, String> {
    let pv = outcome.public_values();
    // Cut every path in one pass. Asking for them one at a time rebuilds the
    // whole tree per candidate — `O(n²)` hashing on precisely the code path
    // that exists to serve a sitting of 10^5–10^6.
    let paths = outcome
        .paths()
        .map_err(|e| format!("cutting inclusion paths: {e:?}"))?;

    Ok(Manifest {
        exam_id: pv.exam_id,
        key_commitment: hex::encode(pv.key_commitment),
        batch_root: hex::encode(pv.batch_root),
        leaf_count: pv.leaf_count,
        entries: outcome
            .reports
            .iter()
            .zip(&paths)
            .enumerate()
            .map(|(index, (r, path))| ManifestEntry {
                index: index as u32,
                student_pseudonym: hex::encode(sheets[index].1.student_pseudonym),
                sheet_hash: hex::encode(r.sheet_hash),
                score_bp: r.score_bp,
                correct: r.correct,
                wrong: r.wrong,
                blank: r.blank,
                cancelled: r.cancelled,
                path: path.siblings.iter().map(hex::encode).collect(),
            })
            .collect(),
    })
}

fn run_batch(args: &[String], mode: Mode) -> Result<(), String> {
    let key = load_key(&flag(args, "--key")?)?;
    let salt = parse_hex32(&flag(args, "--salt")?, "salt")?;
    let sheets = load_sitting(args)?;

    // Grade the sitting here as well as in the guest. Not redundancy: the host
    // needs the reports and paths to publish the manifest, and comparing its
    // root against the guest's committed root is what proves the two agree.
    let sitting: Vec<AnswerSheet> = sheets.iter().map(|(_, s)| s.clone()).collect();
    let outcome =
        grade_batch(&key, &salt, &sitting).map_err(|e| format!("grading the sitting: {e}"))?;
    let expected = outcome.public_values();

    println!("sitting        : {} candidates", sheets.len());
    for (index, ((path, _), report)) in sheets.iter().zip(&outcome.reports).enumerate() {
        println!(
            "  [{index:>4}] {path:<44} {}.{:02}%",
            report.score_bp / 100,
            report.score_bp % 100
        );
    }
    println!("key commitment : {}", hex::encode(expected.key_commitment));
    println!("batch root     : {}", hex::encode(expected.batch_root));

    let mut stdin = SP1Stdin::new();
    stdin.write_vec(encode_answer_key(&key));
    stdin.write_vec(salt.to_vec());
    stdin.write_vec((sheets.len() as u32).to_le_bytes().to_vec());
    for (_, sheet) in &sheets {
        stdin.write_vec(encode_answer_sheet(sheet));
    }

    let client = ProverClient::from_env();
    let committed = match mode {
        Mode::Execute => {
            let (public_values, exec) = client
                .execute(batch_elf(), stdin)
                .run()
                .map_err(|e| format!("execution failed: {e}"))?;
            println!("cycles         : {}", exec.total_instruction_count());
            println!(
                "cycles/sheet   : {}",
                exec.total_instruction_count() / sheets.len() as u64
            );
            public_values.as_slice().to_vec()
        }
        Mode::Prove => {
            let (kind, kind_name) = proof_mode(args)?;
            let t = Instant::now();
            let pk = client
                .setup(batch_elf())
                .map_err(|e| format!("setup failed: {e}"))?;
            println!("setup          : {:.1} s", t.elapsed().as_secs_f64());
            let t = Instant::now();
            let proof = client
                .prove(&pk, stdin)
                .mode(kind)
                .run()
                .map_err(|e| format!("proving failed: {e}"))?;
            println!(
                "proving        : {:.1} s ({kind_name})",
                t.elapsed().as_secs_f64()
            );
            client
                .verify(&proof, pk.verifying_key(), None)
                .map_err(|e| format!("self-verification failed: {e}"))?;
            let out = flag(args, "--out").unwrap_or_else(|_| "batch.bin".into());
            proof.save(&out).map_err(|e| format!("saving proof: {e}"))?;
            println!(
                "proof saved    : {out}, {} bytes (self-verified ✓)",
                file_len(&out)
            );
            proof.public_values.as_slice().to_vec()
        }
    };

    // If the host and the guest disagree about the root, every inclusion path
    // the host is about to publish is against a root nobody proved. Fail loudly
    // rather than emit a manifest that cannot be verified.
    let guest = decode_batch_public_values(&committed)
        .map_err(|e| format!("guest public values: {e}"))?;
    if guest != expected {
        return Err(format!(
            "host and guest disagree about the sitting:\n  \
               host  root {} count {}\n  \
               guest root {} count {}",
            hex::encode(expected.batch_root),
            expected.leaf_count,
            hex::encode(guest.batch_root),
            guest.leaf_count
        ));
    }
    println!("host root matches the guest's committed root ✓");

    let manifest_path = flag(args, "--manifest").unwrap_or_else(|_| "sitting.json".into());
    let manifest = build_manifest(&outcome, &sheets)?;
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest)
            .map_err(|e| format!("serializing manifest: {e}"))?,
    )
    .map_err(|e| format!("writing {manifest_path}: {e}"))?;
    println!("results list   : {manifest_path}");
    Ok(())
}

/// The student's side of a batched sitting.
///
/// Three things have to hold, and all three are checked against the *proof*:
/// the proof is valid, the sitting it proves is under the commitment published
/// before the exam, and the row the institution published for this student is
/// genuinely one of the leaves that proof committed to. Skipping the third is
/// the interesting failure mode of batching — the proof stays perfectly valid
/// while the number next to a name is anything at all.
fn verify_batch(args: &[String]) -> Result<(), String> {
    let proof_path = flag(args, "--proof")?;
    let expected_commitment = parse_hex32(&flag(args, "--commitment")?, "commitment")?;
    let manifest: Manifest = {
        let path = flag(args, "--manifest")?;
        let raw = fs::read_to_string(&path).map_err(|e| format!("reading {path}: {e}"))?;
        serde_json::from_str(&raw).map_err(|e| format!("parsing {path}: {e}"))?
    };
    // The list's own claim about which key graded it. Never substituted with
    // the proof's copy: a report rebuilt from the proof's commitment would
    // verify no matter what the institution published here.
    let listed_commitment = parse_hex32(&manifest.key_commitment, "key commitment")?;

    let mine = match flags(args, "--sheet").as_slice() {
        [] => None,
        [one] => Some(load_sheet(one)?),
        many => {
            return Err(format!(
                "verify-batch checks one candidate at a time, but {} sheets were given",
                many.len()
            ))
        }
    };

    let proof =
        SP1ProofWithPublicValues::load(&proof_path).map_err(|e| format!("loading proof: {e}"))?;
    let client = ProverClient::from_env();
    let pk = client
        .setup(batch_elf())
        .map_err(|e| format!("setup failed: {e}"))?;
    // Timed alone: the setup above re-derives the proving key and dwarfs this.
    let t = Instant::now();
    client
        .verify(&proof, pk.verifying_key(), None)
        .map_err(|e| format!("PROOF INVALID: {e}"))?;
    println!(
        "verify time    : {:.1} ms (SP1 proof only)",
        t.elapsed().as_secs_f64() * 1e3
    );

    let pv = decode_batch_public_values(proof.public_values.as_slice())
        .map_err(|e| format!("REJECTED — {e}"))?;

    // The manifest restates the sitting's identity; any disagreement with the
    // proof means the published list belongs to some other sitting.
    let restated = (
        manifest.key_commitment.to_lowercase(),
        manifest.batch_root.to_lowercase(),
        manifest.exam_id,
        manifest.leaf_count,
    );
    let proven = (
        hex::encode(pv.key_commitment),
        hex::encode(pv.batch_root),
        pv.exam_id,
        pv.leaf_count,
    );
    if restated != proven {
        return Err(format!(
            "REJECTED — the published results list is not the sitting this proof is about:\n  \
               list  root {} count {} exam {}\n  \
               proof root {} count {} exam {}",
            restated.1, restated.3, restated.2, proven.1, proven.3, proven.2
        ));
    }

    println!("── sitting ────────────────────────────────────");
    println!("key commitment : {}", hex::encode(pv.key_commitment));
    println!("batch root     : {}", hex::encode(pv.batch_root));
    println!("exam id        : {}", pv.exam_id);
    println!("candidates     : {}", pv.leaf_count);

    let public_values = proof.public_values.as_slice();
    match &mine {
        Some(sheet) => {
            let sheet_hash = hash_answer_sheet(sheet);
            let wanted = hex::encode(sheet_hash);
            let entry = manifest
                .entries
                .iter()
                .find(|e| e.sheet_hash.eq_ignore_ascii_case(&wanted))
                .ok_or_else(|| {
                    "REJECTED — your answer sheet does not appear in this published sitting"
                        .to_string()
                })?;

            let report = entry.report(manifest.exam_id, listed_commitment)?;
            check_batch_inclusion(
                public_values,
                &expected_commitment,
                &report,
                &entry.merkle_path()?,
                Some(&sheet_hash),
            )
            .map_err(|e| format!("REJECTED — {e}"))?;

            // Inclusion binds the *score* to these answers. It says nothing
            // about the name on the row: the pseudonym is not in the leaf, so
            // a published list can carry a correct, proven score under the
            // wrong identifier. What does bind it is the sheet hash, whose
            // preimage contains the pseudonym alongside the answers — so the
            // one party able to check the identity column is the candidate
            // holding the answers, and that check happens here.
            let listed_pseudonym = parse_hex32(&entry.student_pseudonym, "student pseudonym")?;
            if listed_pseudonym != sheet.student_pseudonym {
                return Err(format!(
                    "REJECTED — this row is published under pseudonym {}, but these \
                     answers were submitted under {}",
                    hex::encode(listed_pseudonym),
                    hex::encode(sheet.student_pseudonym)
                ));
            }

            println!("── your result ────────────────────────────────");
            println!("position       : {} of {}", entry.index, pv.leaf_count);
            print_report(&report);
            println!("proof valid                         ✓");
            println!("grades the pre-exam committed key    ✓");
            println!("your report is in the proven sitting ✓");
            println!("grades your answer sheet             ✓");
            println!("published under your pseudonym       ✓");
        }
        None => {
            audit_every_row(
                &manifest,
                listed_commitment,
                public_values,
                &expected_commitment,
                pv.leaf_count,
            )?;
            println!("proof valid                           ✓");
            println!("grades the pre-exam committed key      ✓");
            println!(
                "all {} published rows are in the proven sitting ✓",
                pv.leaf_count
            );
            // Said plainly rather than left to inference: an auditor has just
            // established that every *score* in the list was proven, and
            // nothing at all about which candidate each belongs to. The
            // pseudonym is bound only through the sheet hash, whose preimage
            // needs that candidate's answers — which an auditor does not hold.
            println!(
                "identity column                       — NOT checked: inclusion binds each \
                 score, not the pseudonym printed beside it. Only the candidate, holding \
                 their own answers, can check that (pass --sheet)."
            );
        }
    }
    Ok(())
}

/// Auditor mode: check that the published list *is* the sitting — every row is
/// a proven leaf, and every position is published exactly once.
///
/// The position check is not decoration. Without it a list could publish one
/// candidate's row twice and quietly omit another's; every row would still open
/// against the root, and the missing candidate would have no way to tell that
/// the list, rather than their grade, is what went wrong.
fn audit_every_row(
    manifest: &Manifest,
    listed_commitment: [u8; 32],
    public_values: &[u8],
    expected_commitment: &[u8; 32],
    leaf_count: u32,
) -> Result<(), String> {
    if manifest.entries.len() as u32 != leaf_count {
        return Err(format!(
            "REJECTED — the proof commits to {leaf_count} candidates but the list publishes {}",
            manifest.entries.len()
        ));
    }
    let mut seen = vec![false; leaf_count as usize];
    for entry in &manifest.entries {
        let slot = seen.get_mut(entry.index as usize).ok_or_else(|| {
            format!("REJECTED — row claims position {} of {leaf_count}", entry.index)
        })?;
        if *slot {
            return Err(format!(
                "REJECTED — position {} is published twice",
                entry.index
            ));
        }
        *slot = true;

        let report = entry.report(manifest.exam_id, listed_commitment)?;
        check_batch_inclusion(
            public_values,
            expected_commitment,
            &report,
            &entry.merkle_path()?,
            None,
        )
        .map_err(|e| format!("REJECTED — row at position {}: {e}", entry.index))?;
    }
    Ok(())
}

// ── input plumbing ──────────────────────────────────────────────────────────

fn print_public_values(bytes: &[u8]) -> Result<(), String> {
    let report = decode_public_values(bytes).map_err(|e| e.to_string())?;
    print_report(&report);
    Ok(())
}

fn print_report(r: &ScoreReport) {
    println!("── public values ──────────────────────────────");
    println!("key commitment : {}", hex::encode(r.key_commitment));
    println!("sheet hash     : {}", hex::encode(r.sheet_hash));
    println!("exam id        : {}", r.exam_id);
    println!(
        "score          : {}.{:02}%",
        r.score_bp / 100,
        r.score_bp % 100
    );
    println!(
        "correct/wrong/blank/cancelled: {}/{}/{}/{}",
        r.correct, r.wrong, r.blank, r.cancelled
    );
}

/// `--mode core|compressed|groth16|plonk`, core when absent. Core is the
/// fastest to produce and grows with the run; compressed is constant-size;
/// groth16 and plonk wrap it into a few hundred bytes that a browser or a
/// contract can verify (groth16 needs about 14 GB on top, plonk about 64 GB).
fn proof_mode(args: &[String]) -> Result<(SP1ProofMode, &'static str), String> {
    match flag(args, "--mode").as_deref() {
        Err(_) | Ok("core") => Ok((SP1ProofMode::Core, "core")),
        Ok("compressed") => Ok((SP1ProofMode::Compressed, "compressed")),
        Ok("groth16") => Ok((SP1ProofMode::Groth16, "groth16")),
        Ok("plonk") => Ok((SP1ProofMode::Plonk, "plonk")),
        Ok(other) => Err(format!(
            "unknown --mode {other}: expected core, compressed, groth16 or plonk"
        )),
    }
}

fn file_len(path: &str) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn flag(args: &[String], name: &str) -> Result<String, String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .ok_or_else(|| format!("missing {name} <value>"))
}

/// Every occurrence of a repeatable flag, in command-line order.
fn flags(args: &[String], name: &str) -> Vec<String> {
    args.iter()
        .enumerate()
        .filter(|(_, a)| a.as_str() == name)
        .filter_map(|(i, _)| args.get(i + 1).cloned())
        .collect()
}

/// Collect the sitting's answer sheets in a **deterministic** order.
///
/// Order is not cosmetic here: it is the leaf order, so it decides the root and
/// every candidate's inclusion path. Explicit `--sheet` flags come first in the
/// order given, then each `--sheets <dir>`'s `*.json` sorted by file name — so
/// re-running the same command reproduces the same sitting, which is what lets
/// anyone else re-derive the published root from the same inputs.
fn load_sitting(args: &[String]) -> Result<Vec<(String, AnswerSheet)>, String> {
    let mut paths = flags(args, "--sheet");
    for dir in flags(args, "--sheets") {
        let mut in_dir: Vec<String> = fs::read_dir(&dir)
            .map_err(|e| format!("reading {dir}: {e}"))?
            .map(|entry| entry.map_err(|e| format!("reading {dir}: {e}")))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|entry| entry.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        in_dir.sort();
        paths.extend(in_dir);
    }

    if paths.is_empty() {
        return Err("a sitting needs at least one --sheet <f> or --sheets <dir>".into());
    }
    paths
        .into_iter()
        .map(|p| load_sheet(&p).map(|s| (p, s)))
        .collect()
}

fn parse_hex32(s: &str, what: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(s).map_err(|e| format!("{what} is not hex: {e}"))?;
    bytes
        .try_into()
        .map_err(|_| format!("{what} must be exactly 32 bytes (64 hex chars)"))
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
        student_pseudonym: parse_hex32(&file.student_pseudonym, "student pseudonym")?,
        answers: file.answers,
    })
}
