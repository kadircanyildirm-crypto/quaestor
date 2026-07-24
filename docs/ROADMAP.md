# Roadmap (12 weeks)

Goal is not revenue: the goal is **credibility artifacts** — a working system,
reproducible benchmarks, a technical report, and pilot evidence — that earn
academic attention and collaboration requests.

## Weeks 1–2 — Core engine ✅ / hardening
- [x] Workspace, `grading-core` (model, canonical encoding, commitments, scoring)
- [x] 15 unit/integration tests, `no_std` build
- [ ] Property-based tests (proptest): score bounds, encoding injectivity,
      commitment binding under random mutations
- [ ] CI (GitHub Actions): test + `no_std` build + fmt/clippy on every push

## Weeks 3–4 — First proof
- [ ] WSL2 Ubuntu + SP1 toolchain (proving is Linux-side; core dev stays on Windows)
- [ ] `program/` SP1 guest wrapping `grading_core::score`
- [ ] `prover/` host CLI: `ispat prove --key key.json --salt ... --sheet sheet.json`
- [ ] `ispat verify` CLI: verifies `(C, H, R, π)` — the demo moment
- [ ] Record honest numbers: proof time, proof size, verify time (CPU + GPU)

## Weeks 5–6 — Batching & benchmarks
- [ ] Batch guest: N sheets → Merkle root of reports, one proof
- [ ] Benchmark curve: proof cost vs class size (10 / 100 / 1k / 10k sheets)
- [ ] Reproducible benchmark harness + `BENCHMARKS.md` (this is what systems
      people read first)

## Weeks 7–8 — Verification UX
- [ ] Browser verifier (WASM): student pastes their answers + published `C`,
      gets ✅/❌ locally — nothing uploaded
- [ ] Public demo page with a full worked exam (commitment published "before",
      scores + proofs "after", one deliberately tampered sheet that fails)

## Weeks 9–10 — Pilot & write-up
- [ ] Pitch BAUM / a course instructor: prove one real midterm's grading
      (anonymized pseudonyms; zero change to their workflow — we only need the
      key, the sheets, and a pre-exam commitment email)
- [ ] Technical report draft: design, trust model, benchmarks, pilot account
- [ ] Appeals story end-to-end: cancelled question → new commitment → regrade
      proofs, fully auditable

## Weeks 11–12 — Outreach
- [ ] arXiv preprint + repo polish (README, reproducibility instructions)
- [ ] Targeted emails (5 lines, link to proof demo): verifiable-evaluations
      authors (MIT), Princeton CITP (Accountable Algorithms lineage), FAccT
      community; Turkish angle: TÜBİTAK / university ethics boards
- [ ] Submission target: FAccT / a security workshop (PETS HotPETs, CCS
      posters) + SOSP/OSDI Student Research Competition

## Later / research track
- Attested AI grading (deterministic pipeline around committed model evals)
- Signed-scan chain of custody for OMR sheets
- Generalization: scholarship rankings, admission lotteries
