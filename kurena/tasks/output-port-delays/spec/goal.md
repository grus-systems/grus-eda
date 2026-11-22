# Goal

Implement `set_max_delay`/`set_min_delay` checks whose `-to` selects
an output port — `docs/path-delays.md` section 5 item 2. Today those
endpoints are resolved (`ExceptionEndpointKind::OutputPort`) but
counted in `unsupported_path_delay_endpoints` and skipped, keeping
their ordinary output-delay clock check. The v1 scope note (section
3) defers them; this task lands them.

The measured formulas (section 1, ad hoc `md_toy*` probes):

- max side: `req = N - output_delay_max - setup_uncertainty`
  (edge reference 0; verified at N = 0.5/1.0/2.0)
- min side: `req ~= N` — the exact additive constant is TBD and must
  be measured first.

## Work

1. **Measure the semantics first.** Before touching
   `crates/grus-core/src/sta/analysis.rs`, add `mdo_*`/`pmo_*`
   scenario SDCs to `tests/designs/sta_exceptions/` that probe the
   exact OpenSTA behavior on `-to [get_ports y]`:

   - `set_max_delay N -to [get_ports y]` over N in {0.5, 1.0, 2.0}
     — confirm the max-side req formula on the committed fixture.
   - `set_min_delay N -to [get_ports y]` over N in {0.0, 0.5, 1.0}
     — pin down the min-side additive constant.
   - `-to [all_outputs]` (selector coverage), `-from` subsets
     (`-from [get_cells u_ff0] -to [get_ports y]`, and a two-source
     `-from [get_cells {u_ff0 u_ff2}] -to [get_ports y]` — `y`'s fanin
     cone reaches both sources through `u_mux`/`u_ff1`), and
     propagated-clock variants (`pmo_*`, same preamble shape as
     `pmd_*`/`pm2_*`).
   - Precedence probes: a `set_false_path -to y` overlapping a delay
     rule, and delay + multicycle on the same port pair — the
     measured precedence (false_path > delay > multicycle) must hold
     on the port side too.
   - If an output port has multiple launch candidates, record which
     candidate OpenSTA reports (worst covered candidate, same rule
     as FF endpoints — verify, do not assume).

   Add every probe file to `SCENARIOS` in `generate_ref.sh` and
   regenerate `exceptions.reference.json` by running the committed
   script inside `ghcr.io/librelane/librelane:3.0.16` (docker is
   available). Every reference value comes from that run — never
   hand-write or round.

2. **Implement** the output-port delay check in
   `crates/grus-core/src/sta/analysis.rs` on the existing
   signature-class machinery: a covered `OutputPort` endpoint gets
   the delay check on the covered side (per-transition worst
   candidate, same composition order as FF endpoints) instead of a
   `unsupported_path_delay_endpoints` count. Uncovered sides keep the
   ordinary output-delay check. Endpoints that remain out of model
   (clock gates, latch-D, async checks) stay counted.

3. **Verify strict**: `cargo test -p grus-core
   test_sta_exceptions_match_opensta` passes on all scenarios with
   no tolerance change. If a real divergence shows up, fix the
   composition, not the test — or record a measured fixture/model
   limit in `docs/results.md`.

4. **Docs**: `docs/path-delays.md` — move the output-port formulas
   from "out of v1 scope" to the supported semantics, update the
   scope list in section 3, mark section 5 item 2 resolved.
   `docs/results.md` — entry with date, scenarios added, measured
   formulas (especially the min-side constant), pass/divergence
   status. `docs/timing-exceptions.md` / `docs/roadmap.md` — sync
   the "remaining" bullet for path delays. Bump the workspace
   version (patch) in the same commit as the functional change.

Files touched: `crates/grus-core/src/sta/analysis.rs`,
`tests/designs/sta_exceptions/mdo_*.sdc` + `pmo_*.sdc` (new),
`tests/designs/sta_exceptions/generate_ref.sh` (SCENARIOS),
`tests/designs/sta_exceptions/exceptions.reference.json`
(regenerated), `docs/path-delays.md`, `docs/results.md`,
`docs/timing-exceptions.md`, `docs/roadmap.md`, `Cargo.toml`,
`Cargo.lock`.
