# quaestor

**Verifiable grading: cryptographic proof that your grade was computed correctly.**

*Quaestor* (Latin: the Roman magistrate who audited the public accounts; from
*quaestio*, "an inquiry" — literally, "a question") is a grading engine whose
execution is proven inside a zkVM. An institution publishes a commitment to the
answer key *before* an exam; after grading, it publishes each student's score
together with a zero-knowledge proof that the score was computed from (a) the
committed key and (b) the student's actual answers — **without revealing the
answer key**.

## The problem

When a student appeals a grade today, the institution's answer is "the system
computed it." There is no way for the student — or a court, or an auditor — to
check that:

- the answer key wasn't quietly changed after the exam,
- the published scoring rules (weights, cancelled questions, accepted answers)
  were actually applied,
- *their* answer sheet was the one graded.

Grade-tampering and answer-key scandals are recurring, real events in national
exam systems. Policy work on algorithmic accountability (OECD, Ada Lovelace
Institute) asks for transparency registers and audits; quaestor gives the stronger
answer: **a proof**. The idea traces to Kroll et al.'s *Accountable Algorithms*
(U. Penn. L. Rev. 2017); zkVMs finally make it practical to build.

## The statement being proven

> There exists an answer key `K` and salt `s` such that
> `SHA-256(canonical(K) ‖ s) = C`, and grading `K` against the answer sheet
> with canonical hash `H` yields exactly the report `R`.

Public: commitment `C` (published pre-exam), sheet hash `H` (recomputable by
the student), report `R` (score, correct/wrong/blank/cancelled counts).
Private: the key `K` and salt `s`.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full design, including
what this does *not* prove (trust model), and
[docs/ROADMAP.md](docs/ROADMAP.md) for where this is going.

## Status

- ✅ `grading-core`: deterministic, `no_std`, zkVM-agnostic grading engine —
  exam model, canonical encode/decode, salted key commitments, scoring with
  weighted questions, post-appeal multiple-accepted answers, and cancelled
  questions under two policies. 32 tests (incl. 17 proptest properties:
  commitment binding, roundtrips, adversarial decoding).
- ✅ `zk/`: SP1 guest program + `quaestor-cli` host (execute/prove/verify),
  92-byte fixed public-values layout — awaiting first build on the SP1
  toolchain (Linux/WSL2).
- 🔜 First end-to-end proof of the demo exam; honest benchmark numbers.
- 🔜 Batch proving (one proof per exam sitting, not per sheet), browser-side
  verifier, pilot with a real course.

## Try it

```sh
cargo test
```

The core is plain Rust — no zkVM toolchain needed to work on grading logic.
Proving will live in a separate crate so the trusted computation stays small
and auditable.

## Design constraints

Everything in `grading-core` must run unchanged inside a zkVM guest:

1. **Determinism** — integer basis points, no floats, no map iteration order,
   no clocks, no randomness.
2. **`no_std` + `alloc`** — guests have no OS.
3. **Hand-rolled canonical encodings** — commitment security must not depend
   on a serde backend's stability.

## Non-goals

No blockchain, no tokens. Proof generation and verification are ordinary
computations; publishing commitments needs nothing more exotic than the
institution's website (or any append-only transparency log).

## License

MIT OR Apache-2.0 (dual, standard Rust convention).
