# Goal

`power` pays for a full STA run it does not need:
`sta_pin_slews` calls `run_sta_detail` (eval walk +
endpoints + checks) only to read `slew_max` per pin
(`crates/grus-core/src/sta/analysis.rs:2281`). Plan rank 2
(`kurena/tasks/perf-profile/design/plan.md`): after
perf-par-slew, `sta.eval.walk` (~830 ms jpeg) plus endpoint
collection is the dominant leftover inside `power.slews`
(jpeg `power` 2.17 s post-v0.3.3, `power.slews` envelope
1824 ms — slew-only should bring jpeg `power` toward
~1.3 s).

## Work

- Add a slew-only path: the minimal pipeline
  `sta_pin_slews` actually needs (design/SDC prep, clock
  seed, `propagate_slew`, any partition/seq fixup it
  feeds) without `eval_sta`'s arrival/required walk or
  endpoint collection. Keep it inside `analysis.rs`
  reusing the shared prep — no duplicated seed logic.
- **Parity first**: before `power` switches over, prove
  `PinSlew` from the slew-only path is identical to the
  full-run `PinSlew` on every committed design (assert in
  a test that runs both — at minimum gcd/picorv32/ibex or
  the existing power-golden harness; byte-identical
  `Vec<PinSlew>` or field-wise assert). Only then point
  `power`'s call site (`crates/grus-eda/src/main.rs:872`)
  at the new path.
- Keep the phase timer honest: slew-only work still lands
  under `sta.prepare.clock.slew`/`power.slews`; record
  which keys moved.
- All committed power goldens byte-identical
  (`tests/designs/**/ *.power*.json` and the `activity`
  diagnostic lines).

## Measurement

Re-run `kurena/tasks/perf-profile/measure.sh` `power`
cells (placement RC + zero-wire; `--vcd` gcd too if the
load is shared), REPS=5 same machine; `docs/results.md`
entry: before/after (before = v0.3.3 numbers), achieved
speedup vs plan's ~×1.3–1.4 residual estimate on jpeg,
what remains (slew pass itself, `power.cells`, activity
resolve).

## Files

- `crates/grus-core/src/sta/analysis.rs` (slew-only entry
  or `run_sta_detail` split), `crates/grus-core/src/sta/mod.rs`
- `crates/grus-eda/src/main.rs` (power call site)
- `crates/grus-core/src/power.rs` only if the signature
  forces it — prefer not
- tests proving parity + `docs/results.md`,
  `docs/roadmap.md`, `kurena/tasks/perf-slewonly-power/`
- `Cargo.toml` (patch bump)
