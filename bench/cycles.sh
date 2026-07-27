#!/usr/bin/env bash
# Cycle counts vs sitting size.
#
# `execute` runs the guest in the zkVM without proving it, so this needs none of
# the memory proving does — the shape of the cost curve is measurable on a
# laptop that cannot generate a single proof. Run it the same way as the demo:
#
#   python bench/generate-sitting.py 1 10 100 400
#   docker run --rm -v "$PWD":/work \
#     -v quaestor-cargo-registry:/usr/local/cargo/registry -v quaestor-sp1:/root/.sp1 \
#     -w /work rust:1 bash bench/cycles.sh 1 10 100 400
set -euo pipefail

export PATH="$HOME/.sp1/bin:$PATH"
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq && apt-get install -y -qq clang pkg-config libssl-dev protobuf-compiler >/dev/null
if ! rustup toolchain list | grep -q succinct; then
  rustup toolchain link succinct "$(ls -d "$HOME"/.sp1/toolchains/* | head -n 1)"
fi

cd /work/zk/script
cargo build --release 2>&1 | tail -1

B=/work/bench/out
SALT=0101010101010101010101010101010101010101010101010101010101010101
SHARD=16777216   # SP1's MAX_SHARD_SIZE, the default

echo
printf '%8s %14s %18s %8s\n' "n" "cycles" "cycles/candidate" "shards"
printf '%8s %14s %18s %8s\n' "--------" "--------------" "------------------" "--------"

for n in "$@"; do
  out=$(./target/release/quaestor-cli execute-batch \
          --key "$B/key.json" --salt "$SALT" --sheets "$B/sitting-$n" 2>&1) || {
    echo "n=$n FAILED:"; echo "$out" | tail -5; continue; }
  cyc=$(awk '/^cycles  / {print $NF; exit}' <<<"$out")
  per=$(awk '/^cycles\/sheet/ {print $NF; exit}' <<<"$out")
  shards=$(( (cyc + SHARD - 1) / SHARD ))
  printf '%8s %14s %18s %8s\n' "$n" "$cyc" "$per" "$shards"
done
