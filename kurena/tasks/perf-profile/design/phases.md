# perf-profile — phase-boundary inventory

Where `timing::phase()` scopes go. Names are `<cmd>.<pass>[.<sub>]`; leaf
scopes inside a command sum to the command's own total minus untimed glue.
Anchors are current line numbers — place scopes by function, not by line.

## `sta` (`main.rs` + `sta/analysis.rs`)

CLI load phases (`crates/grus-eda/src/main.rs`, `Commands::Sta` arm):

| key | wraps |
|---|---|
| `sta.load.liberty` | `load_liberty` (+ `--lib-hold` second load, count x2) |
| `sta.load.lef` | `load_lef_tech_db` |
| `sta.load.def` | `parse_def` (x2 with `--lib-hold`) |
| `sta.load.sdc` | `load_sdc` |
| `sta.parasitics` | `estimate_placement_parasitics` or `spef_parasitics` (per corner) |
| `sta.spef.read` | `load_spef` (SPEF mode only) |
| `sta.spef.write` | `write_spef` + `fs::write` (`--spef-out` only) |
| `sta.report.json` | `--json` serde + write |

Engine phases (`crates/grus-core/src/sta/analysis.rs`):

| key | wraps | anchor |
|---|---|---|
| `sta.prepare.loads` | `set_load`/`get_nets` resolution block | `prepare_sta`, `sdc.net_loads()` loop |
| `sta.prepare.pi_models` | pi-model build (par ≥512) | `cx.pi_models` collect |
| `sta.prepare.wire_delays` | per-net Elmore/pi delay build | `wire_delays_per_net` collect |
| `sta.prepare.graph` | `build_graph` | call in `prepare_sta` |
| `sta.prepare.levels` | `topological_levels` (data order) | call in `prepare_sta` |
| `sta.prepare.ports` | input-port seed loop | `for &pin in &design.io_pins` |
| `sta.prepare.clock.slew` | `propagate_slew` (incl. its levelization retry) | call in `prepare_sta` |
| `sta.prepare.clock.walk` | `propagate_clock` | call in `prepare_sta` |
| `sta.prepare.clock.seed` | propagated consumer copy + ideal seeding loops | consumer/`clock_pins` loops |
| `sta.prepare.launch` | launch + latch `LaunchSeed` build | `launch_seeds`/`latch_seeds` collects |
| `sta.partition.ctx` | `ExceptionCtx` build (name→id maps) | `run_sta_partitioned` head to `ctx` |
| `sta.partition.rules` | selector resolution + `set_clock_groups` desugar + signature classes | rules/class_map section |
| `sta.partition.compose` | post-class endpoint composition | after the class loop |
| `sta.eval.seeds` | port/launch/latch seed merge | `eval_sta` head |
| `sta.eval.walk` | levelized data-arrival loop | `for lvl in …` |
| `sta.eval.endpoints` | `collect_endpoints` total | call in `eval_sta` |
| `sta.eval.endpoints.pulse` | `eval_pulse_width_checks` (inside `collect_endpoints`; also the merged-state call in the partitioned tail) | call sites |
| `sta.assemble` | `assemble_sta_report` | call in `run_sta_detail` |

Notes:

- `sta.eval.*` accumulates across signature classes; the report's count shows
  how many eval runs a design's SDC triggers (baseline single clock, no
  exceptions: 1).
- The `any_propagated || pulse_width_checks` block gates `clock.slew`/`walk`;
  ideal-clock runs without pulse checks skip both — the absent line is data.
- `sta.prepare.levels` covers the data-order levelization only;
  `propagate_slew`'s own `topological_levels(include_seq)` (plus the ibex-style
  retry) sits inside `sta.prepare.clock.slew`.

## `power` (`main.rs` + `power.rs` + `activity.rs`)

| key | wraps | anchor |
|---|---|---|
| `power.load.liberty` / `power.load.lef` / `power.load.def` / `power.load.sdc` | same readers as `sta` | `Commands::Power` arm |
| `power.parasitics` / `power.spef.read` | same as `sta` | `Commands::Power` arm |
| `power.slews` | `sta_pin_slews` (fills `sta.*` keys internally — instrumentation.md §Nested attribution) | `Commands::Power` arm |
| `power.activity.load` | `load_vcd` / `load_saif` + `saif.resolve` | `Commands::Power` arm |
| `power.activity.resolve` | `resolve_activity` | `estimate_power` head |
| `power.net_loads` | per-net `net_load_pf` loop | `estimate_power` |
| `power.cells` | per-cell leakage + `pin_internal_power` loop | `estimate_power` |
| `power.switching` | per-net switching loop | `estimate_power` |

Optional finer keys inside `resolve_activity` if the profile shows it
non-trivial: `power.activity.resolve.funcs` (`comb_function_table` +
`seq_function_table`), `.propagate` (`propagate` walk). Add only if the
coarse line is a hotspot — do not speculatively double the scope list.

## `place` (`main.rs` + `placement/`)

`place` already prints coarse load/GP/legalize/write `elapsed=` on stdout —
keep those lines unchanged; the stderr phases decompose inside them.

| key | wraps | anchor |
|---|---|---|
| `place.gp.init` | `WaWirelengthPrecomp::new` + `DensitySolver::new` + `cg_initial_place` | `run_nesterov_placement` head |
| `place.gp.density` | `compute_density_and_forces` calls (FFT Poisson solve) | per-iteration + final calls; count = iterations+1 |
| `place.gp.wl` | `compute_wa_wirelength_and_grad` calls | per-iteration |
| `place.gp.step` | gradient precondition + BB step + `compute_hpwl_um` + update | remainder of the loop body |
| `place.legal.abacus` | Step 1–3: obstacle subrows, coordinate conversion, cluster insertion | `legalize_placement` to `post_abacus_hpwl_dbu` |
| `place.legal.pre` | `cell_nets`/`PinGeomCache`/`cell_pin_nets` build | Step 4 head |
| `place.legal.pass1` | `equal_width_swap_pass_fast` | ×4 rounds |
| `place.legal.resync` | sub-row resync + `pin_x` rebuild | ×4 |
| `place.legal.pass2` | adjacent sub-row swaps | ×4 |
| `place.legal.pass3` | single-cell site sliding | ×4 |
| `place.legal.verify` | `verify_legality` | per call |
| `place.write` | `write_def` | `Commands::Place` arm |

Aggregate across rounds and iterations (count field), never per-iteration
lines — the goal's own granularity clause.

## `congestion` (`main.rs` + `congestion/`)

| key | wraps | anchor |
|---|---|---|
| `cong.load` | `load_lef_tech_db` + `parse_def` | `Commands::Congestion` arm |
| `cong.grid` | `GCellGrid::build` | `estimate_congestion` |
| `cong.decompose` | `decompose_design_nets` (RSMT, par ≥512) | `estimate_congestion` |
| `cong.route` | `route_segments` (pattern + probabilistic demand) | `estimate_congestion` |
| `cong.report` | ratio/report assembly | `estimate_congestion` tail |

`check-e0` is not instrumented — its existing `elapsed=` stdout lines are
the E0 record; the goal names only the heavy commands.

## Coverage rule

Every scope name that lands must appear in the results entry's phase table —
a scope nobody reads is noise. If a measured share lands inside untimed glue
(≥5% of a command), either add a scope or name the residue `other` in the
table and say so.
