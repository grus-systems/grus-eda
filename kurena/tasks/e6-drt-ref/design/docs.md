# e6-drt-ref — Documentation updates

## `docs/results.md` entry

New `### 2026-11-24 — e6-drt-ref: ...` section (date = run date),
mirroring the `e8-irdrop-ref` entry's structure:

- **Commands verbatim**: `bash tests/designs/generate_drt_ref.sh gcd`
  and `bash tests/designs/generate_drt_ref.sh picorv32`, the container
  image, and the host-side `generate_guide_ours_spef.sh` invocation if
  it ran.
- **Measured grammar**: the DRT- and antenna-related transcript
  surfaces actually found — the `check_antennas` flag form used, the
  final-tally line grammar, anything the `DRT-PROBE`/`DRT-SURFACE`
  markers revealed (the "Measured grammar" paragraph the irdrop entry
  carries).
- **Baseline-vs-delegated delta table** per design:

  | metric | baseline | delegated | delta |
  |---|---|---|---|
  | DRC violations | … | … | … |
  | antenna violations | … | … | … |
  | total wirelength (µm) | … | … | … |
  | via count total | … | … | … |
  | per-cut-layer vias | … | … | … |
  | route runtime (s) | … | … | … |
  | routed-DEF SHA-256 (16) | `…` | `…` | — |

- **Cross-check**: `baseline_def_identical` vs the prior
  `<d>.routed.def` for each design — byte-identity is expected on gcd
  (same commands that produced it, same image); a divergence is
  recorded with its likely cause, not smoothed.
- **vs the P-GR scoping record** (`docs/results.md` 2026-11-22):
  scoping claimed `read_guides` + `detailed_route` on a foreign guide
  gives 0 DRC on gcd — confirm or correct it against the delegated
  run's actual numbers, and name any deviation from what scoping
  recorded.
- **Rerun count** and which fields differed on rerun (`timing`
  expected; anything else is a named residual).
- **Artifact table**: every committed and gitignored artifact with
  size + SHA-256 first 16 (the irdrop table shape).
- **What was run / not run**: state `cargo test --workspace` +
  `cargo clippy --workspace --all-targets -- -D warnings` results and
  that no version bump applies — reference-only, nothing
  Cargo-visible changed.
- If the delegated run produces nonzero DRC, nonzero antenna counts,
  or dies mid-route on either design, that outcome is written down
  verbatim — including partial-transcript evidence — as the ticket's
  actual result, not treated as a blocker.

## `tests/designs/README.md`

- Artifact rows for `<d>.drt.reference.json`, `<d>.drc.<variant>.rpt`,
  `<d>.antenna.<variant>.rpt` in the §2 table, plus a sentence naming
  `generate_drt_ref.sh <design>` and its `generate_guide_ours_spef.sh`
  dependency — same register as the existing CTS/IR-drop paragraphs.
- Note the gitignored `<d>.drt.<variant>.def` outputs.

## `docs/roadmap.md` §E6

A `- Progress 2026-11-24 (ticket \`e6-drt-ref\`): delegation reference
committed — ...` line in the E6/E7 block: baseline vs delegated
verdicts per design (DRC/antenna counts, wirelength delta, whether
`baseline_def_identical` held), same style as the E5/E8 lines.
