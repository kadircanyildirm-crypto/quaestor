# quaestor

**Verifiable grading: cryptographic proof that your grade was computed correctly.**

*Quaestor* (Latin: the Roman magistrate who audited the public accounts; from
*quaestio*, "an inquiry" — literally, "a question") is a grading engine whose
execution is proven inside a zkVM. An institution publishes a commitment to the
answer key *before* an exam; after grading, it publishes each student's score
together with a zero-knowledge proof that the score was computed from (a) the
committed key and (b) the student's actual answers — **without revealing the
answer key**.

## See it work

Seven short animations, in order. They are rendered from an in-browser
explainer that runs this repo's `grading-core` compiled to WebAssembly, so every
seal, fingerprint, grade, Merkle path and verdict in them is computed by the
real code. The one thing drawn rather than computed is the SP1 proof itself
(see [Status](#status)).

Hashes are drawn as pictures. A seal's petals and dots are the 32 bytes of a
commitment, and the timing marks along the edge of a sheet are the 32 bytes of
its hash. Equal hashes give identical pictures; change one input and the picture
changes completely. The exam is the repo's demo exam
([`examples/demo-exam`](examples/demo-exam)): five questions, question 3
accepts two answers after an appeal, question 5 is cancelled, and the sitting
uses all four sample sheets.

### 1. Before the exam: the school seals its answer key

![The answer key and a secret salt are sealed in an envelope; only the seal is posted on the public notice board](docs/media/1-seal.gif)

**What it shows:** The answer key and a secret 32-byte salt go into an
envelope. Only the seal is posted on the public notice board; the key stays
with the school.

**What runs underneath:** `commit_answer_key(key, salt)`, which is SHA-256 over
the key's canonical encoding and the salt. The salt stops anyone guessing the
key from the seal. The hash stops the school changing the key later without
changing the seal. Demo seal: `d4f3126b…5c9bd51d0c`.

### 2. The exam

![Four candidates fill in their bubble sheets; each sheet gets a fingerprint along its edge](docs/media/2-exam.gif)

**What it shows:** Four candidates mark their sheets, and each sheet gets a
fingerprint.

**What runs underneath:** `hash_answer_sheet(sheet)`, which is SHA-256 over the
candidate's pseudonym and answers in canonical form. Candidates are identified
by a 32-byte pseudonym, never by name. This hash is what later ties a result to
one candidate's actual answers.

### 3. Grading, proven

![The sealed key and the sheets go into the SP1 zkVM; results, a Merkle tree and one proof come out](docs/media/3-prove.gif)

**What it shows:** The sealed key and every sheet go into the zkVM. Out come
everyone's results, a Merkle tree over them, and one proof for the whole
sitting.

**What runs underneath:** The batch guest (`zk/program-batch`) runs
`grade_batch(key, salt, sheets)` inside SP1. It checks the key and computes its
seal once, scores every sheet, hashes each result into a leaf, and builds a
Merkle root bound to the number of candidates. The guest publishes only 76
bytes: the seal, the root, the exam id and the candidate count. Measured cost on a
100-question exam: about 214,000 + 39,800 × n zkVM cycles
([BENCHMARKS](docs/BENCHMARKS.md)).

### 4. Your check

![The seal on your result is matched to the posted seal, your answers to the fingerprint, and your row is walked up to the proven root](docs/media/4-check.gif)

**What it shows:** The seal on your result is matched against the posted seal,
your own copy of your answers against the fingerprint on your result, and your
row is walked up the tree to the proven root.

**What runs underneath:** `check_batch_inclusion(public_values,
posted_seal, your_result, your_path, your_sheet_hash)`. It needs no zkVM and
costs a few dozen hashes, so it runs anywhere. In the CLI,
`quaestor-cli verify-batch` runs it right after SP1 has verified the proof.

### 5. Forgery: change the key after the exam

![The school edits question 4 on the key, re-grades and re-proves; the new seal does not match the posted one and the result is rejected](docs/media/5-cheat-key.gif)

**What it shows:** After the exam the school changes the answer to question 4,
re-grades everyone and proves it again. Your score rises to 100%, and the new
proof is valid.

**What runs underneath:** The edited key has a different seal
(`b361aaa4…`), so `check_batch_inclusion` returns `CommitmentMismatch`. The
proof is about a key that was never posted.

### 6. Forgery: someone else's result

![A genuine result belonging to another candidate is handed to you; its fingerprint does not match your answers](docs/media/6-cheat-swap.gif)

**What it shows:** You are handed a genuine, proven result that belongs to
another candidate.

**What runs underneath:** The fingerprint on that result is not the hash of
your answers, so `check_batch_inclusion` returns `SheetHashMismatch`.

### 7. Forgery: raise a published score

![After proving, a 60% result is shown as 100%; its leaf changes and no longer reaches the proven root](docs/media/7-cheat-raise.gif)

**What it shows:** After the proof is made, the results page shows a 60%
candidate as 100%.

**What runs underneath:** The edited result hashes to a different leaf
(`8a02095a…` becomes `8ae3296e…`), and its path no longer reaches the proven
root, so `check_batch_inclusion` returns `NotInBatch`. The score was published
but never proven.

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
