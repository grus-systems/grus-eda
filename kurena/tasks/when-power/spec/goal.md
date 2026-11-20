# Goal

N7 remaining item: evaluate `when`-qualified `internal_power`
blocks instead of skipping them. `power.rs:206` counts each
`when`-carrying block in `conditional_power_blocks` and drops
it; the sky130 `ebu*`/`einv*` cells (instantiated in `aes`) carry
these. The doc note says value-correlated activity is needed —
with VCD per-net timelines (and SAIF T0/T1 marginals) at least
the marginal state probability is available; whether OpenSTA
needs joint correlation is a probe question.

## Work

1. **Measure OpenSTA's `when` semantics** inside the LibreLane
   container, recorded in `docs/results.md`:
   - On a cell with a `when`-qualified `internal_power` block
     (pick an ebufn/einvn instance from `aes`, or a minimal DEF
     fixture if aes's power path is too heavy), does the block
     apply for the *fraction of time its condition holds*
     (marginal P(cond) × trigger rate), or does `report_power`
     move differently?
   - Does `leakage_power` `when` state evaluation matter, or
     does OpenSTA just use `cell_leakage_power` regardless of
     pin states? (Our impl already sums the declared average —
     check whether a `when`-state-aware sum moves the number.)
   - VCD vs SAIF: does the SAIF T0/T1 marginal give the same
     conditional share as the VCD joint waveform? If they
     diverge, which does OpenSTA appear to use?
   Record the exact SDC/activity used and every intermediate
   number, plus any `when` forms beyond a single pin name
   (negation, conjunction) the lib actually instantiates.

2. **Implement** per the measured rule: evaluate `when` blocks
   with the measured probability model; only skip (counted)
   `when` forms the measured model cannot express. Keep the
   unconditional path unchanged.

3. **Fixture + golden**: extend `tests/designs/microbench/` (or
   a small ebufn-bearing DEF) with a `when`-carrying cell +
   activity that exercises both branches of the condition;
   golden emitted by the committed script inside the container,
   STRICT comparison.

4. **Docs**: `docs/results.md` dated entry (measured rule,
   marginal-vs-joint verdict, tolerance result);
   `docs/roadmap.md` N7 updated; version bump (patch).

## Files

- `crates/grus-core/src/power.rs`
- `crates/grus-core/src/lib_db.rs`
- `crates/grus-core/src/readers/vcd.rs`
- `crates/grus-core/src/readers/saif.rs`
- `tests/designs/microbench/`
- `docs/results.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
