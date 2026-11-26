# e5-cts-tree — Overview

E5 ticket 3: the clock-tree constructor. Consumes the committed TritonCTS
references (`e5-cts-ref`: `<d>.cts.def`, `<d>.cts.reference.json`,
`<d>.cts.sta.reference.json`, `<d>.cts.sdc` on gcd + picorv32) and the
`DesignDb` mutation API (`e5-insert-api`, v0.4.0) to build a depth-balanced
buffer tree on each design clock's root net, legalize the inserted cells,
and emit a CTS DEF plus a census report. The strict e2e STA comparison
against the TritonCTS DEF is ticket 4 (`e5-cts-e2e`) — this ticket records
propagated-clock skew/insertion delay as informational values only.

## What exists

- `DesignDb` mutation surface (`design_db.rs`): `insert_cell`,
  `create_net`, `connect_pin`, `disconnect_pin`, `insert_buffer_on_net`,
  invariants I1–I6; `write_def_full` emits the mutated design generically
  (proven: `test_mutation_def_roundtrip`).
- `Sdc::clocks` / `Clock::port` are `pub` (`sta/sdc.rs:293-357`) — the
  same data `analysis.rs:1396-1406` uses to seed the clock-network walk.
  No SDC re-parse is needed, and none may be added (goal).
- `legalize_placement` + `verify_legality` (`placement/legalize.rs:980`,
  `:1669`). `legalize_placement` itself calls `verify_legality` on its
  output (`:1654`).
- Propagated-clock STA (`run_sta`, `sta/analysis.rs`) matches OpenSTA on
  propagated clocks (gcd/picorv32 `*.propagated.reference.json`, STRICT).
  Per-pin arrival state is available in-crate via the `#[cfg(test)]`
  `run_sta_with_state` — see `api.md` for the accessor it needs.
- TritonCTS census on the same inputs (`docs/results.md` 2026-11-22
  `e5-cts-ref`): gcd 5 tree bufs (`clkbuf_16`×5) + 3 dummies, 35 sinks,
  depth 2, clk HPWL 370.244 µm, latency 0.3132–0.3146 ns, skew −1.37 ps;
  picorv32 197 tree bufs + 56 dummies, 1,597 sinks, depth 4, clk HPWL
  12,749.174 µm, latency 0.757–0.793 ns, skew −34.16 ps.

## Verified input facts

- `gcd.placed.def` `clk` net = `( PIN clk )` + 35 `_*_ CLK` sinks;
  `picorv32.placed.def` `clk` net = `( PIN clk )` + exactly 1,597
  `_*_ CLK` sinks (grep-verified in the seeded worktree). Both designs
  have one design clock (`core_clk` on port `clk`), no clock gating, no
  clock-as-data taps — every non-driver pin on the root net is a register
  clock pin, so "all non-driver pins are sinks" coincides with the
  reference's `all_registers is_clock` sink count.
- `write_def(db, …)` — the template-splicing writer used by `place` —
  replaces **only** the COMPONENTS section and re-emits the original
  NETS verbatim (`readers/def.rs:983-1050`). It cannot emit a mutated
  netlist; CTS must use `write_def_full`.

## Key decisions (details in `algorithm.md` / `api.md`)

- **Topology: count-equalized median bisection, bottom-up.** Each level
  partitions its load pins into `ceil(n / max_loads)` spatially coherent
  groups (`max_loads = 8`), so every sink passes through exactly
  `tree_depth` buffers — depth balance by construction, load balance to
  ±1 pin. Estimate: gcd ~6 bufs / depth 2; picorv32 ~230 bufs / depth 4 —
  the TritonCTS league (5+3 / depth 2, 197+56 / depth 4), not
  one-buffer-per-sink. A PD-tree/RSMT backbone was the alternative
  (scoping named it); rejected: it optimizes wirelength, and equal-depth
  grouping is what bounds skew — recorded choice.
- **Fanout via primitive ops, not `insert_buffer_on_net`.** The
  convenience method moves *all* sinks of a net into one new net — a
  k-way group split needs `insert_cell` + `create_net` + `connect_pin`
  directly. Same tested primitives; the goal's "insert via
  `insert_buffer_on_net`" names the API family, and the deviation is
  recorded here.
- **Pin-and-place legalization.** `legalize_placement` moves every
  `Placed`/`Unplaced` `Core` cell — freely legalizing would shift
  pre-existing cells and break the byte-identity acceptance. CTS
  temporarily marks pre-existing movable cells `Fixed`, legalizes only
  the inserted buffers, then restores `Placed`. Consequence recorded:
  OpenROAD legalizes the whole neighborhood (its `gcd.cts.def` shifts
  pre-existing cells); ours pins them, so buffers may land farther from
  their cluster in dense regions — at gcd/picorv32 utilization (~60-65%)
  free sub-row gaps exist near any seed.
- **Sink set = every non-driver pin on each clock root net** — matches
  the reference sink counts exactly on both designs (35 / 1,597).
  Designs with clock-as-data taps or pre-existing buffer fragments get a
  documented behavior, not disambiguation (`algorithm.md` §Limitations).
- **STA accessor, not new STA API.** Clock-pin arrivals come from
  `run_sta_with_state` — today a module-private `#[cfg(test)]` fn in
  `sta::analysis`. The cts tests make it `pub(crate)` (keeping
  `#[cfg(test)]`) plus `pub(crate)` on `PinTimingState` and its
  `arrival_max`/`arrival_min` fields. This is the spec's "only if a
  missing accessor surfaces" case — the accessor surfaced in
  `sta/analysis.rs`, not `design_db.rs` (no DesignDb change is needed).
- **`write_def_full` for emission** (mutated netlist; `write_def` is
  template-only). Recorded emission deltas vs TritonCTS: no
  `+ SOURCE TIMING`, no `+ USE CLOCK`, no `+ NONDEFAULTRULE`; TRACKS,
  GCELLGRID, VIAS, SPECIALNETS are not emitted (pre-existing writer gap;
  `+ SPECIAL` PINS ports also dropped — the insert-api entry records
  `num_pins − 2` on re-read).
- **Report = census only.** The JSON carries the criterion-4
  honesty-gate fields that mirror `<d>.cts.reference.json`
  (`report.md`); `skew_*`/`latency_*`/`cts_wire_unit` are omitted —
  they are STA-side or have no analogue — and arrive with ticket 4.
- **Version `0.4.0 → 0.5.0`** per the acceptance's explicit "minor".
  Same §5-patch-vs-minor discrepancy as `e5-insert-api` — the bump is
  named in the commit body and `docs/results.md`, not silent.

## Files

Implementation is expected to touch:

- `crates/grus-core/src/cts/` (new module — `mod.rs` public surface +
  `tree.rs` constructor + `#[cfg(test)]` tests)
- `crates/grus-core/src/lib.rs` (`pub mod cts;`)
- `crates/grus-core/src/sta/analysis.rs` (visibility only —
  `pub(crate)` on `run_sta_with_state`, `PinTimingState`,
  `arrival_max`/`arrival_min`; the spec's missing-accessor clause)
- `crates/grus-eda/src/main.rs` (`cts` subcommand + CLI parse tests)
- `Cargo.toml` (`[workspace.package] version` 0.4.0 → 0.5.0)
- `docs/e5-cts.md` (new stage doc)
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`

No new files under `tests/designs/` — every input is already committed
(`<d>.placed.def`, `<d>.sdc`, `<d>.cts.sdc`, `<d>.cts.reference.json`,
`<d>.cts.sta.reference.json`); emitted DEF/report go to temp paths in
tests and to `--def-out`/`--report` on the CLI. `picorv32.placed.def` is
gitignored but seeded into the worktree (`workflow.toml`
`worktree_seed`); `require` panics loudly if absent.
