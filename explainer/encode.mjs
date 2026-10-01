// Turn the captured film (out/frames + out/segments.json) into the README GIFs
// and one full MP4. Needs ffmpeg on PATH.
//
//   node explainer/encode.mjs        then copy out/gifs/*.gif to docs/media/
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const out = path.join(here, "out");
const frames = path.join(out, "frames", "%05d.jpg");
const segments = JSON.parse(fs.readFileSync(path.join(out, "segments.json"), "utf8"));

// README order. The short chapter-5 intro is folded into the first forgery.
const GIFS = [
  ["1-seal", ["ch1"]],
  ["2-exam", ["ch2"]],
  ["3-prove", ["ch3"]],
  ["4-check", ["ch4"]],
  ["5-cheat-key", ["ch5", "cheatKey"]],
  ["6-cheat-swap", ["cheatSwap"]],
  ["7-cheat-raise", ["cheatRaise"]],
];
const PALETTE =
  "fps=15,scale=960:-1:flags=lanczos,split[x][y];" +
  "[x]palettegen=max_colors=128:stats_mode=diff[p];" +
  "[y][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle";

const ffmpeg = (args) => execFileSync("ffmpeg", ["-nostdin", "-v", "error", "-y", ...args], { stdio: "inherit" });
fs.mkdirSync(path.join(out, "gifs"), { recursive: true });

for (const [name, segs] of GIFS) {
  const first = segments.findIndex((s) => segs.includes(s));
  const last = segments.findLastIndex((s) => segs.includes(s));
  if (first < 0) throw new Error(`segment ${segs} not found; re-run capture.mjs film`);
  ffmpeg(["-framerate", "30", "-start_number", String(first), "-i", frames, "-frames:v", String(last - first + 1),
    "-vf", PALETTE, "-loop", "0", path.join(out, "gifs", `${name}.gif`)]);
  const kb = fs.statSync(path.join(out, "gifs", `${name}.gif`)).size / 1024;
  console.log(`${name}.gif  frames ${first}-${last}  ${kb.toFixed(0)} KB`);
}

ffmpeg(["-framerate", "30", "-i", frames, "-c:v", "libx264", "-preset", "slow", "-crf", "18", "-pix_fmt", "yuv420p",
  "-movflags", "+faststart", path.join(out, "quaestor-explainer.mp4")]);
console.log("quaestor-explainer.mp4");
