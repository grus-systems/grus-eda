# Goal

Close `docs/path-delays.md` section 5 item 4: the propagated-clock
launch anchor for input-port delay sources is unverified. `md_port`
(`set_max_delay -from [get_ports d_in]`) and `md_minport` cover the
ideal-clock anchor only — `input_clock_latency_ns`, the same I/O
clock anchor the port's own input delay uses. Under
`set_propagated_clock` two candidates differ: the SDC source
latency, or the propagated arrival at the `clk` port pin. The doc
says this is unverified because no golden exercises it.

## Work

1. **Probe OpenSTA on the committed fixture.** Add `pmp_*`
   scenarios to `tests/designs/sta_exceptions/` (shared preamble +
   `set_propagated_clock`, same shape as `pmd_*`/`pm2_*`/`pmo_*`):

   - `pmp_port` — `set_propagated_clock` +
     `set_max_delay N -from [get_ports d_in] -to [get_cells u_ff0]`.
   - `pmp_minport` — propagated + `set_min_delay N` on the same
     port pair.
   - `pmp_port2` — propagated + a different `set_max_delay` value
     or a `d_in` input-delay variant, if needed to make the two
     candidate anchors separable in the reported numbers (the
     design doc should state the predicted value under each
     hypothesis so the golden result discriminates them).
   - If useful for the same question: `pmp_alls` — propagated +
     `-from [all_inputs]` (note `sel` is also an input port —
     predict whether it covers `u_ff1/D` too).

   Add them to `SCENARIOS` in `generate_ref.sh` and regenerate
   `exceptions.reference.json` by running the committed script
   inside `ghcr.io/librelane/librelane:3.0.16` (docker is
   available). Every value comes from that run.

2. **Reconcile with our implementation.** Our port launch anchor
   is `input_clock_latency_ns` (`sdc.rs`), the I/O clock anchor
   used by the port's own input delay. If the propagated golden
   shows OpenSTA uses a different anchor (e.g. propagated clock-
   port arrival), fix the anchor in
   `crates/grus-core/src/sta/analysis.rs` — not the test — or
   record a measured model limit in `docs/results.md`.

3. **Docs**: `docs/path-delays.md` — resolve the "unverified"
   note for the propagated port anchor (update the resolved
   section and item 4). `docs/results.md` — dated entry with the
   scenarios, the two hypotheses, and which one the golden
   confirmed. Version bump (patch) in the same commit.

## Files

- `tests/designs/sta_exceptions/*.sdc`
- `tests/designs/sta_exceptions/generate_ref.sh`
- `tests/designs/sta_exceptions/exceptions.reference.json`
- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/sdc.rs`
- `docs/path-delays.md`
- `docs/results.md`
- `Cargo.toml`
- `Cargo.lock`
