#!/usr/bin/env bash
# Build quaestor-cli and the workspace tests inside the recording image.
# Run from the repo root (see explainer/README.md for the full command).
set -euo pipefail
export PATH="$HOME/.sp1/bin:$PATH"
if ! rustup toolchain list | grep -q succinct; then
  rustup toolchain link succinct "$(ls -d "$HOME"/.sp1/toolchains/* | head -n 1)"
fi
cargo-prove prove --version
cd /root/quaestor/zk/script
cargo build --release
cd /root/quaestor
cargo test --workspace --no-run -q
ls -la "${CARGO_TARGET_DIR:-target}/release/quaestor-cli"
