# Benchmarks

> **Update, 2026-10-04:** the first real proofs exist. The demo exam proves on a
> 4-core, 16 GB machine with swap in about 82 s (core), 5 min (compressed) and
> 32–38 min (groth16, 1.8 KB). Details and verifying keys are in
> [`../proofs/2026-10-04`](../proofs/2026-10-04). The sections below describe
> cycle counts, which remain the only figures at sitting scale.

## What this measures — and what it does not

**Measured:** zkVM *cycle counts* — how much work the guest performs — as a
function of sitting size.

**Not measured: proving time, proof size, verification time.** Not one proof has
been generated yet. SP1 6.3.1 assumes a 24 GB machine (`DEFAULT_MEMORY_LIMIT`),
and on the 16 GB laptop these numbers were taken on, proving the 42,518-cycle
demo was killed by the OOM killer and stayed killed after every available knob
was turned down. See [`../zk/README.md`](../zk/README.md).

The distinction matters, because cycles are a *proxy* for proving cost, not
proving cost. Within one shard configuration, proving time is roughly linear in
cycles — but the constant is exactly what is missing here, and it is the number
anyone evaluating this project will want. Until a proof exists, this file
describes the *shape* of the cost curve and none of its magnitude.

Cycle counts are nonetheless real and reproducible: `execute` runs the guest in
the zkVM without proving it, so it needs none of proving's memory.

**These numbers are expected to improve before they are final.** Per-candidate
cost is dominated by SHA-256, and both guests currently use vanilla `sha2`
while SP1 ships hashing precompiles (see "What the shape reveals" below).

## Machine

| | |
|---|---|
| CPU | 12th Gen Intel Core i5-12450H (12 logical cores) |
| RAM | 15.7 GB. First run: Docker/WSL2 capped at 8.19 GB, with unrelated containers holding ~2.5 GB of it for part of the session. Current figures: container capped at 10 GB |
| OS | Windows 11 Pro, Docker Desktop → WSL2, `rust:1` container |
| SP1 | 6.3.1, guest toolchain `rustc 1.94.0-dev (succinct)` |
| Date | 2026-07-27 (first run); 2026-10-04 (the figures below, re-measured after adding the duplicate-candidate check) |

## Reproducing

```sh
python bench/generate-sitting.py 1 10 100 200 400
docker run --rm -v "$PWD":/work \
  -v quaestor-cargo-registry:/usr/local/cargo/registry \
  -v quaestor-sp1:/root/.sp1 \
  -w /work rust:1 bash bench/cycles.sh 1 10 100 200 400
```

The generator is seeded and the key is fixed, so the inputs are byte-identical
on every run and on every machine. The exam is 100 questions, 5 choices, one
question cancelled under full-credit policy, and every 17th question carrying
two accepted answers (a post-appeal ruling).

## Cycles vs sitting size

| candidates | total cycles | per candidate | marginal |
|---:|---:|---:|---:|
| 1 | 249,323 | 249,323 | — |
| 10 | 613,899 | 61,389 | 40,508 |
| 100 | 4,238,149 | 42,381 | 40,269 |
| 200 | 8,263,275 | 41,316 | 40,251 |
| 400 | 16,312,486 | 40,781 | 40,246 |
| 800 | *executor OOM* (first run) | — | — |

Fitted over n ≥ 100:

```
cycles ≈ 213,000 + 40,250 × n
```

The marginal cost is consistent to 0.7% across four independent intervals, and
the model predicts n = 200 to within 0.005%. It is slightly loose at the small
end — the implied fixed cost drifts from 209,073 at n = 1 to 213,370 at n = 400,
about 2% — so the single-candidate figures below are the *measured* ones, not
the fit's.

Compared with the first run (2026-07-27), every sitting costs about 1.1% more:
roughly 450 cycles per candidate at these sizes. That is the check that no
pseudonym appears twice in a sitting, which sorts the candidates' pseudonyms
inside the guest.

The 800-candidate row is not a property of quaestor: the SP1 *executor* itself
ran out of memory on this laptop. Execution ceiling here is between 400 and 800
candidates; the proving ceiling is zero.

## What batching buys

The fixed ~213,000 cycles are key validation (a quadratic id scan), the canonical
key encoding, and the key commitment hash. None of it depends on the sheet, so
a per-sheet proof pays all of it for one candidate while a sitting shares it:

| | cycles per candidate |
|---|---:|
| one proof per sheet (measured, n = 1) | 249,323 |
| one proof per sitting (marginal) | 40,250 |
| **ratio** | **6.19×** |

That is only the *execution* saving. The larger win is orthogonal and not
captured in this table: a sitting of n candidates produces **one** proof instead
of n, so the per-candidate cost of proof generation, storage, and publication
falls by a factor of n. Each candidate receives that one proof plus a
`log2(n)`-hash inclusion path.

## Projection to real sittings

SP1's default shard is `1 << 24` = 16,777,216 cycles, so at 40,250 cycles per
candidate a shard holds about **416 candidates**.

| sitting | cycles | shards |
|---:|---:|---:|
| 1,000 | 40.5 M | 3 |
| 10,000 | 403 M | 25 |
| 100,000 | 4.03 G | 240 |
| 1,000,000 | 40.2 G | 2,399 |

The duplicate-candidate check grows as n log n rather than n, so at a million
candidates these linear projections are low by roughly 1–2%. A sitting is also
capped at 2²⁰ = 1,048,576 candidates (`MAX_BATCH`): a larger exam is graded as
several sittings, for example one per exam centre, each with its own proof.

The operationally important consequence: **memory does not grow with sitting
size — time does.** Shards are proven in sequence and only a bounded number are
in flight, so a 32 GB machine that can prove a thousand-candidate sitting can
also prove a hundred-thousand-candidate one; it simply takes proportionally
longer. Scaling this system calls for a patient machine, not an exotic one.

## What the shape reveals

Raising the exam from 5 to 100 questions — a 20× increase in grading work —
moved per-candidate cost only about 33% (roughly 30,000 → 40,000 cycles). Per
candidate, therefore, quaestor spends most of its cycles **hashing, not
grading**: the answer-sheet hash, the report encoding, and the leaf hash.

Two things follow. Optimising the scoring loop would be close to pointless. And
routing the guests' SHA-256 through SP1's `syscall_sha256_compress` /
`syscall_sha256_extend` precompiles — which they do not currently use — should
move the headline per-candidate number substantially. That is the next
measurement, and it has to be gated on producing byte-identical commitments,
since a hash that disagrees with the published one changes the meaning of every
commitment ever issued.

## Checking cost (candidate side)

What a candidate does with the published results, timed directly: hash their
own answer sheet, then run `check_batch_inclusion` on their row. Unlike the cycle
counts above, these are wall-clock times on the host, natively and in
WebAssembly. Verifying the SP1 proof itself comes first and is not included; it
needs a real proof.

| Candidates | Inclusion path | Data per candidate | Native x86-64 | WebAssembly (V8) |
|---:|---:|---:|---:|---:|
| 10 | 4 hashes | 220 B | 1.3 µs | 4.8 µs |
| 100 | 7 hashes | 316 B | 1.7 µs | 6.7 µs |
| 1,000 | 10 hashes | 412 B | 2.0 µs | 8.8 µs |
| 10,000 | 14 hashes | 540 B | 2.5 µs | 11.2 µs |
| 100,000 | 17 hashes | 636 B | 2.9 µs | 13.2 µs |
| 1,000,000 | 20 hashes | 732 B | 3.2 µs | 15.2 µs |

"Data per candidate" is the 92-byte report plus 32 bytes per sibling hash on the
inclusion path, which is what the results list must deliver to each candidate
beside the proof. Both it and the check time grow with log₂ n: about 0.6 µs
and 32 bytes per doubling in WebAssembly. Grading a million-sheet sitting
natively with `grade_batch`, which the institution does to publish the results
list, took 1.4 s (including the duplicate-candidate check).

**Method.** The exam has the same shape as the cycle benchmark (100 questions,
five choices, one cancelled, every 17th with two accepted answers), generated
deterministically in `crates/grading-core/examples/verify_cost.rs`. The middle
candidate of each sitting is checked. Each figure is the median of 15 rounds of
20,000 checks, with rounds interleaved across sittings. Interleaving matters on
this CPU. Timed one sitting after another, the WebAssembly run showed a 2× jump
between 10,000 and 100,000 candidates that path length cannot explain: over a
long run, the hybrid i5-12450H shifts clock speed and moves work between core
types. WebAssembly runs `crates/grading-wasm` on Node.js 26.7 (V8 14.6), the
engine family Chrome uses. Native runs Rust 1.96 with `--release`. Same machine,
2026-10-01.

```sh
cargo run --release -p grading-core --example verify_cost -- --fixtures bench/out/verify-fixtures.json
(cd crates/grading-wasm && cargo build --release --target wasm32-unknown-unknown)
node bench/verify-wasm.mjs
```

## Still missing

- Proving time, proof size and verification time at sitting scale (the demo
  exam's are in `proofs/2026-10-04`)
- The same curve with hashing precompiles enabled
- GPU (`SP1_PROVER=cuda`) figures
- Verification of the SP1 proof itself in a browser. The candidate-side claim
  check that follows it is measured above.
