#!/usr/bin/env bash
# Build the SP1 proving layer and prove the demo exam inside a Linux container.
# Windows/macOS hosts run this via Docker Desktop from the repo root:
#
#   docker run --rm \
#     -v "$PWD":/work \
#     -v ispat-cargo-registry:/usr/local/cargo/registry \
#     -v ispat-sp1:/root/.sp1 \
#     -w /work rust:1 bash zk/run-in-docker.sh
#
# The two named volumes cache the cargo registry and the SP1 toolchain so only
# the first run pays the multi-GB download.
set -euo pipefail

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

echo "== execute (no proof, cycle count) =="
cargo run --release -- execute --key "$KEY" --salt "$SALT" --sheet "$SHEET"

echo "== prove =="
time cargo run --release -- prove --key "$KEY" --salt "$SALT" --sheet "$SHEET" --out ../../demo-proof.bin

echo "== verify =="
cargo run --release -- verify --proof ../../demo-proof.bin

echo "== done: first end-to-end ispat proof =="
