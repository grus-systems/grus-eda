# e4-guides-emit — Metrics (done-when 1–2)

Exact definitions as the test computes them, so the `docs/results.md`
entry and the assertions measure the same thing. Criteria text is from
`kurena/tasks/next-stage-scoping/design/done-when.md` (E4 rows 1–2); both
bounds are *provisional* — "tighten once measured" applies.

## Shared setup

- Ours: `route --tlef --lef --def <d>.placed.def --guide-out <tmp>` output,
  parsed by `parse_guides` (or the `NetGuide`s used directly).
- Reference: committed `tests/designs/<d>/<d>.guide`, parsed by the same
  `parse_guides`.
- Net matching: guide net names resolved through `escape_def_name`
  round-trip against `design_db.net_names`. A reference net absent from
  ours counts share 0 and is listed; our nets absent from the reference
  count into our box total and are listed.

## Metric 1 — per-net guide-box overlap share

For each net present in the reference guide:

- a reference box `(rect, layer)` is **covered** iff at least one of our
  boxes for the same net has a DBU rect with positive-area intersection:
  `xl < r.xh && r.xl < xh && yl < r.yh && r.yl < yh`. Layer is **not**
  compared (the criterion says "GCell rect intersects", no layer
  qualifier); a layer-aware share is computed alongside as a diagnostic
  line in the test output and results entry.
- per-net share = `covered / ref_box_count` (0 for missing nets).

Report per design: box-weighted share (Σ covered / Σ ref boxes), per-net
median, per-net minimum, count of nets below 0.70, and the worst-5 nets by
share with names. **Gate: box-weighted share ≥ 0.70 on each design.**

If the gate fails: do not loosen silently — name the diff root cause in
`docs/results.md` (per-net share distribution shows where: stub layers,
strip merging, or route divergence), then set the test bound to the
measured value with a comment pointing at the entry. The provisional
character of 0.70 is documented in `done-when.md`; honest naming beats
green paint (AGENTS.md §7.5).

## Metric 2 — guide-box count (wirelength proxy)

- `total_ref` / `total_ours` = box counts over all signal nets in each
  file (reference includes its stub boxes; ours includes ours).
- **Gate: `|total_ours − total_ref| / total_ref ≤ 0.15` on each design.**
- Per-net comparison (AGENTS.md §7.5 — a small total must not hide a
  systematic per-net error): per-net `delta = ours − ref` and relative
  `ours/ref`; report median |relative − 1|, p90 |relative − 1|, max
  |delta| with net names, and the sign split (how many nets over vs
  under). The test prints these; `docs/results.md` records them. A
  passing total with a lopsided per-net picture (e.g. all nets
  systematically +2 stub boxes) is a named root cause, not a pass.

## `docs/results.md` entry shape

One dated entry `e4-guides-emit` recording, in existing style:

- commands run: our `route` invocation per design, `generate_ref.sh` step
  5c per design (naming which steps ran, §7.9), `check_guide_readback.sh`;
- per design: ref/ours box totals + delta %, box-weighted + median per-net
  overlap shares, per-net delta stats, runtime of `route` (load, route,
  emit separately if the timing report gives it);
- the ≥0.70 / ≤15% verdicts with actual numbers; if below the provisional
  bound, the named root cause;
- `read_guides` outcome (accepted / error text) and whether
  `estimate_parasitics -global_routing` was exercised on the read-back
  file;
- `picorv32.guide` byte size + the >1 MB commit-deviation note;
- the fail-proof mutation that was applied and restored (tests.md).
