// Times the candidate-side check in WebAssembly, on V8 (the engine in Chrome
// and Node.js): hash your own answer sheet, then check_batch_inclusion on your
// row. The inputs are the rows that the native benchmark writes:
//
//   cargo run --release -p grading-core --example verify_cost -- --fixtures bench/out/verify-fixtures.json
//   (cd crates/grading-wasm && cargo build --release --target wasm32-unknown-unknown)
//   node bench/verify-wasm.mjs
import fs from "node:fs";

const here = (p) => new URL(p, import.meta.url);
const wasm = fs.readFileSync(here("../crates/grading-wasm/target/wasm32-unknown-unknown/release/grading_wasm.wasm"));
const fixtures = JSON.parse(fs.readFileSync(here("out/verify-fixtures.json"), "utf8"));
const { instance } = await WebAssembly.instantiate(wasm, {});
const ex = instance.exports;

const hex = (h) => Uint8Array.from(h.match(/../g), (b) => parseInt(b, 16));
function put(bytes) {
  const p = ex.q_alloc(bytes.length);
  new Uint8Array(ex.memory.buffer).set(bytes, p);
  return p;
}

function median(xs) {
  return [...xs].sort((a, b) => a - b)[xs.length >> 1];
}

// One function for every sitting, so V8 optimizes a single call site rather
// than a fresh closure per row.
function checkRow(pSheet, sheetLen, pHash, pPv, pC, pR, index, pSib, nSib) {
  ex.q_sheet_hash(pSheet, sheetLen, pHash);
  return ex.q_check_inclusion(pPv, pC, pR, index, pSib, nSib, pHash);
}

// Rows are prepared first and timed round-robin, so drift over the run (CPU
// clocks, a hybrid CPU moving the thread to an efficiency core) lands on every
// sitting alike instead of on whichever comes last.
const rows = fixtures.map((f) => {
  // grading-wasm sheet format: [exam_id u64][pseudonym][n u32][answers], little-endian
  const sheet = new Uint8Array(8 + 32 + 4 + f.answers.length);
  const view = new DataView(sheet.buffer);
  view.setBigUint64(0, BigInt(f.exam_id), true);
  sheet.set(hex(f.pseudonym), 8);
  view.setUint32(40, f.answers.length, true);
  f.answers.forEach((a, i) => (sheet[44 + i] = a === null ? 0xff : a));
  const siblings = new Uint8Array(32 * f.siblings.length);
  f.siblings.forEach((s, i) => siblings.set(hex(s), 32 * i));
  const row = {
    n: f.n,
    args: [put(sheet), sheet.length, ex.q_alloc(32), put(hex(f.batch_public_values)), put(hex(f.commitment)),
      put(hex(f.report)), f.index, put(siblings), f.siblings.length],
    samples: [],
  };
  if (checkRow(...row.args) !== 0) throw new Error(`row ${f.index} of ${f.n} did not verify`);
  return row;
});

const iters = 20000;
for (const row of rows) {
  const [a, b, c, d, e, g, h, i, j] = row.args;
  for (let k = 0; k < iters; k++) checkRow(a, b, c, d, e, g, h, i, j); // warm-up
}
for (let r = 0; r < 15; r++) {
  for (const row of rows) {
    const [a, b, c, d, e, g, h, i, j] = row.args;
    const t = process.hrtime.bigint();
    for (let k = 0; k < iters; k++) checkRow(a, b, c, d, e, g, h, i, j);
    row.samples.push(Number(process.hrtime.bigint() - t) / iters / 1000);
  }
}

console.log(`WebAssembly on Node.js ${process.version} (V8 ${process.versions.v8})`);
console.log("| Candidates | Inclusion path | Check (WebAssembly) |");
console.log("|---:|---:|---:|");
for (const row of rows) {
  console.log(`| ${row.n} | ${row.args[8]} hashes | ${median(row.samples).toFixed(1)} µs |`);
}
