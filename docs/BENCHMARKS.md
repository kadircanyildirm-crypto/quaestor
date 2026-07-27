# Benchmarks

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
| RAM | 15.7 GB, of which Docker/WSL2 received 8.19 GB |
| OS | Windows 11 Pro, Docker Desktop → WSL2, `rust:1` container |
| SP1 | 6.3.1, guest toolchain `rustc 1.94.0-dev (succinct)` |
| Date | 2026-07-27 |

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
| 1 | 248,862 | 248,862 | — |
| 10 | 609,370 | 60,937 | 40,056 |
| 100 | 4,193,494 | 41,934 | 39,823 |
| 200 | 8,174,020 | 40,870 | 39,805 |
| 400 | 16,134,045 | 40,335 | 39,800 |
| 800 | *executor OOM* | — | — |

Fitted over n ≥ 100:

```
cycles ≈ 214,000 + 39,800 × n
```

The marginal cost is consistent to 0.6% across four independent intervals, and
the model predicts n = 400 to within 0.003%. It is slightly loose at the small
end — the implied fixed cost drifts from 209,062 at n = 1 to 214,005 at n = 400,
about 2.4% — so the single-candidate figures below are the *measured* ones, not
the fit's.

The 800-candidate row is not a property of quaestor: the SP1 *executor* itself
ran out of memory on this laptop. Execution ceiling here is between 400 and 800
candidates; the proving ceiling is zero.

## What batching buys

The fixed 213,500 cycles are key validation (a quadratic id scan), the canonical
key encoding, and the key commitment hash. None of it depends on the sheet, so
a per-sheet proof pays all of it for one candidate while a sitting shares it:

| | cycles per candidate |
|---|---:|
| one proof per sheet (measured, n = 1) | 248,862 |
| one proof per sitting (marginal) | 39,800 |
| **ratio** | **6.25×** |

That is only the *execution* saving. The larger win is orthogonal and not
captured in this table: a sitting of n candidates produces **one** proof instead
of n, so the per-candidate cost of proof generation, storage, and publication
falls by a factor of n. Each candidate receives that one proof plus a
`log2(n)`-hash inclusion path.

## Projection to real sittings

SP1's default shard is `1 << 24` = 16,777,216 cycles, so at 39,800 cycles per
candidate a shard holds about **416 candidates**.

| sitting | cycles | shards |
|---:|---:|---:|
| 1,000 | 40.0 M | 3 |
| 10,000 | 398 M | 24 |
| 100,000 | 3.98 G | 238 |
| 1,000,000 | 39.8 G | 2,378 |

The operationally important consequence: **memory does not grow with sitting
size — time does.** Shards are proven in sequence and only a bounded number are
in flight, so a 32 GB machine that can prove a thousand-candidate sitting can
also prove a hundred-thousand-candidate one; it simply takes proportionally
longer. Scaling this system calls for a patient machine, not an exotic one.

## What the shape reveals

Raising the exam from 5 to 100 questions — a 20× increase in grading work —
moved per-candidate cost only about 33% (roughly 30,000 → 39,800 cycles). Per
candidate, therefore, quaestor spends most of its cycles **hashing, not
grading**: the answer-sheet hash, the report encoding, and the leaf hash.

Two things follow. Optimising the scoring loop would be close to pointless. And
routing the guests' SHA-256 through SP1's `syscall_sha256_compress` /
`syscall_sha256_extend` precompiles — which they do not currently use — should
move the headline per-candidate number substantially. That is the next
measurement, and it has to be gated on producing byte-identical commitments,
since a hash that disagrees with the published one changes the meaning of every
commitment ever issued.

## Still missing

- Proving time, proof size, verification time — needs a 32 GB machine
- The same curve with hashing precompiles enabled
- GPU (`SP1_PROVER=cuda`) figures
- Verification cost in a browser, which is the number a candidate actually cares
  about
