// Drive the page's film mode off-screen, one frame at a time.
//
//   node explainer/capture.mjs check   play the whole film; exit 1 on any console error
//   node explainer/capture.mjs film    every frame at 30 fps into out/frames/, plus out/segments.json
//
// Time is stepped by hand through gsap.updateRoot, so frames are evenly spaced
// no matter how slow the machine is. Uses the installed Chrome; set CHROME to a
// browser executable to use another one.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright-core";

const mode = process.argv[2] || "check";
const FPS = 30;
const MAX_SECONDS = 180;
const here = path.dirname(fileURLToPath(import.meta.url));
const out = path.join(here, "out");
const url = pathToFileURL(path.join(here, "dist", "quaestor.html")).href + "?film";

const browser = await chromium.launch(
  process.env.CHROME ? { executablePath: process.env.CHROME } : { channel: "chrome" },
);
const page = await browser.newPage({ viewport: { width: 1920, height: 1080 }, colorScheme: "light" });
const errors = [];
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
page.on("pageerror", (e) => errors.push(e.message));

await page.goto(url);
await page.waitForFunction(() => window.__ready === true, null, { timeout: 30000 });
await page.evaluate(() => document.fonts.ready);
const t0 = await page.evaluate(() => {
  gsap.ticker.remove(gsap.updateRoot);
  const t = gsap.ticker.time;
  window.__film();
  return t;
});
const step = (t) => page.evaluate((x) => gsap.updateRoot(x), t0 + t);
const done = () => page.evaluate(() => window.__done === true);

if (mode === "film") {
  const frames = path.join(out, "frames");
  fs.rmSync(frames, { recursive: true, force: true });
  fs.mkdirSync(frames, { recursive: true });
  const segments = [];
  let f = 0;
  while (f < FPS * MAX_SECONDS && !(await done())) {
    await step(f / FPS);
    segments.push(await page.evaluate(() => window.__seg || ""));
    await page.screenshot({ path: path.join(frames, `${String(f).padStart(5, "0")}.jpg`), type: "jpeg", quality: 94 });
    if (++f % 300 === 0) console.log(`frame ${f}`);
  }
  fs.writeFileSync(path.join(out, "segments.json"), JSON.stringify(segments));
  console.log(`${f} frames (${(f / FPS).toFixed(1)} s) in out/frames`);
} else {
  let t = 0;
  while (t < MAX_SECONDS && !(await done())) await step((t += 0.1));
  console.log((await done()) ? `film plays to the end (${t.toFixed(1)} s)` : "film did not finish");
  if (!(await done())) errors.push("film did not finish");
}

await browser.close();
if (errors.length) {
  console.error(errors.join("\n"));
  process.exit(1);
}
console.log("no console errors");
