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
  questions under two policies. 84 tests, incl. proptest properties for
  commitment binding, encoding roundtrips, and adversarial decoding.
- ✅ Public-values ABI (`grading_core::public_values`): the two layouts a guest
  commits and every verifier parses — 92 bytes for one sheet, 76 for a whole
  sitting — owned by one module so guest and verifier cannot drift. With them,
  the claim checks that turn "this proof is valid" into "this proof is about
  *my* sheet under the key committed *before* the exam". `no_std` and
  proof-system-agnostic, so the same code will verify in a browser.
- ✅ `zk/`: two SP1 guests + `quaestor-cli` host. Both build under SP1 6.3.1
  (pinned exactly; the guest image ID is part of the claim). Both execute in the
  zkVM: the demo exam takes **42,518 cycles** for one sheet and **119,700** for
  a three-candidate sitting, reproducing the expected reports in each case.
- ✅ Batching, end to end (`grading_core::batch` + `zk/program-batch`): a whole
  sitting is graded in one guest execution and committed as a single Merkle
  root, so N candidates cost one proof plus a `log2(n)` inclusion path each
  instead of N proofs. `check_batch_inclusion` is the entire student-side
  check — `no_std`, no proof system, a few dozen hashes.
- ✅ The pipeline refuses all six forgeries end to end: a proof checked against
  another candidate's answers, against a commitment never published, a
  candidate absent from the sitting, one published score raised after the
  sitting was proven, that edited list re-audited whole, and a list whose scores
  are all genuinely proven but whose identities were swapped.
- ✅ Determinism, confirmed rather than asserted: the host's independently
  computed Merkle root equals the root the guest committed — the same
  `grade_batch` code, on x86-64 and inside the RISC-V zkVM, byte for byte.
- ⛔ **No proof has been generated yet.** SP1 6.3.1 assumes a 24 GB machine;
  proving the 42,518-cycle demo was OOM-killed on a 16 GB laptop and stayed
  killed after every available knob was turned down (see `zk/README.md`). The
  checks above ran under `SP1_PROVER=mock`, which exercises quaestor's own
  logic and none of SP1's cryptography. Everything else here runs on a laptop;
  proof *generation* needs 32 GB.
- 🔜 Honest benchmark numbers (proof time, proof size, verify time; CPU + GPU)
  and the cost-vs-class-size curve that batching exists to flatten.
- 🔜 Browser-side verifier (WASM), pilot with a real course.

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
