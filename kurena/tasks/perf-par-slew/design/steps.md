# perf-par-slew — implementation order and gates

One commit: functional change + docs + version bump together (repo rule:
version in the same commit as the functional change). ~80–120 lines of
`analysis.rs`.

## 0. Baseline captures (before editing)

Run `cargo build --release` and save the pre-change `--json` reports per
[`verification.md`](verification.md) §2 — needed for the post-change
`cmp`. Cheap insurance; skip only if the serial comparison will come from
goldens alone (record which).

## 1. `analysis.rs` — parallel `propagate_slew`

Per [`algorithm.md`](algorithm.md):

- Build `rank` (position of each pin in `levelized.level_pins`).
- Hoist the pin body into an `eval_pin(&slew, pin) ->
  Option<(usize, PinTimingState)>` closure — verbatim serial logic: net
  load replace, combinational arc merges, `seq_in` merges, `arc_slew`.
- Level loop: `level.len() < 32` serial, else
  `par_iter().filter_map(..).collect()` then apply in order.
- Per-level seq fixup in `level_pins` order, `lo <= rank[src] < i`, into
  `slew[p]` in place, under the `pin_nets[p]` guard.
- `arc_slew`, `slew_sink`, `merge_slew`, `slew_reached` unchanged; the
  `sta.prepare.clock.slew` scope at the call site unchanged — no
  `timing.rs` edit.

Gate: `cargo test --workspace` + `cargo clippy --workspace --all-targets
-- -D warnings` green.

## 2. Equivalence + determinism runs

Per [`verification.md`](verification.md): post-change `--json` `cmp`
against the step-0 captures; `RAYON_NUM_THREADS` sweep on jpeg and ibex;
`cargo test --release -p grus-core sta` green; `git status` clean under
`tests/designs/**`; `GRUS_PHASE_TIMING=1` run shows
`sta.prepare.clock.slew`.

## 3. Fail-proof + fixup probe

Per [`verification.md`](verification.md) §4: intentional break → record
red tests → restore green; temporary fixup counter → record which designs
exercise it → remove.

## 4. Measurement

`REPS=5 sh kurena/tasks/perf-profile/measure.sh /tmp/perf-par-slew.log`
on the final release binary (or the acceptance-minimum cell subset —
record which). Extract medians and the `sta.prepare.clock.slew` phase
medians.

## 5. Docs + version + commit

- `docs/results.md` entry per [`measurement.md`](measurement.md) skeleton:
  before/after table, phase line, achieved-vs-ceiling, residual named,
  equivalence/determinism/fail-proof evidence, "what was run".
- `docs/roadmap.md` N8: `perf-par-slew` → done with measured number.
- `Cargo.toml` `0.3.2` → `0.3.3`, `Cargo.lock` follows.
- Commit message: `perf:` or `feat:` conventional prefix; name the
  mechanism, the fallback fixup, measured speedups, fail-proof result,
  and the `Co-Authored-By` trailer.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `docs/results.md`
- `docs/roadmap.md`
- `kurena/tasks/perf-par-slew/design/`
- `Cargo.toml`
- `Cargo.lock`
