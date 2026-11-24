# e6-drt-e2e — docs updates

## `docs/e6-drt.md`

- **§7 recorded-looseness inventory**: update the host-cell OBS
  carve entry — it stays (approach corridor into exempted pin-rect
  nodes) but the unconditional version that let wires cross own
  instance internals is superseded by the instance-hull mask;
  record the mask, the `pin_obs_rects` addition, and which ladder
  variant (strict / L1 painted-layers / L2 corridor) shipped with
  its measured residual counts.
- **New e2e section**: the container gate result — per-rule
  `lef_expressible` counts on ours vs placed vs delegated
  (all-zero table), `magic_only` decomposition verdict, the two
  unclassified names with per-input counts, antenna surface.
- **Claim boundary**: claimed — Magic `drc(full)` clean on the
  `lef_expressible` subset for our routed DEF under committed
  guides; NOT claimed — `count_flat` (insensitive counter),
  `drt_report` (n/a for the native router), any `magic_only`
  residual names, unrouted residual classes by name.

## `docs/results.md`

One dated entry:

- The RED first-contact measurement (7 rules, ~3025 fragments, 2
  unclassified names, self-DRC=0 proof) — the gate's demonstrated
  red arm, kept as failure log per convention.
- The model fix + which ladder variant landed, with
  `hull_masked` / unrouted-rise census numbers.
- The green table: per-rule `lef_expressible` `{ours, placed,
  routed_ref}` rows (all-zero), `magic_only` `inherited` verdicts,
  unclassified counts.
- The `delta` block as rows: wirelength µm + %, via total +
  per-cut-layer, antenna, `signal_nets_routed` — vs baseline and
  vs delegated, with named explanations for outliers (e.g.
  wirelength divergence from hull detours, via-count deltas from
  island-pin conns).
- Which verify arms ran: `generate_drt_ours.sh --verify`
  (`OURS-DETERMINISTIC <sha>`), `check_drt_ours.sh` per design,
  `cargo test` legs (including the ignored picorv32 leg run
  manually), clippy.

## `docs/status.md` / `docs/roadmap.md`

E7-style done line for E6's native-DRT subset scope: "routed under
committed guides, Magic `drc(full)` clean on the `lef_expressible`
subset" + named residuals if any remain. Honest wording: if
unrouted residuals exist the line names them, same as E8's
convention.

## Files

- `docs/e6-drt.md`, `docs/results.md`, `docs/status.md`,
  `docs/roadmap.md`
