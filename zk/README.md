# zk — SP1 proving layer

Three crates, deliberately outside the root workspace (they need the SP1
toolchain, which is Linux-only; the root stays buildable everywhere):

- **`program/`** — the single-sheet guest. ~40 lines wrapping
  `grading_core::score`. This plus `grading-core` is the entire trusted
  computing base: no serde, no JSON; inputs are quaestor's canonical bytes,
  decoded by the same code the commitments are defined over.
- **`program-batch/`** — the batch guest: grades a whole sitting in one
  execution and commits a single Merkle root over the reports, so N candidates
  cost one proof instead of N. Same trusted base, same grading code.
- **`script/`** — the host CLI (`quaestor-cli`): loads human-friendly JSON,
  produces canonical bytes, and runs `execute` / `prove` / `verify` for one
  sheet and `execute-batch` / `prove-batch` / `verify-batch` for a sitting.

The two guests are separate programs on purpose. Separate programs have
separate verifying keys, which is what makes it impossible to present a batch
proof where a single-sheet proof is expected — enforced by the proof system,
not by a flag in the public values that a verifier could forget to check.

No guest owns the public-values layout: it lives in
`grading_core::public_values`, so the bytes a guest commits and the bytes a
verifier parses come from one function and cannot drift apart. That module is
`no_std` and proof-system-agnostic — checking a proof's *claims* needs no zkVM,
which is what will let the same code verify in a browser.

## Building (Linux / WSL2)

```sh
# once:
curl -L https://sp1up.succinct.xyz | bash
sp1up

# then:
cd zk/script
cargo run --release -- execute \
  --key ../../examples/demo-exam/key.json \
  --salt 0101010101010101010101010101010101010101010101010101010101010101 \
  --sheet ../../examples/demo-exam/sheet.json

cargo run --release -- prove --key ... --salt ... --sheet ... --out proof.bin

# the student's side: public data only — the proof, the commitment the
# institution published before the exam, and their own copy of their answers
cargo run --release -- verify \
  --proof proof.bin \
  --commitment <64-hex, as published pre-exam> \
  --sheet ../../examples/demo-exam/sheet.json
```

### A whole sitting, proven once

```sh
# --sheet is repeatable; --sheets <dir> takes every *.json in it, sorted by
# name. Either way the order is the leaf order, so it is deterministic and
# anyone can re-derive the same root from the same inputs.
cargo run --release -- prove-batch \
  --key ../../examples/demo-exam/key.json \
  --salt 0101010101010101010101010101010101010101010101010101010101010101 \
  --sheet ../../examples/demo-exam/sheet.json \
  --sheet ../../examples/demo-exam/other-sheet.json \
  --sheet ../../examples/demo-exam/third-sheet.json \
  --out batch.bin --manifest sitting.json

# a candidate: "is the number published next to my name the one that was proven?"
cargo run --release -- verify-batch \
  --proof batch.bin --manifest sitting.json \
  --commitment <64-hex, as published pre-exam> \
  --sheet ../../examples/demo-exam/sheet.json

# an auditor, holding no answer sheets: "is this published list *the* sitting?"
cargo run --release -- verify-batch \
  --proof batch.bin --manifest sitting.json \
  --commitment <64-hex, as published pre-exam>
```

`sitting.json` is the results list the institution publishes: one row per
candidate — their report and the `log2(n)` inclusion path that ties it to the
proven root. Nothing in it is trusted. The root and the commitment a verifier
checks against are read out of the *proof*; the list's copies exist only so a
disagreement can be reported as "this list is not the proven sitting" instead
of surfacing as an inscrutable hash failure.

This is the failure mode batching introduces and per-sheet proving does not
have: **the proof covers a root, not a results page.** A sitting can be proven
honestly and one row edited afterwards, and the proof still verifies — because
it was never about that row. The inclusion path is the only thing that closes
the gap, so a missing one is a non-zero exit, and auditor mode additionally
requires every position to be published exactly once (otherwise a list could
duplicate one candidate's row and quietly drop another's).

One half of that gap does not close for auditors, and the CLI says so instead
of implying otherwise. A leaf is the report, and a report carries no pseudonym,
so inclusion binds every published *score* and not the name beside it: a list
whose identities are swapped passes the audit with every score genuinely
proven. What binds a pseudonym is the sheet hash, whose preimage holds it next
to the answers — so the only party who can check the identity column is the
candidate, and `verify-batch --sheet` does check it. Without `--sheet`, the
output reports the identity column as **NOT checked** rather than printing an
unqualified pass. Binding it for auditors too would mean putting the pseudonym
in the leaf, which is an ABI change rather than a fix.

`verify` requires `--commitment` by design. A proof that merely verifies says
"*some* sheet was graded under *some* key" — a claim a tampering institution
satisfies trivially by proving honest grading under a key it swapped in after
the exam. Only the pre-exam commitment rules that out, and only `--sheet` binds
the result to one student rather than to some other candidate. Any mismatch is
a non-zero exit, never a printed warning.

Windows/macOS hosts get all of this — build, proof, and both negative cases —
from `docker run ... bash zk/run-in-docker.sh` (header of that file has the
full command).

## Versions, and why they are pinned hard

First real build: **2026-07-27, SP1 6.3.1**, guest toolchain `rustc 1.94.0-dev
(succinct)`. Both guests compiled unmodified — no SDK call names had drifted
from what this repo was written against.

Every SP1 crate is now pinned with `=6.3.1` and all three `Cargo.lock`s are
committed. That is stricter than ordinary hygiene, for a specific reason: the
guest ELF's **image ID is part of the claim**. It is what tells a verifier
which program produced a proof, and `docs/ARCHITECTURE.md` says so ("the guest
binary is public and reproducible; its image ID pins the code"). A floating
dependency would change the ELF, change the image ID, and quietly break that
sentence. Bumping SP1 is therefore a deliberate act that ends with fresh image
IDs recorded here.

## Memory: this needs a real machine

SP1 6.3.1 assumes **24 GB** (`DEFAULT_MEMORY_LIMIT` in `sp1-core-executor`) and
provisions each shard for `1 << 24` = 16.7M cycles by default. The demo exam is
42,518 cycles — four hundred times smaller — and it does not matter: proving it
was killed by the OOM killer on a 16 GB laptop giving Docker 8 GB, and stayed
killed after cutting every worker count to 1 and setting `SHARD_SIZE=2^19` and
`MEMORY_LIMIT=6GB`. Tuning does not get you under the floor.

**Plan for 32 GB and 8+ cores**, or a CUDA box (`SP1_PROVER=cuda`) for anything
at sitting scale. Everything else in this repo — the whole of `grading-core`,
both guests' compilation, `execute`, and every claim check — runs comfortably
on a laptop; it is proof *generation* that does not.

`SP1_PROVER=mock` runs the guests and exercises the entire pipeline without the
cryptography, which is the right tool for testing quaestor's own checks (every
negative case below is this project's code, not SP1's) and the wrong tool for
any number. `run-in-docker.sh` refuses to present a mock run's durations as
measurements.

## Public values: single sheet (92 bytes, fixed)

| offset | size | field |
|---|---|---|
| 0 | 32 | answer-key commitment (must equal the pre-exam publication) |
| 32 | 32 | answer-sheet hash (student recomputes from their own answers) |
| 64 | 8 | exam id (LE) |
| 72 | 4 | score in basis points (LE) |
| 76 | 4 | correct count (LE) |
| 80 | 4 | wrong count (LE) |
| 84 | 4 | blank count (LE) |
| 88 | 4 | cancelled count (LE) |

A verifier accepts iff: the proof verifies against the program's verifying
key, offset-0 equals the published commitment, and offset-32 equals the hash
of the answers the student believes they submitted. All three are enforced by
`quaestor-cli verify`; the last two are `grading_core::check_public_values`,
which is unit-tested without any zkVM in `tests/public_values.rs`.

## Public values: batch (76 bytes, fixed)

| offset | size | field |
|---|---|---|
| 0 | 32 | answer-key commitment (must equal the pre-exam publication) |
| 32 | 32 | Merkle root over the sitting's report leaves |
| 64 | 8 | exam id (LE) |
| 72 | 4 | leaf count — how many candidates sat (LE) |

Note what is absent: no score, no sheet hash, nothing about any individual. A
sitting of any size commits to these 76 bytes, which is the whole cost argument
for batching. A candidate turns them into a statement about themselves with
`grading_core::check_batch_inclusion` — their report, their inclusion path, and
a few dozen SHA-256 compressions, no proof system involved.

The two layouts are kept unconfusable at three levels: separate programs with
separate verifying keys, decoders that demand an exact length, and two lengths
that differ — the last enforced by a `const` assertion, because a verifier that
parsed one as the other would read a Merkle root as a sheet hash.

## Demo exam

`examples/demo-exam/` is a 5-question exam exercising the interesting rules:
one question with two accepted answers (post-appeal), one cancelled question
under full-credit policy. `sheet.json` scores 3 correct + 1 wrong + the
cancelled credit = 80.00%; `other-sheet.json` scores 60.00% and
`third-sheet.json` 100.00%.

Those three are the demo *sitting* — deliberately an odd number, so the tree's
odd-level promotion path is exercised by the end-to-end run and not only by a
unit test. `absent-sheet.json` is a fourth candidate who is not in it.

The negative cases matter more than the positive one, since a verifier that
accepts everything would also have printed "valid" above. `run-in-docker.sh`
requires all five to be refused: a proof checked against another candidate's
answers, a proof checked against a commitment that was never published, a
candidate absent from the sitting, one published score raised after the sitting
was proven, and the same edited list re-audited as a whole.
