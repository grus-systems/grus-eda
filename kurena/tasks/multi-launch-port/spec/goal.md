# Goal

Close the multi-launch coverage gap the `output-port-delays` run
recorded: `y` has exactly one launch source (`u_ff1/Q`), so the
worst-covered-candidate merge at an output port is exercised only
per-transition, never per-source. Extend the `sta_exceptions`
fixture so one output port is reachable from two sequential launch
sources, then golden the merged delay check.

## Fixture

`tests/designs/sta_exceptions/exceptions.def` — add one
`sky130_fd_sc_hd__mux2_1` (`u_mux2`) and one output port `y2`:

- `u_mux2/A0` <- `u_ff0/Q`, `u_mux2/A1` <- `u_ff2/Q`,
  `u_mux2/X` -> `y2`. Choose the A0/A1 assignment deliberately and
  say why in the design doc (mirroring `u_mux`'s post-swap
  orientation makes `u_ff0` the larger pure delay; the opposite
  repeats it — either is fine as long as the design records the
  predicted winner identities and verifies them against the
  regenerated golden).
- `u_mux2/S` may share the existing `sel` net (one more load on
  `sel`; all scenarios regenerate anyway) — a separate select is
  also acceptable if simpler.
- The extra sinks add fanout load to `d0`/`d2`/`sel`, so every
  scenario whose report touches those nets shifts — that is
  expected; all reference values come from the regeneration.
- Keep DEF self-contained: placements inside the 20000x20000 die on
  the site grid, `y2` PIN on the left edge without overlapping
  existing pin shapes, all cell/pin names resolvable in
  `sky130_fd_sc_hd`. Add a `set_output_delay` for `y2` in the
  shared preamble of every `.sdc` file (same values as `y`: min
  0.1 / max 0.7) so `y2` is a real checked endpoint, not a dangling
  net.

## Scenarios

New `ml_*`/`pml_*` files (shared preamble + `y2` output delays;
`pml_*` additionally carry `set_propagated_clock`), added to
`SCENARIOS` in `generate_ref.sh`:

- `ml_both` — `set_max_delay N -from [get_cells {u_ff0 u_ff2}] -to
  [get_ports y2]` (single command: one signature class merging both
  launches at the port).
- `ml_max0` / `ml_max2` — the same `-to [get_ports y2]` with a
  single `-from u_ff0` / `-from u_ff2`: per-source delay checks so
  the golden decomposes which candidate the merged rule reports.
- `ml_all` — `set_max_delay N -to [all_outputs]` (selector
  coverage; note it also covers `y`).
- `ml_min` — `set_min_delay N -from [get_cells {u_ff0 u_ff2}] -to
  [get_ports y2]` (min side).
- `pml_both` / `pml_min` — propagated-clock variants, same shape
  as `pm2_*`/`pmo_*`.

Regenerate `exceptions.reference.json` by running the committed
`generate_ref.sh` inside `ghcr.io/librelane/librelane:3.0.16`
(docker is available). Every value comes from that run — never
hand-write or round.

## Verification and docs

- Confirm from the regenerated numbers that `y2`'s merged rule
  reports the worst covered candidate (compare `ml_both` against
  `ml_max0`/`ml_max2` and `base_prop`'s `y2` arrival). If the
  composition in `crates/grus-core/src/sta/analysis.rs` diverges
  from OpenSTA, fix the composition, not the test — or record a
  measured fixture/model limit in `docs/results.md`.
- `docs/path-delays.md`: record that multi-source port coverage is
  now exercised (update the honest caveat left by
  `output-port-delays`). `docs/results.md`: dated entry with
  fixture change, scenarios, winner outcome, fail-proof note.
- Version bump (patch) in the same commit as the functional
  change.

## Files

- `tests/designs/sta_exceptions/exceptions.def`
- `tests/designs/sta_exceptions/*.sdc`
- `tests/designs/sta_exceptions/generate_ref.sh`
- `tests/designs/sta_exceptions/exceptions.reference.json`
- `crates/grus-core/src/sta/analysis.rs`
- `docs/path-delays.md`
- `docs/results.md`
- `Cargo.toml`
- `Cargo.lock`
