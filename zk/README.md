# zk — SP1 proving layer

Two crates, deliberately outside the root workspace (they need the SP1
toolchain, which is Linux-only; the root stays buildable everywhere):

- **`program/`** — the guest. ~40 lines wrapping `grading_core::score`. This
  plus `grading-core` is the entire trusted computing base: no serde, no JSON;
  inputs are quaestor's canonical bytes, decoded by the same code the commitments
  are defined over.
- **`script/`** — the host CLI (`quaestor-cli`): loads human-friendly JSON,
  produces canonical bytes, runs `execute` (fast, no proof), `prove`
  (proof + self-verify + save), and `verify`.

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
cargo run --release -- verify --proof proof.bin
```

SP1 crate versions are written as `"6"` (the major current mid-2026); pin the
exact minor and adjust any drifted SDK call names on the first real build —
the API surface used here (`ProverClient::from_env`, `execute/prove/verify`,
`include_elf!`) is SP1's stable core.

## Public values layout (92 bytes, fixed)

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
of the answers the student believes they submitted.

## Demo exam

`examples/demo-exam/` is a 5-question exam exercising the interesting rules:
one question with two accepted answers (post-appeal), one cancelled question
under full-credit policy. The sample sheet scores 3 correct + 1 wrong + the
cancelled credit = 80.00%.
