# Goal

N7 remaining item: SAIF input for power reporting. `power.rs`
estimates leakage/switching/internal power from VCD switching
activity (`readers/vcd.rs` → `VcdActivity`), formula-validated vs
OpenSTA `report_power`. There is no SAIF reader, so files from
flows that emit `.saif` cannot drive the estimator.

## Work

1. **Measure OpenSTA's SAIF semantics first** (inside the
   LibreLane container, recorded in `docs/results.md`):
   - how `read_saif` maps SAIF fields (TC/TX/T0/T1, glitch counts,
     duration, timescale) into activity — toggle rate vs duty
     cycle vs both;
   - how OpenSTA's `report_power` output compares when fed the
     same activity window as SAIF vs VCD on the microbench
     (`tests/designs/microbench/mb.*`);
   - how hierarchy scoping resolves (SAIF instance paths vs our
     DEF instance/net names; unmatched scopes counted or ignored).
   Produce a `mb.saif` (from the same activity window — converted
   or hand-authored on the tiny design is fine, but its values
   must come from running OpenSTA/sim, not be assumed equal to
   the VCD counts by fiat).

2. **Implement.** A `readers/saif.rs` reader producing the
   activity structure `estimate_power` already consumes (extend
   it minimally if SAIF needs a field VCD lacks — e.g. separate
   duty/high-time), a `--saif` CLI flag alongside `--vcd`, and
   diagnostics for SAIF-scope names that match no design net
   (counted, never silently dropped). Power math itself should be
   unchanged — the equivalence check is that SAIF-driven numbers
   land where measured OpenSTA-SAIF numbers do.

3. **Fixture + golden.** Extend `tests/designs/microbench/` with
   `mb.saif` + the OpenSTA reference (a `power_saif` entry in the
   existing golden JSON or a sibling file — match the fixture's
   conventions) emitted by the committed Tcl inside the container.
   STRICT comparison test.

4. **Docs**: `docs/results.md` dated entry (measured SAIF field
   semantics, SAIF-vs-VCD agreement, residual divergence);
   `docs/roadmap.md` N7 remaining list updated (SAIF struck,
   `when`-blocks + statistical-activity items stay open);
   `README.md`/`main.rs` help if the CLI surface changes. Version
   bump (patch).

## Files

- `crates/grus-core/src/readers/saif.rs` (new)
- `crates/grus-core/src/readers/mod.rs`
- `crates/grus-core/src/power.rs`
- `crates/grus-eda/src/main.rs`
- `tests/designs/microbench/`
- `docs/results.md`
- `docs/roadmap.md`
- `README.md`
- `Cargo.toml`
- `Cargo.lock`
