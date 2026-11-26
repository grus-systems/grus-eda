# e6-drt-ref — Overview

E6 ticket 1: commit the detailed-routing delegation reference — a
reproduction script plus its measured surface — so later E6 tickets can
quantify "our guides → OpenROAD DRT" as the delegation product. The path
is already proven: P-GR (`docs/results.md` 2026-11-22
next-stage-scoping) ran `read_guides` + `detailed_route` on a foreign
guide file and got a 0-DRC routed DEF on gcd. This ticket makes that
run reproducible on both designs and records both variants verbatim —
no `grus-eda` code changes.

## Deliverables

- `tests/designs/generate_drt_ref.sh` — the committed generator
  (`script.md`). `<design>` as `$1`, host wrapper re-invoking itself
  inside `ghcr.io/librelane/librelane:3.0.16`, mirroring
  `tests/designs/generate_irdrop_ref.sh` / `generate_cts_ref.sh`.
- `tests/designs/gcd/gcd.drt.reference.json` +
  `tests/designs/picorv32/picorv32.drt.reference.json` — per variant
  (`baseline`, `delegated`): DRC count, antenna count, routed-DEF
  wirelength, per-cut-layer via counts, routed-DEF SHA-256 prefix +
  size, runtime — every value emitted by the script itself
  (`golden.md`).
- `tests/designs/<d>/<d>.drc.<variant>.rpt` and
  `<d>.antenna.<variant>.rpt` — committed when < 1 MB (expected: empty
  or a handful of lines), otherwise left under the gitignored
  `runs/drt/` with size/SHA recorded in the JSON.
- `tests/designs/<d>/<d>.drt.<variant>.def` — routed DEFs, always
  gitignored (> 1 MB on picorv32); SHA-256 prefix + size in the JSON.
- `.gitignore` — `tests/designs/<d>/<d>.drt.*.def` lines.
- `docs/results.md` entry, `tests/designs/README.md` artifact rows,
  `docs/roadmap.md` §E6 progress line.

## Key decisions

- **Two variants, one script run, two openroad processes.** Clean-state
  per variant (the 9b/9c / `generate_cts_ref.sh` step-C precedent):
  process A runs the **baseline** (the reference's natural input — the
  exact step-5b spelling from each `generate_ref.sh`: `set_routing_layers
  -signal met1-met5 -clock met3-met5`, `global_route -guide_file
  runs/drt/<d>.baseline.guide -congestion_iterations 100`, then
  `detailed_route -output_drc ... -verbose 0`); process B runs the
  **delegated** variant (same prelude, then `read_guides
  runs/guide-check/<d>.ours.guide` in place of `global_route`, same
  `detailed_route`). Both end with `check_antennas` + `write_def`. A
  nonzero DRC count or a failed command on the delegated path is
  recorded data, not a blocker — the parser fails only on missing
  artifacts or unpinned grammar.
- **Baseline is step 5b verbatim, not step 5c.** Step 5b (which
  produced the committed/gitignored `<d>.routed.def` we already treat
  as reference truth) does **not** set
  `set_global_routing_layer_adjustment`; step 5c (the committed guide
  golden our `route --guide-out` is scored against) does. The baseline
  reproduces the reference's own routed result, and the JSON records a
  SHA cross-check between `<d>.drt.baseline.def` and the prior
  `<d>.routed.def` — byte-identity would be strong determinism
  evidence; a divergence is recorded verbatim, not smoothed.
- **Ours guide stays at `runs/guide-check/<d>.ours.guide`.** The spec's
  `<d>.guide.ours` name resolves to the actual emitted path; the
  regeneration dependency is `tests/designs/generate_guide_ours_spef.sh
  <design>` (host-side, release build — the spec's
  `generate_guide_ours.sh` name does not exist; this is the real
  script). The host wrapper invokes it when the guide is absent, which
  also re-emits the deterministic `<d>.guide.ours.spef`.
- **Metrics from the emitted artifacts, verbatim from the tool.**
  `drc_violations` is the non-blank line count of the `-output_drc`
  report (empty file = 0, matching the `generate_ref.sh` convention);
  `antenna_violations` comes from the `check_antennas` report/log
  grammar pinned at run time; wirelength and per-via-name counts are
  computed by parsing the tool's own routed DEF (`golden.md` documents
  the arithmetic). Any DRT self-reported summary lines are captured
  verbatim into `drt_summary_lines` for corroboration.
- **`timing` is the named non-reproducing field.** `clock milliseconds`
  around each phase inside the Tcl gives `global_route_s` /
  `read_guides_s` / `detailed_route_s` / `check_antennas_s` /
  `total_route_s`. Everything else in the JSON is expected to
  byte-reproduce on rerun (TritonRoute is deterministic on identical
  inputs — the committed-script rule, §7.1, applies: run twice, `git
  diff`, and any diff outside `timing` is a named residual or a bug).
- **Reports in the design dir only when committable.** The Tcl emits
  `-output_drc`/antenna output under the gitignored `runs/drt/`; the
  python step copies them up to `<d>.drc.<variant>.rpt` /
  `<d>.antenna.<variant>.rpt` when < 1 MB (`committed: true`), else
  records `committed: false` with size + SHA-256 prefix — a flooded
  report is data, not a failure.

## Files

Implementation is expected to touch:

- `tests/designs/generate_drt_ref.sh` (new)
- `tests/designs/gcd/gcd.drt.reference.json` (generated, committed)
- `tests/designs/picorv32/picorv32.drt.reference.json` (generated, committed)
- `tests/designs/gcd/gcd.drc.*.rpt`, `tests/designs/gcd/gcd.antenna.*.rpt` (generated, committed if < 1 MB)
- `tests/designs/picorv32/picorv32.drc.*.rpt`, `tests/designs/picorv32/picorv32.antenna.*.rpt` (generated, committed if < 1 MB)
- `tests/designs/gcd/gcd.drt.*.def`, `tests/designs/picorv32/picorv32.drt.*.def` (generated, gitignored)
- `.gitignore`
- `tests/designs/README.md`
- `docs/results.md`
- `docs/roadmap.md`
