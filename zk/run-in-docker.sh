#!/usr/bin/env bash
# Build the SP1 proving layer and prove the demo exam inside a Linux container.
# Windows/macOS hosts run this via Docker Desktop from the repo root:
#
#   docker run --rm \
#     -v "$PWD":/work \
#     -v quaestor-cargo-registry:/usr/local/cargo/registry \
#     -v quaestor-sp1:/root/.sp1 \
#     -w /work rust:1 bash zk/run-in-docker.sh
#
# The two named volumes cache the cargo registry and the SP1 toolchain so only
# the first run pays the multi-GB download.
set -euo pipefail

# The run records its own numbers. Until proving time, proof size and verify
# time are written down somewhere, the project's central claim — that a zkVM
# makes this *practical* — has no evidence behind it at all.
NUMBERS=/tmp/quaestor-numbers.txt
: > "$NUMBERS"
now_ms() { date +%s%3N; }
record() { printf '  %-38s %s\n' "$1" "$2" >> "$NUMBERS"; }

# SP1_PROVER=mock runs the guest and produces a proof object without doing the
# cryptography, which is exactly right for exercising *this* project's checks —
# every negative case below is quaestor's own claim-checking code, not SP1's.
# It is exactly wrong for timing anything, so a mock run refuses to present its
# durations as measurements.
PROVER="${SP1_PROVER:-cpu}"
case "$PROVER" in
  cpu | cuda) REAL_PROVER=1 ;;
  *) REAL_PROVER=0 ;;
esac
record "prover" "$PROVER"

export PATH="$HOME/.sp1/bin:$PATH"
export DEBIAN_FRONTEND=noninteractive

echo "== installing build dependencies =="
apt-get update -qq
apt-get install -y -qq curl clang pkg-config libssl-dev protobuf-compiler >/dev/null

if ! command -v cargo-prove >/dev/null 2>&1; then
  echo "== installing SP1 toolchain =="
  # sp1up.succinct.xyz is unreachable from some networks (this one included);
  # zk/.cache/sp1up is the same script fetched from the GitHub source of truth.
  # Everything sp1up itself downloads comes from github.com, which is fine.
  # The GitHub sp1up script is the complete installer: it installs
  # cargo-prove and links the succinct rust toolchain in one pass.
  if [ -f zk/.cache/sp1up ]; then
    bash zk/.cache/sp1up
  else
    curl -sSfL --retry 5 --retry-all-errors https://raw.githubusercontent.com/succinctlabs/sp1/main/sp1up/sp1up | bash
  fi
fi
cargo-prove prove --version

# The container is ephemeral but only ~/.sp1 persists (named volume); the
# rustup registration of the succinct toolchain lives in RUSTUP_HOME and must
# be re-linked on every run.
if ! rustup toolchain list | grep -q succinct; then
  echo "== linking succinct toolchain into rustup =="
  TOOLCHAIN_DIR=$(ls -d "$HOME"/.sp1/toolchains/* 2>/dev/null | head -n 1 || true)
  if [ -n "$TOOLCHAIN_DIR" ]; then
    rustup toolchain link succinct "$TOOLCHAIN_DIR"
  else
    cargo prove install-toolchain
  fi
fi

echo "== building host CLI (build.rs also compiles the guest ELF) =="
cd zk/script
cargo build --release

SALT=0101010101010101010101010101010101010101010101010101010101010101
KEY=../../examples/demo-exam/key.json
SHEET=../../examples/demo-exam/sheet.json
OTHER_SHEET=../../examples/demo-exam/other-sheet.json
THIRD_SHEET=../../examples/demo-exam/third-sheet.json
ABSENT_SHEET=../../examples/demo-exam/absent-sheet.json
PROOF=../../demo-proof.bin
BATCH_PROOF=../../demo-batch.bin
MANIFEST=../../demo-sitting.json
TAMPERED=../../demo-sitting-tampered.json
RELABELLED=../../demo-sitting-relabelled.json
PSEUDONYM_B=1122334455667788112233445566778811223344556677881122334455667788
PSEUDONYM_C=c3d4e5f6a7b8091ac3d4e5f6a7b8091ac3d4e5f6a7b8091ac3d4e5f6a7b8091a

echo "== execute (no proof, cycle count) =="
cargo run --release -- execute --key "$KEY" --salt "$SALT" --sheet "$SHEET" | tee /tmp/execute.log

# What the institution would publish *before* the exam. Everything after this
# point is the student's side and uses only public data.
COMMITMENT=$(awk '/^key commitment/ {print $NF; exit}' /tmp/execute.log)
echo "published commitment: $COMMITMENT"

record "single-sheet cycles" "$(awk '/^cycles/ {print $NF; exit}' /tmp/execute.log)"

echo "== prove =="
T=$(now_ms)
cargo run --release -- prove --key "$KEY" --salt "$SALT" --sheet "$SHEET" --out "$PROOF"
record "single-sheet proving" "$(( $(now_ms) - T )) ms"
record "single-sheet proof size" "$(stat -c%s "$PROOF") bytes"

echo "== verify: student checks the proof against the published commitment and their own answers =="
T=$(now_ms)
cargo run --release -- verify --proof "$PROOF" --commitment "$COMMITMENT" --sheet "$SHEET"
# Honest label: this wall-clock includes process start and the CLI's
# `client.setup()` call, which re-derives the proving key just to reach the
# verifying key. The *verification* itself is a small fraction of it. Week 2
# has to separate the two before any of this is quoted as "verify time".
record "single-sheet verify (incl. setup)" "$(( $(now_ms) - T )) ms"

# The negative cases matter more than the positive one: a verifier that accepts
# everything would also print "valid" above. Both proofs below are genuine and
# cryptographically verify — they simply prove the wrong statement.
echo "== negative 1: same proof, a different candidate's answers — must be rejected =="
if cargo run --release -- verify --proof "$PROOF" --commitment "$COMMITMENT" --sheet "$OTHER_SHEET"; then
  echo "FAIL: verifier accepted a proof about someone else's sheet"; exit 1
fi
echo "correctly rejected ✓"

echo "== negative 2: same proof, a commitment the institution never published — must be rejected =="
FORGED_COMMITMENT=$(printf '%s' "$COMMITMENT" | tr '0-9a-f' '1-9a-f0')
if cargo run --release -- verify --proof "$PROOF" --commitment "$FORGED_COMMITMENT" --sheet "$SHEET"; then
  echo "FAIL: verifier accepted a proof under an uncommitted key"; exit 1
fi
echo "correctly rejected ✓"

# ── batching: the same exam proven once for the whole sitting ────────────────
# Three candidates, deliberately an odd number so the tree's promotion path is
# exercised by the demo itself and not only by a unit test.

echo "== batch: grade the whole sitting in one guest execution =="
cargo run --release -- execute-batch --key "$KEY" --salt "$SALT" \
  --sheet "$SHEET" --sheet "$OTHER_SHEET" --sheet "$THIRD_SHEET" | tee /tmp/execute-batch.log
record "batch cycles (3 candidates)" "$(awk '/^cycles  / {print $NF; exit}' /tmp/execute-batch.log)"
record "batch cycles per candidate" "$(awk '/^cycles\/sheet/ {print $NF; exit}' /tmp/execute-batch.log)"

echo "== batch: one proof for the sitting =="
T=$(now_ms)
cargo run --release -- prove-batch --key "$KEY" --salt "$SALT" \
  --sheet "$SHEET" --sheet "$OTHER_SHEET" --sheet "$THIRD_SHEET" \
  --out "$BATCH_PROOF" --manifest "$MANIFEST"
BATCH_MS=$(( $(now_ms) - T ))
record "batch proving (3 candidates)" "$BATCH_MS ms"
record "batch proving per candidate" "$(( BATCH_MS / 3 )) ms"
record "batch proof size" "$(stat -c%s "$BATCH_PROOF") bytes"
record "published results list" "$(stat -c%s "$MANIFEST") bytes"

echo "== batch: a candidate checks their own row of the published results =="
T=$(now_ms)
cargo run --release -- verify-batch --proof "$BATCH_PROOF" --manifest "$MANIFEST" \
  --commitment "$COMMITMENT" --sheet "$SHEET"
record "batch verify, one candidate (incl. setup)" "$(( $(now_ms) - T )) ms"

echo "== batch: an auditor checks that the published list *is* the proven sitting =="
cargo run --release -- verify-batch --proof "$BATCH_PROOF" --manifest "$MANIFEST" \
  --commitment "$COMMITMENT"

echo "== negative 3: a candidate who was not in this sitting — must be rejected =="
if cargo run --release -- verify-batch --proof "$BATCH_PROOF" --manifest "$MANIFEST" \
     --commitment "$COMMITMENT" --sheet "$ABSENT_SHEET"; then
  echo "FAIL: verifier placed a candidate in a sitting they never sat"; exit 1
fi
echo "correctly rejected ✓"

# The failure mode that only exists once grading is batched: the proof covers a
# root, not a results page, so an institution can prove the sitting honestly and
# then edit one row. Nothing about the proof changes — only the missing path.
echo "== negative 4: one published score raised after proving — must be rejected =="
# Guard both ends of the edit. A negative case that silently fails to tamper
# passes for the wrong reason, which is worse than not having it: the demo
# would then be advertising a check it never ran.
if ! grep -q '"score_bp": 6000' "$MANIFEST"; then
  echo "FAIL: expected a 60.00% row in the published list to tamper with"; exit 1
fi
sed 's/"score_bp": 6000/"score_bp": 10000/' "$MANIFEST" > "$TAMPERED"
if grep -q '"score_bp": 6000' "$TAMPERED"; then
  echo "FAIL: the tamper edit did not apply; the negative case would pass vacuously"; exit 1
fi
if cargo run --release -- verify-batch --proof "$BATCH_PROOF" --manifest "$TAMPERED" \
     --commitment "$COMMITMENT" --sheet "$OTHER_SHEET"; then
  echo "FAIL: verifier accepted a score that was never proven"; exit 1
fi
echo "correctly rejected ✓"

echo "== negative 5: the whole list re-audited after the same edit — must be rejected =="
if cargo run --release -- verify-batch --proof "$BATCH_PROOF" --manifest "$TAMPERED" \
     --commitment "$COMMITMENT"; then
  echo "FAIL: audit accepted a results list that is not the proven sitting"; exit 1
fi
echo "correctly rejected ✓"

# Every score in this list is genuinely proven; only the names on them are
# swapped. Inclusion is blind to it — the pseudonym is not in the leaf — so the
# check that catches it is the candidate's own sheet, whose hash commits to
# their pseudonym alongside their answers. An auditor holding no sheets cannot
# catch this, which is why `verify-batch` without --sheet says so out loud.
echo "== negative 6: two candidates' identities swapped in the published list — must be rejected =="
sed -e "s/$PSEUDONYM_B/__SWAP__/" -e "s/$PSEUDONYM_C/$PSEUDONYM_B/" -e "s/__SWAP__/$PSEUDONYM_C/" \
  "$MANIFEST" > "$RELABELLED"
if cmp -s "$MANIFEST" "$RELABELLED"; then
  echo "FAIL: the relabel edit did not apply; the negative case would pass vacuously"; exit 1
fi
if cargo run --release -- verify-batch --proof "$BATCH_PROOF" --manifest "$RELABELLED" \
     --commitment "$COMMITMENT" --sheet "$OTHER_SHEET"; then
  echo "FAIL: verifier accepted a row published under someone else's pseudonym"; exit 1
fi
echo "correctly rejected ✓"

echo
echo "== numbers =============================================================="
cat "$NUMBERS"
echo "========================================================================="
if [ "$REAL_PROVER" = "1" ]; then
  echo "Three candidates is a demo, not a benchmark: per-candidate cost only"
  echo "starts falling once the sitting is large enough to amortise the fixed"
  echo "cost of one proof. The curve is week 2's job."
else
  echo "!! SP1_PROVER=$PROVER — NOT A REAL PROVER."
  echo "!! Every duration and proof size above is meaningless and must never be"
  echo "!! quoted. What this run does establish is that the pipeline is correct:"
  echo "!! the guests execute, the sitting is graded, the results list is built,"
  echo "!! and all six forgeries are refused by quaestor's own checks."
fi
echo
echo "== done: per-sheet and per-sitting proofs, and six forgeries refused =="
