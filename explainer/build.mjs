// Build dist/quaestor.html: the walkthrough page with grading-core inlined as
// WebAssembly, plus a pure-JavaScript fallback (wasm2js) for pages whose
// content security policy blocks WebAssembly.
//
//   node explainer/build.mjs
//
// Needs cargo with the wasm32-unknown-unknown target. The fallback also needs
// binaryen's `wasm2js` on PATH; without it the page is built WebAssembly-only.
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(here, "..");
const dist = path.join(here, "dist");
const targetDir = path.join(dist, "wasm-target");
fs.mkdirSync(dist, { recursive: true });

// MVP target features keep the module translatable by wasm2js. A separate
// target dir keeps this build from replacing the benchmark's default build.
execFileSync(
  "cargo",
  ["build", "--release", "--target", "wasm32-unknown-unknown", "--target-dir", targetDir],
  { cwd: path.join(root, "crates", "grading-wasm"), stdio: "inherit", env: { ...process.env, RUSTFLAGS: "-C target-cpu=mvp" } },
);
const wasmPath = path.join(targetDir, "wasm32-unknown-unknown", "release", "grading_wasm.wasm");
const wasm = fs.readFileSync(wasmPath);

let fallback = "";
try {
  const out = path.join(dist, "grading_wasm.wasm2js.mjs");
  execFileSync("wasm2js", ["--enable-bulk-memory", "--enable-sign-ext", "--enable-mutable-globals", "-Oz", wasmPath, "-o", out], {
    stdio: "inherit",
  });
  // wasm2js emits an ES module; the page needs a classic script that sets a global.
  const body = fs.readFileSync(out, "utf8").split("\n").filter((l) => !l.startsWith("export var ")).join("\n");
  if (/<\/script/i.test(body)) throw new Error("wasm2js output contains a closing script tag");
  fallback = `(function () {\n${body}\nwindow.QWASM2JS = retasmFunc;\n})();`;
} catch (e) {
  if (e.code !== "ENOENT") throw e;
  console.warn("wasm2js (binaryen) not found: building without the JavaScript fallback");
}

let html = fs.readFileSync(path.join(here, "src.html"), "utf8");
const swap = (marker, value) => {
  if (!html.includes(marker)) throw new Error("missing marker " + marker);
  html = html.replace(marker, () => value);
};
swap("/*WASM2JS*/", fallback);
swap("/*WASM_B64*/", wasm.toString("base64"));
swap("/*ENGINE*/", fs.readFileSync(path.join(here, "engine.js"), "utf8"));
fs.writeFileSync(path.join(dist, "quaestor.html"), html);
console.log(`dist/quaestor.html  ${(html.length / 1024).toFixed(0)} KB  (fallback: ${fallback ? "yes" : "no"})`);
