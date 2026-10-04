# First real proofs (2026-10-04)

The first SP1 proofs ever generated for quaestor, on the demo exam
([`examples/demo-exam`](../../examples/demo-exam)). Both are groth16 proofs,
the format a browser or a smart contract can verify.

| File | What it proves |
|---|---|
| `single-groth16.bin` | `sheet.json` graded under the committed key: 80.00%, 3 right, 1 wrong, 1 cancelled |
| `demo-groth16.bin` | The three-candidate sitting (`sheet.json`, `other-sheet.json`, `third-sheet.json`) graded in one execution, committed as one Merkle root |
| `demo-groth16.json` | The results list published with the sitting proof: each candidate's report and inclusion path |

Both proofs are under the demo commitment
`d4f3126b73e9d22c7131c60e28b73a787b340a935d55af42fb04775c9bd51d0c`.

## How they were made

- GitHub Codespace: 4 cores (AMD EPYC 7763), 15 GB RAM, 32 GB swap
- SP1 6.3.1 (`cargo-prove sp1 8252c29 2026-06-25`), quaestor commit `a914d16`
- `quaestor-cli prove --mode …` and `prove-batch --mode …`, `SP1_PROVER=cpu`

| What | Mode | Proving | Proof size | Verify | Peak RAM |
|---|---|---:|---:|---:|---:|
| 1 sheet | core | 82 s | 2.78 MB | 124 ms | 9.4 GB |
| 1 sheet | compressed | 304 s | 1.27 MB | 57 ms | 14.4 GB |
| 1 sheet | groth16 | 2,252 s | 1,785 B | 382 ms | 14.0 GB |
| 3-candidate sitting | core | 84 s | 2.78 MB | 124 ms | 9.7 GB |
| 3-candidate sitting | compressed | 303 s | 1.27 MB | 66 ms | 14.2 GB |
| 3-candidate sitting | groth16 | 1,920 s | 1,769 B | 370 ms | 14.3 GB |

Every proof self-verified when it was made and verified again with
`quaestor-cli verify` / `verify-batch`. With the real groth16 proof, the
verifier refused both forgeries tried: another candidate's answers, and a
commitment that was never published.

Read the times with care. The machine is small, proving spilled into swap, and
the first groth16 run includes downloading SP1's circuit artifacts. Peak RAM is
the CLI process alone; the groth16 wrap runs in a separate Docker container and
is not counted. Setup took about 9 s in every run and is not included above.

## Verifying keys

| Program | ELF sha256 | Verifying-key hash |
|---|---|---|
| `quaestor-program` (one sheet) | `fd5f4120…5bd866` | `0x0004e01484167ecde04b58715171dfcf076bfbd84bc2cced0fd3c417c07dc905` |
| `quaestor-batch-program` (sitting) | `99666dc2…fed58f` | `0x007ea81dc6416fc792b503d05fc488a7b31a31caeb889e9f603b6213e46666e4` |

Full ELF hashes:
`fd5f41203b9fd9b1b0685aff5efcdd5dbb359a02aa656c396de4e97fe55bd866` and
`99666dc2bf5c56fc682938ad99b6861f3e774a4e6745675febdb9788b9fed58f`.

**The build is not yet reproducible across machines.** The same source and SP1
version built elsewhere (in `rust:1` with the repo at `/work`) produced
different ELFs, most likely because the build embeds file paths. A guest you
build yourself therefore has a different verifying key, and `quaestor-cli
verify` will reject these proofs. They do verify against the verifying-key
hashes above, for example with SP1's
[`sp1-verifier`](https://docs.succinct.xyz/docs/sp1/verification/off-chain-verification).
The fix is SP1's Docker-based reproducible build, after which proofs will be
regenerated so that anyone can rebuild the guest and get the same key.

## Checksums (sha256)

```
d87edb54f67f5f8265670bbf3615be413154b0259bf0ef06b4223fa7f6c24c67  single-groth16.bin
23f877536c14d4912d5bf98c9a67f994d7779f066475c31cb667938d3def52d9  demo-groth16.bin
d712dfab4ab9db22cca88cc49d5e9cf3a1afbbc3034e904f7aa8dfb40ae71517  demo-groth16.json
```

The core and compressed proofs (2.8 MB and 1.3 MB each) are not in the repo.
