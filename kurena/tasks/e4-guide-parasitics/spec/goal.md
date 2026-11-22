# Goal

E4 ticket 2 — done-when criterion 3
(`kurena/tasks/next-stage-scoping/design/done-when.md`):
guide-based parasitics. Build a `NetRcTree` per net from
the emitted guide boxes and compare against OpenROAD's
`estimate_parasitics -global_routing` in **both
directions**:

- **(a) our model on their guides**: `parse_guides` on
  the committed `gcd.guide`/`picorv32.guide` → our
  guide-RC model → per-net total capacitance vs
  `estimate_parasitics -global_routing -spef_file` on the
  same guide file: within `max(5%, 1 fF)` per net.
- **(b) their model on our guides**: our emitted guide →
  `read_guides` → `estimate_parasitics -global_routing`
  (the `gcd.ours.guide.spef` flow already exercised in
  e4-1) vs the same run on OpenROAD's own guide file —
  same bound; this cell measures guide quality
  downstream, not our RC code.

Both directions are measured and recorded; the pass gate
is (a). Residuals named per AGENTS §7.5 — per-net, not
just totals.

## Mechanism (design freedom inside)

- Guide box → wire model: first **probe what OpenROAD
  emits** (`estimate_parasitics -global_routing
  -spef_file` on a tiny committed case — record the
  per-net `*D_NET` cap and RC structure: lumped? lumped
  per box? pi?). Then implement the matching model on
  our guide boxes: per-layer length from box span,
  per-unit R/C from LEF layers (the `Rc`/`PiModel`
  machinery in `sta/parasitics.rs`), topology across
  GCells matching the committed 2D route.
- Emit `--spef-out` (or a `power`-style flag) using the
  existing `write_spef` so `sta --spef` can consume it —
  criterion 4's harness plugs in next ticket, keep the
  plumbing obvious.
- Our parasitic model stays unit-consistent with the
  SPEF path (same `NetParasitics` type, `pin_caps` —
  reuse, don't fork).

## Measurement

`docs/results.md` entry: the probe of OpenROAD's guide
SPEF shape, per-net cap comparison tables (a) and (b) on
gcd + picorv32, fraction of nets inside `max(5%, 1 fF)`,
worst offenders with root cause. Anchors: guide-SPEF
total cap on gcd ≈ 0.7397 pF (between placement 0.4406
and routed 0.8962 — scoping measured).

## Files

- `crates/grus-core/src/congestion/guide.rs` or
  `sta/parasitics.rs` (guide→NetRcTree builder), `main.rs`
  CLI surface
- `tests/designs/{gcd,picorv32}/` (guide SPEF references,
  committed via script like e4-1)
- `docs/results.md`, `docs/roadmap.md`, `docs/e4-guides.md`
- `Cargo.toml` (bump per change size)
