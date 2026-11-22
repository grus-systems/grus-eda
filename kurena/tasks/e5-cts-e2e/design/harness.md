# e5-cts-e2e — Comparison harness

All glue lives in `cts/mod.rs`'s `#[cfg(test)] mod tests` next to the
existing T1–T7 helpers (`load_design`, `emit_def_string`,
`sta_pin_name`, `net_by_name`). Nothing is added to `analysis.rs`; the
differences from `compare_design_files` are enumerated below.

## 1. The ours-side run

Factor the body of `propagated_sta_on_cts_def` into a shared runner so
T5, the e2e gate, and the decomposition share one code path:

```rust
/// load -> load_sdc -> run_cts -> emit DEF -> re-parse -> met2
/// placement parasitics -> run_sta_with_state. Returns everything the
/// gate and the prints need.
fn cts_sta_run(design_name: &str)
    -> (Design /* {tech, lib, db: pre-CTS} */, CtsReport, String /* emitted */,
        DesignDb /* re-parsed */, StaReport, Vec<PinTimingState>)
```

Steps (all existing calls):

1. `load_design(name, "<d>.placed.def")` — committed placed DEF
   (`require`d, loud when the gitignored picorv32 input is missing).
2. `load_sdc("<d>.cts.sdc")` — the propagated-clock SDC; the *same* file
   feeds `run_cts` (clock-root resolution) and the STA run, matching
   reference step C.
3. `run_cts(&mut db, &tech, &lib, &sdc, &CtsConfig::default())`.
4. `emit_def_string` → write to temp → `parse_def` → `db2`. The emitted
   text is the comparison object, not the in-memory `db` — the
   reference golden was likewise collected on a DEF read back from
   disk.
5. `WireRc::from_layer(&tech, "met2")` +
   `estimate_placement_parasitics(&db2, &tech, rc)` — the same model
   class reference step C used (`set_wire_rc -layer met2` +
   `estimate_parasitics -placement`; E3.4 proved our PD trees
   edge-identical to OpenROAD's).
6. `run_sta_with_state(&db2, &lib, &sdc, Some(&par))` → `(report,
   state)`.

The reference-DEF variant for the decomposition is the same minus
`run_cts`/emit: `parse_def("<d>.cts.def")` → steps 5–6.

## 2. Golden consumption

Two `serde_json::Value` reads per design:

- `<d>.cts.sta.reference.json` → `["cts_placement_rc"]`:
  `wns_max_ns`/`tns_max_ns`/`wns_min_ns`/`tns_min_ns`, `endpoints[]`
  (`pin`, `max_arr_ns`, `max_req_ns`, `max_slack_ns`, `max_slew_ns`,
  `min_*` — no nulls on either committed golden, verified), and
  `clock_pins[]` (`pin`, `arr_rise_max_ns`, …).
- `<d>.cts.reference.json` → `skew_setup_ns`, `skew_hold_ns`,
  `latency_max_ns`, `latency_min_ns`, `inserted_instances`,
  `tree_buffers`, `dummy_loads`, `clock_net_wirelength_um`, `sinks`,
  `tree_depth` (the honesty-gate fields). A small
  `#[derive(Deserialize)] CtsRefJson` keeps field typos loud —
  `serde` derive is already a grus-core dependency.

## 3. Per-endpoint comparison

Mirrors `compare_design_files` in shape, simplified:

- `report.endpoints.len() == golden endpoints.len()` and every golden
  `pin` resolves by name (both directions checked via a `HashMap`) —
  assert loudly; the endpoint set is a property of the register
  inventory, which both DEFs share.
- Per endpoint, the eight fields off `EndpointReport`
  (`max_arr_ns`, …, `min_slew_ns`): `ours` is `Option<f32>`;
  `Some` vs numeric golden → `within_tolerance`; `None` vs numeric →
  failure; `None` vs `null` → pass (none on these goldens — the arm
  exists so a future `null` is not a panic).
- `within_tolerance(ours, ref, rel, abs)`: `|ours − ref| <=
  max(abs, rel·|ref|)` — copy the five-line helper; do not import the
  analysis one (private to its `mod tests`).
- WNS/TNS: `report.wns_max_ns` etc. vs the golden scalars with the
  same Tol (slack bound for WNS, `tns_rel`/`tns_abs` for TNS).
- `EndpointReport` carries `pick_worst_polarity`'s tuple — the exact
  values the analysis harness compares by default. What is **not**
  reproduced is `resolve_tied_polarity` (a ≤2 ps rise/fall slack tie
  where our pick and OpenSTA's pick differ would surface as a spurious
  multi-field diff). Contingency if a failure is diagnosed as exactly
  that: `pub(crate)` `run_sta_detail` + `EndpointPolarities`/`Worst`/
  `pick_worst_polarity` and a local `resolve_tied_polarity` — same
  visibility precedent as `run_sta_with_state`. Recorded, not assumed.
- Max-diff-per-field + worst-5 absolute-diff rows print on every run
  (`--nocapture`), feeding `docs/results.md`.

## 4. Clock-pin comparison (criteria 1–2 surface)

- Golden `clock_pins` pin names vs the original-sink set (non-driver
  pins on the resolved root nets, pre-mutation — `sta_pin_name`):
  **exact set equality** asserted (35 gcd / 1,597 picorv32). Both are
  "register `is_clock` pins" on the same register inventory; an
  asymmetric member is a finding, not a tolerance case.
- Every golden pin resolves in `db2` and has a reached
  `state[p].arrival_max[RISE]`.
- Per pin: `our_lat(p) = state[p].arrival_max[RISE]` (the active edge —
  fall arrivals sit a half-period out, same convention the reference
  census Tcl used).
- Aggregates:
  - `our_skew = max_p − min_p`; `ref_skew = latency_max_ns −
    latency_min_ns`. Gate: `|our − ref| <= max(0.025, 0.25·ref)` ns.
  - `our_lat_max` vs `latency_max_ns`: `|diff| <= 0.15·ref`.
- Per-pin sanity gate: `|our_lat(p) − arr_rise_max_ns(p)| <=
  max(0.025, 0.15·ref_pin)` per pin — a degenerate-tree catch
  (one-buffer-per-sink spreads latency far wider), *not* the
  criterion; criterion 1 is the aggregate. Recorded as such in
  `docs/results.md`.
- The `clock_pins` per-pin table (ours vs golden vs diff) prints
  worst-5 — the skew decomposition's raw material.

`skew_hold_ns` prints alongside; on both committed references it
equals `skew_setup_ns` (same worst pair) — assert nothing on it.

## 5. Tolerances

A tiny local `CtsTol { arr_rel, arr_abs_ns, slack_abs_ns, slew_rel,
tns_rel, tns_abs_ns }` mirrors `Tol`'s shape (same field names so the
results table reads identically):

| design | arr_abs | slack_abs | slew_rel | arr_rel | tns |
|---|---|---|---|---|---|
| gcd | 0.005 ns | 0.005 ns | 0.05 | 0.01 | rel 0.01 / abs 0.005 |
| picorv32 | 0.006 ns | 0.040 ns | 0.10 | 0.01 | rel 0.025 / abs 0.005 |

- gcd slew inherits the `GCD_PLACEMENT` 5% convention: both engines
  ran placement RC, so the slew residual is the recorded cell-stage
  gap, not a criterion field (the criterion bounds arrival/required/
  slack; slew is measured and reported regardless).
- picorv32's are the recorded `PICORV32_SPEF` floors verbatim — "its
  recorded floors" in the acceptance.
- **If a criterion bound fails, the bound does not move.** Measure →
  decompose (`decomposition.md`) → name the cause → hold the measured
  value in a doc-commented constant (`PICORV32_CTS_*`, the
  `PICORV32_GUIDE` precedent) with the named cause in
  `docs/results.md`. The floor constants live next to `CtsTol`.

## 6. Honesty-gate print (criterion 4)

The gate test prints, per design, a side-by-side of `CtsReport`
vs `CtsRefJson`: `inserted_instances` (with the reference's
tree+dummy decomposition), `tree_buffers` total and per-master,
`dummy_loads` (ours always `{}` — named, not hidden),
`clock_subnets`, `sinks`, `tree_depth`, `clock_net_wirelength_um`,
`avg_sink_wirelength_um` (flagged: different quantity, already
recorded in `docs/e5-cts.md` §3). Print-only; the `docs/results.md`
table is written from it.
