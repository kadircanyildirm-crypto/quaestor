# Roadmap

The goal is not revenue: it is **credibility artifacts** — a working system,
reproducible benchmarks, a technical note, and pilot evidence — that earn
academic attention and collaboration requests.

## Where this stands (2026-07-27)

- [x] `grading-core`: exam model, canonical encode/decode, salted key
      commitments, weighted scoring, post-appeal accepted sets, two cancellation
      policies. 84 tests incl. proptest properties.
- [x] CI: fmt, clippy, tests, `no_std` build on every push; SP1 end-to-end job
      on demand and weekly.
- [x] Public-values ABI owned by one module: 92 bytes single-sheet, 76 bytes
      batch, cross-parse blocked at compile time.
- [x] `zk/program` single-sheet guest — **executes** in the zkVM (42,349 cycles,
      reproduces the expected 80.00% report).
- [x] Batching end to end: `grade_batch`, tagged Merkle tree with leaf-count
      binding, one-pass path cutting (`O(n log n)`; 3085× over per-candidate
      cutting at 16k leaves), `zk/program-batch` guest, `prove-batch` /
      `verify-batch`, published results list, six forgeries refused in CI.
- [x] First container run (2026-07-27, SP1 6.3.1): both guests compile
      unmodified, both execute, the pipeline refuses all six forgeries, and the
      host's Merkle root matches the guest's committed root — the same
      `grade_batch` on x86-64 and RISC-V, byte for byte.
- [x] Measured cost model, 100-question exam: **`cycles ≈ 209,000 + 39,900n`**
      (marginal consistent to 0.6% across two independent intervals). Batching
      therefore cuts per-candidate zkVM work **6.2×** — 248,900 cycles when each
      sheet carries the fixed cost alone, 39,900 when a sitting shares it. At
      SP1's 16.7M-cycle shard that is ~420 candidates per shard.
- [ ] **No proof has been produced yet.** No proof time, no proof size, no
      verify time. Proving was OOM-killed on the 16 GB development laptop and
      stayed killed with every knob turned down; SP1 6.3.1 assumes 24 GB. The
      checks above ran under `SP1_PROVER=mock`, which exercises quaestor's
      logic and none of SP1's cryptography.

That last line is the whole reason this repo is not ready to send to anyone.
Note what it is *not*: not a bug, not a design problem, and no longer a risk
that the pipeline itself is broken. It is one machine away.

## The bar for outreach

The project's claim is that zkVMs make verifiable grading *practical*. Nothing
in the repo currently supports the word "practical". A researcher's first
question is always cost, so the outreach bar is three artifacts, in this order:

1. **A real proof, with numbers.** Proving time, proof size, verify time.
2. **A cost curve.** Proof cost vs sitting size — the graph batching exists to
   flatten. This is the actual contribution: the construction is a
   straightforward application of a zkVM and a commitment (Kroll et al. 2017
   proposed the idea); what is new is the empirical result at exam scale.
3. **A verifier anyone can run.** "Open this tab and check the proof yourself"
   does more work than any email body.

Sending before these exist wastes the one cold email each person will read.

---

## Week 1 (Jul 27 – Aug 2) — the demo moment

The highest-risk week: neither `zk/program-batch` nor the rewritten
`zk/script` has ever been compiled, and the SP1 crates are pinned only to major
`"6"`.

- [ ] `docker run --rm -v "$PWD":/work -v quaestor-cargo-registry:/usr/local/cargo/registry -v quaestor-sp1:/root/.sp1 -w /work rust:1 bash zk/run-in-docker.sh`
      — expect compile errors in the two new files; fix them
- [ ] First single-sheet proof: record proving time, proof size, verify time
- [ ] First batch proof over the 3-candidate demo sitting; confirm all six
      negative cases fail as intended
- [ ] Pin exact SP1 minor versions and **commit `Cargo.lock` for both guest
      crates** — a floating dependency means a non-reproducible ELF, and a
      non-reproducible ELF means a verifying key nobody else can re-derive.
      For a paper, the image ID is part of the claim.
- [ ] Record both programs' verifying keys / image IDs in `zk/README.md`

**Exit criterion:** `demo-proof.bin` and `demo-batch.bin` exist, verify, and
their numbers are written down.

## Week 2 (Aug 3 – Aug 9) — the cost curve

- [x] Synthetic sitting generator (100-question exam, deterministic seed) and
      the cycle half of the curve — `execute` needs none of proving's memory, so
      the shape was measurable on the laptop that cannot produce a proof
- [ ] **Route guest hashing through SP1's SHA-256 precompiles.** Per-candidate
      cost is dominated by hashing, not grading: a 20× increase in question
      count (5 → 100) moved per-candidate cycles only ~33% (30k → 40k). Both
      guests use vanilla `sha2` while SP1 6.3.1 ships `syscall_sha256_compress`
      / `_extend`. Do this *before* the timing sweep — it moves the headline
      number, and re-running the sweep afterwards wastes a day. Gate it on
      byte-identical output against the golden commitments already recorded.
- [ ] Measure at n = 1, 10, 100, 1 000, and as high as the machine allows:
      proving time, proof size, verify time, peak memory
- [ ] `BENCHMARKS.md`: exact machine spec, exact commands, one script that
      reproduces every row. Systems readers check reproducibility before they
      check results.
- [ ] Headline number: **proving cost per candidate**, batched vs per-sheet.
      That is the sentence people quote.
- [ ] GPU run if a CUDA box is available; if not, say CPU-only and report the
      hardware honestly rather than extrapolating.

**Risk:** large sittings may exhaust a laptop. Contingency is a day on a rented
GPU box — budget for it rather than quietly capping n and reporting the cap as
if it were the limit.

**Exit criterion:** a curve that shows per-candidate cost falling with sitting
size, reproducible from a clean checkout.

## Week 3 (Aug 10 – Aug 16) — verifier in the browser

`check_public_values` and `check_batch_inclusion` are already `no_std` and
proof-system-agnostic, so the claim half compiles to WASM as-is. The proof half
is the open question.

- [ ] **Settle this first, it decides the week's shape:** can an SP1 wrapped
      (Groth16/PLONK) proof be verified in WASM, and what does wrapping cost on
      the prover side? Answer before building anything.
- [ ] `crates/quaestor-verify-wasm`: wasm-bindgen over sheet hashing, claim
      checks, and Merkle inclusion
- [ ] Full proof verification in the tab if the answer above allows it;
      otherwise ship the inclusion verifier and state plainly which half runs
      locally and which does not

**Exit criterion:** a candidate pastes their answers and gets ✅/❌ locally,
with nothing uploaded.

## Week 4 (Aug 17 – Aug 23) — the public demo

- [ ] Static page (GitHub Pages): commitment published "before", results list
      and proof "after"
- [ ] One deliberately tampered row that fails **in front of the visitor** —
      the negative case is what shows the check is real
- [ ] Reproduction instructions a stranger can follow end to end
- [ ] README rewritten around the demo link and the headline number

**Exit criterion:** a link that makes the argument without you in the room.

## Week 5 (Aug 24 – Aug 30) — the technical note

- [ ] 4–6 pages: problem, trust model (including what it does *not* prove),
      construction, benchmarks, limitations, related work
- [ ] Related work done properly: *Accountable Algorithms* lineage, zkVM
      systems papers, verifiable-evaluation / zkML work
- [ ] Appeals story written up end to end: cancelled question → new commitment
      → regrade proofs, both commitments on the record
- [ ] PDF in the repo. arXiv is optional at this stage; a linkable note is not.

## Week 6 (Aug 31 – Sep 6) — outreach preparation

- [ ] Target list with a specific reason per person — a mail that could have
      been sent to forty people reads like it was
- [ ] Five-line emails: one sentence on the problem, one on the result, the
      demo link, the benchmark number, one concrete ask
- [ ] Submission target chosen (FAccT, a security workshop, or a systems SRC)
      and its deadline put on the calendar
- [ ] Repo polish pass: no stale claims anywhere, status section honest

**Send: week of Sep 7.**

---

## Parallel track, starting now — pilot

Do **not** leave this to week 6. University calendars have weeks of lead time,
so the ask has to be made while the engineering is still in progress.

- [ ] Week 1: ask one instructor you already know. The ask is deliberately
      small — you need only the answer key, the sheets, and one pre-exam email
      containing the commitment. Zero change to their workflow.
- [ ] Weeks 2–5: whatever their exam calendar allows
- [ ] Fallback if no course lands: replay a *past* exam with pseudonymised data.
      Weaker (the commitment is not genuinely pre-exam and must be labelled as
      such) but far better than synthetic data alone.

A pilot is the one thing on this page nobody else in this space has. If a real
midterm lands, it moves ahead of the technical note in priority.

## Later / research track

- Sharded sittings with a top-level aggregation proof (the 10^6 story)
- Putting the candidate pseudonym in the batch leaf, so an auditor holding no
  answer sheets can bind identities too — an ABI change, deliberately deferred
- Attested AI grading: deterministic pipeline around a committed model eval
- Signed-scan chain of custody for OMR sheets
- Generalisation: scholarship rankings, admission lotteries
