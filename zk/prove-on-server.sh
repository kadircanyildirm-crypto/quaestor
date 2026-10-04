#!/usr/bin/env bash
# One session of real SP1 proofs on a rented Linux server, every number recorded.
#
# Needs Ubuntu 24.04 on x86-64, run as root, with at least 32 GB of RAM (64 GB
# recommended, e.g. Hetzner CCX43). On a fresh server:
#
#   curl -sSfL https://raw.githubusercontent.com/kadircanyildirm-crypto/quaestor/main/zk/prove-on-server.sh | bash
#
# What it does, printing as it goes:
#   1. installs the build tools, Docker (SP1 runs the groth16 wrap in a
#      container) and SP1 6.3.1, the version the guests are pinned to;
#   2. runs zk/run-in-docker.sh with the real CPU prover: the demo exam proven
#      per sheet and per sitting, and all six forgeries refused;
#   3. proves the demo again in compressed and groth16 mode, and the 100- and
#      400-candidate benchmark sittings, and checks that real groth16 proofs
#      still refuse forgeries;
#   4. writes the results to bench/out/proofs.md and prints them.
# Nothing on the server is needed afterwards; delete it when the report is out.
set -euo pipefail

SP1_VERSION=v6.3.1
REPO_URL=https://github.com/kadircanyildirm-crypto/quaestor
REPO=${REPO:-$HOME/quaestor}
export DEBIAN_FRONTEND=noninteractive

mem_gb=$(awk '/MemTotal/ {printf "%d", $2 / 1048576}' /proc/meminfo)
if [ "$mem_gb" -lt 30 ]; then
  echo "This machine has ${mem_gb} GB of RAM; SP1 6.3.1 proving needs at least 24 GB and 32+ is safer." >&2
  echo "Set FORCE=1 to try anyway." >&2
  [ "${FORCE:-0}" = 1 ] || exit 1
fi

echo "== installing build tools and Docker"
apt-get update -qq
apt-get install -y -qq build-essential clang pkg-config libssl-dev protobuf-compiler libprotobuf-dev \
  docker.io git curl python3 time >/dev/null
systemctl enable --now docker >/dev/null 2>&1 || true

export PATH="$HOME/.cargo/bin:$HOME/.sp1/bin:$PATH"
if ! command -v cargo >/dev/null 2>&1; then
  echo "== installing Rust"
  curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal >/dev/null
fi
if ! command -v cargo-prove >/dev/null 2>&1; then
  echo "== installing SP1 $SP1_VERSION"
  curl -sSfL https://raw.githubusercontent.com/succinctlabs/sp1/main/sp1up/sp1up | bash -s -- --version "$SP1_VERSION"
fi
cargo prove --version

[ -d "$REPO/.git" ] || git clone -q "$REPO_URL" "$REPO"
cd "$REPO"
mkdir -p bench/out
REPORT="$REPO/bench/out/proofs.md"
{
  echo "# Real SP1 proofs"
  echo
  echo "- date: $(date -u +%Y-%m-%dT%H:%MZ)"
  echo "- commit: $(git rev-parse --short HEAD)"
  echo "- cpu: $(lscpu | awk -F: '/Model name/ {gsub(/^ +/, "", $2); print $2; exit}'), $(nproc) threads"
  echo "- memory: ${mem_gb} GB"
  echo "- sp1: $(cargo prove --version 2>&1 | head -n 1)"
  echo
} >"$REPORT"

echo "== step 2: demo exam with the real prover, and the six forgeries"
SP1_PROVER=cpu bash zk/run-in-docker.sh 2>&1 | tee bench/out/run-in-docker.log
{
  echo "## Demo exam, core proofs (zk/run-in-docker.sh)"
  echo
  echo '```'
  sed -n '/== numbers/,/====================/p' bench/out/run-in-docker.log
  grep -c "correctly rejected" bench/out/run-in-docker.log | sed 's/^/forgeries refused: /'
  echo '```'
  echo
  echo "## Proof modes and sitting sizes"
  echo
  echo "| What | Mode | Proving | Setup | Proof size | Verify | Peak RAM |"
  echo "|---|---|---:|---:|---:|---:|---:|"
} >>"$REPORT"

cd zk/script
CLI=./target/release/quaestor-cli
export SP1_PROVER=cpu
SALT=0101010101010101010101010101010101010101010101010101010101010101
E=../../examples/demo-exam
W=../../bench/out/proofs
mkdir -p "$W"
COMMIT=$("$CLI" execute --key "$E/key.json" --salt "$SALT" --sheet "$E/sheet.json" | awk '/^key commitment/ {print $NF; exit}')

field() { awk -v k="$1" -v n="$2" '$1 == k {print $n; exit}' "$3"; }
# measure LABEL MODE OUT VERIFY... -- PROVE...: prove (peak memory recorded), verify, add a row.
measure() {
  local label=$1 mode=$2 out=$3
  shift 3
  local verify=()
  while [ "$1" != "--" ]; do verify+=("$1"); shift; done
  shift
  echo "== $label, $mode"
  /usr/bin/time -f "%M" -o "$W/mem.txt" "$CLI" "$@" --mode "$mode" --out "$out" | tee "$W/prove.log"
  "$CLI" "${verify[@]}" --proof "$out" | tee "$W/verify.log"
  local bytes
  bytes=$(sed -n 's/^proof saved .*, \([0-9]*\) bytes.*/\1/p' "$W/prove.log" | head -n 1)
  printf '| %s | %s | %s s | %s s | %s B | %s ms | %s GB |\n' "$label" "$mode" \
    "$(field proving 3 "$W/prove.log")" "$(field setup 3 "$W/prove.log")" "$bytes" \
    "$(awk '/^verify time/ {print $4; exit}' "$W/verify.log")" \
    "$(awk '{printf "%.1f", $1 / 1048576}' "$W/mem.txt")" >>"$REPORT"
}

DEMO_SITTING=(--sheet "$E/sheet.json" --sheet "$E/other-sheet.json" --sheet "$E/third-sheet.json")
for mode in compressed groth16; do
  measure "1 sheet (demo)" "$mode" "$W/single-$mode.bin" \
    verify --commitment "$COMMIT" --sheet "$E/sheet.json" -- \
    prove --key "$E/key.json" --salt "$SALT" --sheet "$E/sheet.json"
  measure "3-candidate sitting (demo)" "$mode" "$W/demo-$mode.bin" \
    verify-batch --manifest "$W/demo-$mode.json" --commitment "$COMMIT" --sheet "$E/sheet.json" -- \
    prove-batch --key "$E/key.json" --salt "$SALT" "${DEMO_SITTING[@]}" --manifest "$W/demo-$mode.json"
done

echo "== benchmark sittings"
(cd ../.. && python3 bench/generate-sitting.py 100 400)
B=../../bench/out
BENCH_COMMIT=$("$CLI" execute --key "$B/key.json" --salt "$SALT" --sheet "$B/sitting-100/c000000.json" |
  awk '/^key commitment/ {print $NF; exit}')
for n in 100 400; do
  for mode in compressed groth16; do
    measure "$n-candidate sitting" "$mode" "$W/bench-$n-$mode.bin" \
      verify-batch --manifest "$W/bench-$n-$mode.json" --commitment "$BENCH_COMMIT" -- \
      prove-batch --key "$B/key.json" --salt "$SALT" --sheets "$B/sitting-$n" --manifest "$W/bench-$n-$mode.json"
  done
done

echo "== real groth16 proofs must still refuse forgeries"
refused=0
expect_rejected() {
  if "$CLI" "$@" >/dev/null 2>&1; then echo "FAIL: accepted: $*" | tee -a "$REPORT"; else refused=$((refused + 1)); fi
}
FORGED=$(printf '%s' "$COMMIT" | tr '0-9a-f' '1-9a-f0')
expect_rejected verify --proof "$W/single-groth16.bin" --commitment "$COMMIT" --sheet "$E/other-sheet.json"
expect_rejected verify --proof "$W/single-groth16.bin" --commitment "$FORGED" --sheet "$E/sheet.json"
expect_rejected verify-batch --proof "$W/demo-groth16.bin" --manifest "$W/demo-groth16.json" \
  --commitment "$COMMIT" --sheet "$E/absent-sheet.json"
sed 's/"score_bp": 6000/"score_bp": 10000/' "$W/demo-groth16.json" >"$W/tampered.json"
expect_rejected verify-batch --proof "$W/demo-groth16.bin" --manifest "$W/tampered.json" \
  --commitment "$COMMIT" --sheet "$E/other-sheet.json"
{
  echo
  echo "Forgeries refused with real groth16 proofs: $refused of 4."
  echo
  echo "Proving and setup are wall-clock seconds inside quaestor-cli. Verify is the SP1"
  echo "verification call alone. Peak RAM is the CLI process's maximum resident set; the"
  echo "groth16 wrap runs in a Docker container and is not included in it."
} >>"$REPORT"

echo
echo "================================================================"
cat "$REPORT"
echo "================================================================"
echo "Report saved to $REPORT. Copy it before deleting the server."
