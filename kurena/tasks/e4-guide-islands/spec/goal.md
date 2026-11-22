# Goal

Fix the dominant E4 e2e residual named in the e4-sta-e2e
decomposition (`docs/results.md` 2026-11-22): **island
semantics**. OpenROAD's `-global_routing` model leaves
unpaired guide-stub components electrically dead —
self-loop resistors in the SPEF, no wire RC into the
Elmore path. Our `NetRcTree` forces one tree, so
`estimate_guide_parasitics` currently links each
disconnected component into the driver tree through
`SPEF_STUB_RES_KOHM` — electrically transparent R but the
island's wire RC enters the path (gcd +201.7 ps median /
picorv32 +1408.0 ps median on the parasitics axis).

## Work

- Match OpenROAD: components unreachable from the
  driver's tree get emitted as **self-loop resistors**
  (`n n 0.00x` style per the probed `_000_:6 _000_:6`
  pattern), carrying their caps as *CAP on their nodes
  but no inter-component R. The per-net `*D_NET` cap must
  stay identical (e4-2 gate keeps passing 100%).
- Also reduce the island population at the source:
  `build_guides` stub pairing — our guide emits ~4.3x
  more unpaired stubs than OpenROAD's (gcd 212 vs 152
  islands / 1,389 vs 325 self-loops). Measure the stub
  classes and fix the dominant one (e.g. pairing a stub
  with an adjacent `L+1` segment the way `updateVias`
  expects, layer coverage of mid-cell-ending boxes).
  Keep e4-1's topology gates (overlap ≥0.70, boxes ±15%)
  inside bounds.
- Re-measure the e4-3 e2e gate after the fix; record
  the new decomposition numbers — expectation per the
  attribution: parasitics-axis median should collapse
  toward the engine band, total residual then dominated
  by the topology axis.

## Files

- `crates/grus-core/src/sta/parasitics.rs`
  (`estimate_guide_parasitics` island handling),
  `crates/grus-core/src/congestion/guide.rs` (stub
  pairing) — one ticket because the fix needs both sides
  measured together
- `tests/designs/{gcd,picorv32}/` (regenerate
  `*.guide.ours.spef` + `*.guide.sta.reference.json` if
  they move; committed via existing scripts)
- `docs/results.md`, `docs/e4-guides.md`
- `Cargo.toml` (patch bump)
