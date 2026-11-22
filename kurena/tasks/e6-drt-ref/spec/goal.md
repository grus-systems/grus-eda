# e6-drt-ref — Detailed-routing delegation reference

E6 ticket 1 (reference generation, no Rust work). The delegation path
was already proven in scoping (`read_guides` + `detailed_route` on a
foreign guide → 0-DRC routed DEF on gcd). This ticket makes that path
reproducible and commits its measured surface so later tickets can
quantify "our guides → OpenROAD DRT" as the E6 product.

## Scope

1. Committed script `tests/designs/generate_drt_ref.sh <design>` in the
   pinned image (`ghcr.io/librelane/librelane:3.0.16`), self-invoking
   like `generate_irdrop_ref.sh`.
2. Two route runs per design:
   - **baseline**: OpenROAD's own `global_route` → `detailed_route`
     (the reference's natural input)
   - **delegated**: `read_guides <design>.guide.ours` → `detailed_route`
     — our E4 guide file driving the reference DRT (the sellable path)
3. Per run capture: `detailed_route -output_drc` report, `write_def`
   routed DEF, `check_antennas` report, reported total wirelength and
   per-layer via counts, runtime.
4. Machine-readable `<design>.drt.reference.json`: per variant,
   { drc_violations, antenna_violations, total_wirelength_um(?),
     via_count_total + per cut layer, routed_def_sha/sha-prefix + size,
     runtime_s }. Routed DEFs are >1MB → gitignore, commit SHA-256
   prefix + size; DRC/antenna reports commit if <1MB.
5. Both designs: gcd and picorv32 (`picorv32.guide.ours` regenerates
   via `generate_guide_ours.sh` if absent — the script documents the
   dependency).
6. Compare delegated vs baseline: DRC counts (expect both 0 on gcd —
   record actual), wirelength/via-count deltas, runtime. Record
   verbatim, no smoothing.
7. `docs/results.md` entry: commands, numbers, artifact table with
   sizes + SHA prefixes, rerun determinism check (run twice, diff
   JSON).

## Non-goals

No `grus-eda` side change beyond optionally accepting/emitting the
routed DEF for downstream tickets — the DEF reader's NETS `ROUTED`
storage is a separate ticket (`e6-routed-def` if needed). This ticket
is container-side only.

## Files

- `tests/designs/generate_drt_ref.sh`
- `tests/designs/<d>/<d>.drt.reference.json`
- `tests/designs/<d>/<d>.drc.*.rpt`, `<d>.antenna.*.rpt` (if <1MB)
- `.gitignore` for routed DEFs; `docs/results.md`
