#!/usr/bin/env bash
# Record the terminal demo with VHS. Stages a scratch exam directory so nothing
# is written into the repo. Run after build.sh (see explainer/README.md).
set -euo pipefail
HERE=/root/quaestor/explainer/terminal
export PATH="/target/release:$HOME/.sp1/bin:$PATH"
export CARGO_TARGET_DIR=/target
cp "$HERE/demo.bashrc" /root/demo.bashrc
rm -rf /root/exam && mkdir -p /root/exam/sheets
cp /root/quaestor/examples/demo-exam/{key,sheet,other-sheet,third-sheet}.json /root/exam/
cp /root/exam/{sheet,other-sheet,third-sheet}.json /root/exam/sheets/
# The forged key for "Forgery 2": the answer to question 4 changed after the exam.
sed 's/"accepted": \[4\]/"accepted": [1]/' /root/exam/key.json > /root/exam/swapped-key.json
cmp -s /root/exam/key.json /root/exam/swapped-key.json && { echo "swap edit did not apply"; exit 1; }
# Pre-build the tests at this mount path so the video shows no compile wait.
cargo test --workspace --no-run -q --manifest-path /root/quaestor/Cargo.toml
cd /root/exam
vhs "${1:-$HERE/demo.tape}"
