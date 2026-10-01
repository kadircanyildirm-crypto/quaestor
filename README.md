# quaestor

[![CI](https://github.com/kadircanyildirm-crypto/quaestor/actions/workflows/ci.yml/badge.svg?branch=main&event=push)](https://github.com/kadircanyildirm-crypto/quaestor/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**Verifiable grading.** quaestor proves that an exam score was computed from
the answer key the institution committed to before the exam, and from the
candidate's own answers, without revealing the key.

The grading engine runs inside the [SP1](https://github.com/succinctlabs/sp1)
zkVM. Before the exam, the institution publishes a salted SHA-256 commitment to
its answer key. After the exam, it grades the entire sitting in one zkVM
execution and publishes a single proof together with a results list. Using only
public data and their own answer sheet, each candidate can confirm that the
score published for them is the score that was proven.

> [!NOTE]
> The grading core, both zkVM guests and the complete verification pipeline are
> implemented and tested. A real SP1 proof has not been generated yet: proving
> requires at least 24 GB of memory. See [Status](#status).

## Walkthrough

The animations follow one exam from commitment to verification, then show
three forgeries being rejected. They are rendered from a WebAssembly build of
`grading-core` running on [`examples/demo-exam`](examples/demo-exam), so every
hash, score, Merkle path and verdict on screen is real output of this code. The
SP1 proof itself is illustrated, not generated.

Commitments are drawn as seals: the 16 rays and 16 dots encode the 32 bytes of
the digest. Sheet hashes are drawn as the timing marks along a sheet's edge.
Equal digests produce identical figures, and any change to the input produces a
completely different one.

### 1. Commitment

![The answer key and a secret salt are sealed; only the commitment is posted publicly](docs/media/1-seal.gif)

Before the exam, the institution publishes `C = commit_answer_key(K, s)`, a
SHA-256 digest of the canonically encoded answer key `K` and a secret 32-byte
salt `s`. The salt makes the commitment hiding, since answer keys have too
little entropy to survive a brute-force search on their own. SHA-256 makes it
binding. Demo commitment: `d4f3126b…9bd51d0c`.

### 2. The sitting

![Four candidates mark their answer sheets; each sheet's hash appears as timing marks](docs/media/2-exam.gif)

Each sheet is identified by a 32-byte pseudonym rather than a name.
`hash_answer_sheet` computes its sheet hash `H`, which the candidate can
recompute at any time from their own copy of their answers. In the demo exam,
question 3 accepts two answers after an appeal and question 5 is cancelled under
the full-credit policy.

### 3. Grading and proof

![The key and all sheets enter the zkVM; reports, a Merkle tree and one proof come out](docs/media/3-prove.gif)

The batch guest (`zk/program-batch`) runs `grade_batch(K, s, sheets)` inside
SP1. The key is validated and committed once per sitting. Each sheet is scored,
each report is hashed into a Merkle leaf, and the root is bound to the number of
candidates. The guest commits 76 bytes of public values (`C`, the root, the
exam id and the candidate count), so one proof covers the whole sitting.
Measured cost on a 100-question exam is about 214,000 + 39,800 × n zkVM cycles
([benchmarks](docs/BENCHMARKS.md)).

### 4. Verification

![The candidate's report is checked against the posted commitment, their own sheet hash, and the proven Merkle root](docs/media/4-check.gif)

The candidate holds the proof, the published results list, the pre-exam
commitment and their own answer sheet. Once the proof verifies,
`check_batch_inclusion` confirms three things: the proof is under the published
commitment, the report carries the candidate's sheet hash, and the report's
inclusion path reaches the proven root. This check needs no zkVM and costs a
few dozen hash evaluations. `quaestor-cli verify-batch` runs the full sequence.

### 5. Forgery: answer key changed after the exam

![The institution edits the key, re-grades and re-proves; the new commitment does not match the posted one](docs/media/5-cheat-key.gif)

The institution changes the answer to question 4, re-grades the sitting and
proves it again. The new proof is valid and the candidate's score rises to 100%,
but the edited key has a different commitment (`b361aaa4…`).
**Rejected:** `CommitmentMismatch`.

### 6. Forgery: another candidate's result

![A genuine result belonging to another candidate is presented; its sheet hash does not match](docs/media/6-cheat-swap.gif)

The candidate is handed a genuine, proven result that belongs to someone else.
Its sheet hash is not the hash of the candidate's answers.
**Rejected:** `SheetHashMismatch`.

### 7. Forgery: score raised after proving

![A published 60% is changed to 100%; the edited report no longer hashes to a leaf of the proven tree](docs/media/7-cheat-raise.gif)

After the proof is published, the results list shows a 60% candidate as 100%.
The edited report hashes to a different leaf (`8a02095a…` becomes `8ae3296e…`),
and its inclusion path no longer reaches the proven root.
**Rejected:** `NotInBatch`.

## Motivation

When a candidate appeals a grade, the institution's answer is usually that the
system computed it. Neither the candidate nor an auditor or court can verify that

- the answer key was not changed after the exam,
- the published scoring rules (weights, cancelled questions, accepted answers)
  were actually applied, and
- the candidate's own answer sheet was the one graded.

Answer-key and grade-tampering incidents recur in national examination systems.
Policy work on algorithmic accountability (OECD, Ada Lovelace Institute) calls
for transparency registers and audits. quaestor offers a stronger guarantee: a
proof. The approach follows Kroll et al., *Accountable Algorithms* (University
of Pennsylvania Law Review, 2017); general-purpose zkVMs make it practical.

## The proven statement

For a sitting of `n` candidates, the batch guest proves:

> There exist an answer key `K` and a salt `s` such that `commit(K, s) = C`, and
> grading `K` against the sitting's answer sheets yields reports `R₁ … Rₙ` whose
> Merkle root, bound to `n`, is `root`.

**Public:** `C` (published before the exam), `root`, the exam id and `n`. Each
report `Rᵢ` and its inclusion path are published in the results list.
**Private:** `K` and `s`.

A single-sheet guest (`zk/program`) proves the per-candidate form of the same
statement, with public values `(C, H, R)`.

### What a valid proof establishes

- The scores were produced by the published grading program. The guest's image
  ID pins the exact code.
- The answer key used is the one committed before the exam.
- Each report was computed from the answer sheet with hash `H`, so a candidate
  can detect substitution of their answers.

### What it does not establish

- **Chain of custody.** The proof covers the recorded answers, not the paper
  sheet. Scanning is outside the proof boundary.
- **Key correctness.** The proof shows that the committed key was applied, not
  that it is academically correct. Appeals still exist; their effects become
  visible as new commitments.
- **Row ownership, for auditors.** A Merkle leaf contains the report but not the
  pseudonym. Candidates can detect a relabelled row with their own sheet; an
  auditor holding no sheets cannot.

The full design and trust model are in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Status

| Component | State |
|---|---|
| `grading-core` | Done. Deterministic `no_std` grading engine with canonical encodings, salted commitments, weighted scoring, multiple accepted answers and two cancellation policies. 84 tests, including property-based tests for commitment binding, encoding round-trips and adversarial decoding. |
| Public-values ABI | Done. Fixed layouts of 92 bytes per sheet and 76 bytes per sitting, defined in one module shared by guests and verifiers. |
| SP1 guests and CLI | Done. Both guests build under SP1 6.3.1 (pinned exactly, because the image ID is part of the claim) and execute in the zkVM: 42,518 cycles for one demo sheet, 119,700 for a three-candidate sitting. |
| Batching | Done. One proof per sitting, plus a `log₂(n)` inclusion path per candidate. |
| Forgery rejection | Done. The end-to-end pipeline (`zk/run-in-docker.sh`) rejects six forgeries: a proof checked against another candidate's answers, an unpublished commitment, a candidate absent from the sitting, a score raised after proving, the edited list re-audited in full, and swapped identities. |
| Determinism | Verified. The Merkle root computed by the host equals the root committed by the guest, byte for byte, on x86-64 and RISC-V. |
| Proof generation | **Blocked on hardware.** SP1 6.3.1 requires at least 24 GB of memory, and proving was killed for lack of memory on a 16 GB machine. All end-to-end runs so far used `SP1_PROVER=mock`, which exercises quaestor's logic but not SP1's cryptography. |
| Planned | Proving time, proof size and verification benchmarks (CPU and GPU), a browser verifier, and a pilot with a real course. See [docs/ROADMAP.md](docs/ROADMAP.md). |

## Getting started

The grading core is plain Rust and needs no zkVM toolchain:

```sh
cargo test
```

The proving layer builds on Linux or WSL2 (see [zk/README.md](zk/README.md)),
or on any platform through Docker. On machines with less than 24 GB of memory,
add `-e SP1_PROVER=mock` to run the full pipeline without SP1's cryptography:

```sh
docker run --rm -v "$PWD":/work \
  -v quaestor-cargo-registry:/usr/local/cargo/registry \
  -v quaestor-sp1:/root/.sp1 \
  -w /work rust:1 bash zk/run-in-docker.sh
```

The host CLI, `quaestor-cli`, covers both sides of the protocol:

```sh
quaestor-cli prove-batch  --key key.json --salt <hex> --sheets sheets/ \
                          --out batch.bin --manifest sitting.json
quaestor-cli verify-batch --proof batch.bin --manifest sitting.json \
                          --commitment <hex> --sheet my-sheet.json
```

## Repository layout

```text
crates/grading-core/   grading engine: model, encodings, commitments, scoring, batching, public values
zk/program/            SP1 guest for a single answer sheet
zk/program-batch/      SP1 guest for a whole sitting
zk/script/             quaestor-cli: execute, prove and verify, per sheet and per sitting
examples/demo-exam/    demo answer key and answer sheets
bench/                 cycle-count benchmark scripts
docs/                  architecture, benchmarks and roadmap
```

## Design constraints

Everything in `grading-core` must run unchanged inside a zkVM guest:

1. **Determinism.** Scores are integer basis points. No floating point, no
   map iteration order, no clocks, no randomness.
2. **`no_std` + `alloc`.** Guests have no operating system.
3. **Hand-written canonical encodings.** Commitment security must not depend on
   the stability of a serialization library.

## Non-goals

No blockchain and no tokens. Proving and verification are ordinary
computations, and publishing a commitment requires nothing more than the
institution's website or any append-only transparency log.

## About the name

A *quaestor* was the Roman magistrate responsible for auditing the public
accounts. The word comes from *quaestio*, "an inquiry".

## License

Dual-licensed under the [MIT License](LICENSE-MIT) or the
[Apache License 2.0](LICENSE-APACHE), at your option.
