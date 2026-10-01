# explainer

The sources behind the animations in the main README, and the terminal demo.

- **`src.html`** is an animated, interactive walkthrough of the protocol (seal,
  exam, proof, check, and three forgeries). Every seal, sheet hash, grade,
  Merkle path and verdict is computed in the browser by `grading-core`, through
  [`crates/grading-wasm`](../crates/grading-wasm). The SP1 proof is drawn, not
  generated.
- **`terminal/`** records the command-line demo (`quaestor-cli` under
  `SP1_PROVER=mock`) with [VHS](https://github.com/charmbracelet/vhs).

## Walkthrough page and README animations

Requirements: Rust with the `wasm32-unknown-unknown` target, Node.js 18+,
Chrome, ffmpeg, and optionally [binaryen](https://github.com/WebAssembly/binaryen)
for the JavaScript fallback.

```sh
cd explainer
npm install
node build.mjs          # dist/quaestor.html; open it in a browser to use it
node capture.mjs check  # plays the film off-screen, fails on any console error
node capture.mjs film   # out/frames/ at 30 fps and out/segments.json (~4 min)
node encode.mjs         # out/gifs/*.gif and out/quaestor-explainer.mp4
cp out/gifs/*.gif ../docs/media/
```

Open `dist/quaestor.html?film` to see the full-screen film mode that the
capture records. Capture steps animation time frame by frame through
`gsap.updateRoot`, so the output does not depend on machine speed.

## Terminal demo

Run from the repository root. The named volumes cache the cargo registry, the
SP1 toolchain and the build between runs.

```sh
docker build -t quaestor-demo explainer/terminal

docker run --rm -v "$PWD":/root/quaestor \
  -v quaestor-cargo-registry:/usr/local/cargo/registry -v quaestor-sp1:/root/.sp1 \
  -v quaestor-demo-target:/target -e CARGO_TARGET_DIR=/target \
  quaestor-demo bash /root/quaestor/explainer/terminal/build.sh

docker run --rm --memory=3g --shm-size=512m -v "$PWD":/root/quaestor:ro \
  -v "$PWD/explainer/out":/out \
  -v quaestor-cargo-registry:/usr/local/cargo/registry -v quaestor-sp1:/root/.sp1 \
  -v quaestor-demo-target:/target \
  quaestor-demo bash /root/quaestor/explainer/terminal/record.sh
```

The result is `explainer/out/quaestor-demo.mp4`. The recording hides the two
`prove` waits (about 25 s each under the mock prover); every other command plays
in real time.
