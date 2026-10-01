#!/usr/bin/env python3
"""Draw the README benchmark charts as plain SVG, one light and one dark variant each.

    python bench/plot.py

No plotting library: the charts are small, and hand-placed SVG keeps them crisp
and byte-stable. The numbers below are copied from measurements, not computed:

- PROVING:  `bash bench/cycles.sh` (see docs/BENCHMARKS.md, "Cycles vs sitting size")
- CHECKING: `cargo run --release -p grading-core --example verify_cost` (native)
            `node bench/verify-wasm.mjs` (WebAssembly)
"""
import math
import os

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "docs", "media")

# candidates -> zkVM cycles per candidate, batch guest, 100-question exam, SP1 6.3.1
PROVING = [(1, 248_862), (10, 60_937), (100, 41_934), (200, 40_870), (400, 40_335)]
PER_SHEET = 248_862  # one proof per sheet pays the whole fixed cost every time

# candidates -> microseconds to hash your sheet and run check_batch_inclusion
CHECK_NATIVE = [(10, 1.3), (100, 1.7), (1_000, 2.0), (10_000, 2.5), (100_000, 2.9), (1_000_000, 3.2)]
CHECK_WASM = [(10, 4.8), (100, 6.7), (1_000, 8.8), (10_000, 11.2), (100_000, 13.2), (1_000_000, 15.2)]

THEMES = {
    "light": dict(surface="#ffffff", text="#1f2328", muted="#59636e", grid="#e6eaef", axis="#c8d1da",
                  s1="#2a78d6", s2="#eb6834"),
    "dark": dict(surface="#0d1117", text="#f0f6fc", muted="#9198a1", grid="#21262d", axis="#3d444d",
                 s1="#3987e5", s2="#d95926"),
}
FONT = "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', Helvetica, Arial, sans-serif"
W, H = 880, 440
LEFT, RIGHT, TOP, BOTTOM = 64, 196, 104, 58


class Chart:
    def __init__(self, theme, title, subtitle, xdomain, ydomain):
        self.t = THEMES[theme]
        self.x0, self.x1 = (math.log10(v) for v in xdomain)
        self.y0, self.y1 = ydomain
        self.parts = [
            f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" '
            f'font-family="{FONT}" style="font-variant-numeric: tabular-nums">',
            f"<title>{title}</title>",
            f'<rect width="{W}" height="{H}" rx="6" fill="{self.t["surface"]}"/>',
            self.text(LEFT - 40, 32, title, size=17, weight=600),
            self.text(LEFT - 40, 54, subtitle, size=13, color="muted"),
        ]

    def x(self, v):
        return LEFT + (math.log10(v) - self.x0) / (self.x1 - self.x0) * (W - LEFT - RIGHT)

    def y(self, v):
        return H - BOTTOM - (v - self.y0) / (self.y1 - self.y0) * (H - TOP - BOTTOM)

    def text(self, x, y, s, size=12, color="text", anchor="start", weight=400):
        return (f'<text x="{x:.1f}" y="{y:.1f}" font-size="{size}" font-weight="{weight}" '
                f'fill="{self.t[color]}" text-anchor="{anchor}">{s}</text>')

    def legend(self, items):
        x = LEFT - 40
        for label, key in items:
            self.parts.append(f'<line x1="{x}" y1="76" x2="{x + 18}" y2="76" stroke="{self.t[key]}" '
                              f'stroke-width="2.5" stroke-linecap="round"/>')
            self.parts.append(self.text(x + 25, 80, label, size=12.5))
            x += 25 + 7.2 * len(label) + 26

    def grid(self, yticks, ylabel, xticks):
        for v in yticks:
            yy = self.y(v)
            color = self.t["axis"] if v == self.y0 else self.t["grid"]
            self.parts.append(f'<line x1="{LEFT}" y1="{yy:.1f}" x2="{W - RIGHT}" y2="{yy:.1f}" '
                              f'stroke="{color}" stroke-width="1"/>')
            self.parts.append(self.text(LEFT - 10, yy + 4, ylabel(v), size=12, color="muted", anchor="end"))
        for v, label in xticks:
            xx = self.x(v)
            self.parts.append(f'<line x1="{xx:.1f}" y1="{H - BOTTOM}" x2="{xx:.1f}" y2="{H - BOTTOM + 5}" '
                              f'stroke="{self.t["axis"]}" stroke-width="1"/>')
            self.parts.append(self.text(xx, H - BOTTOM + 21, label, size=12, color="muted", anchor="middle"))

    def axis_title(self, s):
        mid = LEFT + (W - LEFT - RIGHT) / 2
        self.parts.append(self.text(mid, H - 10, s, size=12.5, color="muted", anchor="middle"))

    def series(self, points, key, markers=True):
        d = " ".join(f"{'M' if i == 0 else 'L'}{self.x(a):.1f},{self.y(b):.1f}" for i, (a, b) in enumerate(points))
        self.parts.append(f'<path d="{d}" fill="none" stroke="{self.t[key]}" stroke-width="2" '
                          f'stroke-linejoin="round" stroke-linecap="round"/>')
        if markers:
            for a, b in points:
                self.parts.append(f'<circle cx="{self.x(a):.1f}" cy="{self.y(b):.1f}" r="4.5" '
                                  f'fill="{self.t[key]}" stroke="{self.t["surface"]}" stroke-width="2"/>')

    def end_label(self, point, key, name, value, dy=0):
        xx, yy = self.x(point[0]) + 12, self.y(point[1]) + dy
        self.parts.append(f'<line x1="{xx}" y1="{yy - 4:.1f}" x2="{xx + 12}" y2="{yy - 4:.1f}" '
                          f'stroke="{self.t[key]}" stroke-width="2.5" stroke-linecap="round"/>')
        self.parts.append(self.text(xx + 18, yy, name, size=12.5, weight=600))
        self.parts.append(self.text(xx + 18, yy + 16, value, size=12.5, color="muted"))

    def save(self, name):
        self.parts.append("</svg>")
        with open(os.path.join(OUT, name), "w", encoding="utf-8", newline="\n") as f:
            f.write("\n".join(self.parts) + "\n")


def proving(theme):
    c = Chart(theme, "Proving work per candidate falls as the sitting grows",
              "zkVM cycles per candidate · batch guest · 100-question exam · SP1 6.3.1, execution only",
              (1, 400), (0, 275_000))
    c.legend([("One proof per sitting (measured)", "s1"), ("One proof per sheet", "s2")])
    c.grid(range(0, 275_001, 50_000), lambda v: "0" if v == 0 else f"{v // 1000}k",
           [(1, "1"), (10, "10"), (100, "100"), (200, "200"), (400, "400")])
    c.axis_title("Candidates in the sitting (log scale)")
    c.series([(1, PER_SHEET), (400, PER_SHEET)], "s2", markers=False)
    c.series(PROVING, "s1")
    # The gap between the two lines at the right edge is the whole case for batching.
    last = PROVING[-1]
    xx = c.x(last[0]) - 26
    c.parts.append(f'<line x1="{xx:.1f}" y1="{c.y(PER_SHEET) + 8:.1f}" x2="{xx:.1f}" y2="{c.y(last[1]) - 8:.1f}" '
                   f'stroke="{c.t["muted"]}" stroke-width="1"/>')
    for yy, sign in ((c.y(PER_SHEET) + 8, 1), (c.y(last[1]) - 8, -1)):
        c.parts.append(f'<path d="M{xx - 4:.1f},{yy + 6 * sign:.1f} L{xx:.1f},{yy:.1f} L{xx + 4:.1f},{yy + 6 * sign:.1f}" '
                       f'fill="none" stroke="{c.t["muted"]}" stroke-width="1"/>')
    mid = (c.y(PER_SHEET) + c.y(last[1])) / 2
    c.parts.append(c.text(xx - 10, mid - 4, f"{PER_SHEET / last[1]:.1f}× less", size=13, weight=600, anchor="end"))
    c.parts.append(c.text(xx - 10, mid + 13, "work per candidate", size=12, color="muted", anchor="end"))
    c.end_label((400, PER_SHEET), "s2", "One proof per sheet", f"{PER_SHEET:,} cycles", dy=4)
    c.end_label(last, "s1", "One proof per sitting", f"{last[1]:,} cycles", dy=4)
    c.parts.append(c.text(c.x(1) + 2, c.y(PER_SHEET) - 14, "At 1 candidate the two coincide",
                          size=12, color="muted"))
    c.save(f"bench-proving-{theme}.svg")


def checking(theme):
    c = Chart(theme, "Checking your result stays in microseconds",
              "Hash your sheet, then check_batch_inclusion on your row · one SHA-256 path hash per doubling of the sitting",
              (10, 1_000_000), (0, 18))
    c.legend([("WebAssembly, in V8", "s1"), ("Native x86-64", "s2")])
    c.grid(range(0, 19, 3), lambda v: f"{v} µs" if v else "0",
           [(10, "10"), (100, "100"), (1_000, "1k"), (10_000, "10k"), (100_000, "100k"), (1_000_000, "1M")])
    c.axis_title("Candidates in the sitting (log scale) · inclusion path: 4 hashes at 10, 20 at a million")
    c.series(CHECK_WASM, "s1")
    c.series(CHECK_NATIVE, "s2")
    c.end_label(CHECK_WASM[-1], "s1", "WebAssembly", f"{CHECK_WASM[-1][1]} µs at 1M", dy=4)
    c.end_label(CHECK_NATIVE[-1], "s2", "Native", f"{CHECK_NATIVE[-1][1]} µs at 1M", dy=4)
    c.save(f"bench-checking-{theme}.svg")


if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    for theme in THEMES:
        proving(theme)
        checking(theme)
    print("wrote bench-proving-{light,dark}.svg and bench-checking-{light,dark}.svg to docs/media/")
