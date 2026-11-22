# e6-drt-ref — Steps

## 1. Write `tests/designs/generate_drt_ref.sh`

Implement `script.md` end to end (host guards + guide regeneration →
Docker bootstrap → step A → step B → step C). Keep the Tcl
line-per-command and `== 0` rules.

## 2. Run gcd

```bash
mise exec docker-cli@latest colima@latest lima@latest -- colima start --memory 12 --cpu 8
bash tests/designs/generate_drt_ref.sh gcd
```

`gcd`'s `runs/guide-check/gcd.ours.guide` may not exist in the
worktree — the host phase must invoke
`tests/designs/generate_guide_ours_spef.sh gcd` in that case (release
build; also rewrites the deterministic `gcd.guide.ours.spef`).

Then fix what the loud failures find, in order of likelihood:

- **`check_antennas` flag surface** — the `-report_file` spelling is
  assumed, not verified. Probe first; whichever call form the image
  accepts is what the committed script uses, recorded in
  `antenna_check_command`.
- **Transcript grammar** — the final DRT violation tally and the
  antenna verdict line shapes are written against the actual
  `baseline.log`/`delegated.log` text, never against an assumed
  format (the irdrop "measured grammar" rule; `drt_summary_lines` /
  `antenna_log_lines` pin them in the golden).
- **Routed-DEF parse** — `+ ROUTED` chain grammar on real output:
  via-def names, `NEW` layer breaks, `+ SHIELD`/spacing decorations if
  any appear. Unrecognized shape → abort, record, do not fold.
- **Antenna report shape** — violation-line count from whatever file
  or log the probed call produced.

## 3. Byte-reproducibility check

Re-run `bash tests/designs/generate_drt_ref.sh gcd` and `git diff`
every committed artifact — the JSON may differ only inside `timing`
(`golden.md` §determinism). A diff anywhere else → fix the cause or
record it as a named residual in `docs/results.md`; never commit a
golden that reproduced once by luck (§7.1).

## 4. picorv32

```bash
bash tests/designs/generate_drt_ref.sh picorv32
```

`picorv32.placed.def` is gitignored — the worktree seed or
`tests/designs/picorv32/generate_ref.sh` supplies it;
`runs/guide-check/picorv32.ours.guide` regenerates the same way as
gcd's. Then the same rerun + `git diff` check.

The delegated run on picorv32 is the first end-to-end exercise of our
6,744-net guide through DRT — scoping only covered gcd. Whatever it
does (0 DRC, nonzero DRC, antenna hits, a mid-route failure) is the
recorded result.

## 5. Docs

`docs.md` spells out the `docs/results.md` entry, the
`tests/designs/README.md` rows, and the `docs/roadmap.md` §E6 line.
The results entry names every deviation from the P-GR scoping record
(P-GR ran on the full guide only on gcd; this ticket adds picorv32,
the baseline arm, antennas, wirelength/via metrics, and runtime).

## 6. `.gitignore`

Add the `tests/designs/<d>/<d>.drt.*.def` block from `golden.md`. The
`runs/drt/` scratch needs no new rule.

## 7. Verify

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`

Both must pass — nothing Cargo-visible changed; the results entry
states that and that no version bump applies (reference-only ticket).

## Done-when map (acceptance checklist)

| Acceptance item | Where handled |
|---|---|
| `generate_drt_ref.sh` runs both variants in the pinned image for gcd + picorv32; rerun byte-reproduces or names the varying field | `script.md`; `golden.md` §determinism; step 3/4 |
| `<d>.drt.reference.json` committed: DRC, antenna, wirelength, per-layer vias, routed-DEF SHA prefix + size, runtime — all script-emitted | `golden.md`; step C |
| delegated run recorded verbatim incl. nonzero DRC or failure | `script.md` step B note; `docs.md` |
| routed DEFs gitignored + SHA/size in JSON; reports committed if < 1 MB | `golden.md` table; step 6 |
| `docs/results.md`: commands verbatim, delta table, rerun count, deviations from scoping | `docs.md`; step 5 |
| `cargo test` + `clippy -D warnings` pass; no version bump stated | step 7 |

## Files

Implementation is expected to touch:

- `tests/designs/generate_drt_ref.sh` (new)
- `tests/designs/gcd/gcd.drt.reference.json`
- `tests/designs/picorv32/picorv32.drt.reference.json`
- `tests/designs/gcd/gcd.drc.*.rpt`
- `tests/designs/gcd/gcd.antenna.*.rpt`
- `tests/designs/picorv32/picorv32.drc.*.rpt`
- `tests/designs/picorv32/picorv32.antenna.*.rpt`
- `tests/designs/gcd/gcd.drt.*.def` (gitignored)
- `tests/designs/picorv32/picorv32.drt.*.def` (gitignored)
- `.gitignore`
- `tests/designs/README.md`
- `docs/results.md`
- `docs/roadmap.md`
