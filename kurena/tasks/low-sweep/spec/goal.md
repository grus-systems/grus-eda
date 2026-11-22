# Goal

Sweep the accumulated review `low` findings from the
merged perf and activity tickets — mechanical code/doc
fixes only, no behavior changes. Each item lists the
finding as recorded on its kurena card; fix what's cheap,
and for anything skipped write a one-line reason in the
design notes.

## Items

**Docs/results fixes**

- `docs/results.md` perf entries: mark the ~10 ms-
  granularity `gcd`/≤50 ms cells' speedup figures as
  timer-floor noise (perf-par-slew/perf-parsers tables);
  fix "45 cells × 5 reps" claims — actual executed matrix
  is 38 cells / 190 runs (perf-profile entry +
  `perf-slewonly-power` design doc "full 45-cell matrix"
  wording).
- perf-dmp-hotloop entry: add the (min–max) ranges to
  the phase table per `measurement.md` skeleton; note
  `sta.prepare.clock.walk` omission (zero calls on ideal-
  clock cells); thicken the achieved-vs-ceiling note
  (memo hit-rate × per-call cost math, not just prose).
- perf-profile `phases.md` attribution drift vs code:
  `place.legal.abacus` row shouldn't list `verify`
  (code times it under `place.legal.verify`);
  `sta.partition.compose` doesn't include merged pulse
  eval (it lands under `sta.eval.endpoints.pulse`).
- `perf-parsers` docs: `unit_scale` still uses std
  `parse::<f32>()` — narrow the fast-float2 claim;
  `split_ascii_whitespace` note overclaims the
  non-ASCII-whitespace failure mode (merged-token case);
  `steps.md` stale `u32 NAME_MAP` vs the `*N` digit-slice
  key.
- AP3 leftovers: `docs/` claim of `!state_var` support —
  code matches exact `state_var_n` only (fix doc, or
  record why exact-match is right); sky130 census script
  has literal `$PDK_ROOT` inside a Python heredoc that
  won't expand — fix quoting or pin the env-var note.

**Code nits**

- `assert_pin_slew_parity` re-implements `sta_pin_slews`
  dispatch (`uncovered_borrow` → `eval_state_full` /
  `slew_eval`) — share the dispatch or comment why the
  duplication is intentional.
- `dmp.rs` `memo_table`: `borrow_mut()` held across the
  `table(c)` call — safe today but re-entrancy-fragile;
  take/drop the Ref around the call or comment the
  invariant.
- `legalize.rs` pass-2/3: `moved` tuples pass literal `0`
  for `new_y` (dead under `move_y=false`) — comment the
  contract or use a clearer sentinel; `moved_buf` built
  for `pin_count<=1` nets that `update_net_extrema` skips
  — gate the loop on it; the pass-1 `cell_w_dbu[c2] !=
  w1` recheck is structurally unreachable — comment or
  debug_assert.
- `LibSeq` direct parser unit tests (bare `Var`,
  `state_var_n`, expression-rejection) + a group-valued
  `next_state` edge test if cheap.
- AP2 evaluator: comment that the op budget resets per
  sensitivity call (worst-case bound is per-call, not
  global).

## Files

- `docs/results.md`, plus the touched design docs under
  `kurena/tasks/*/`
- `crates/grus-core/src/sta/{analysis,dmp}.rs`,
  `crates/grus-core/src/placement/legalize.rs`,
  `crates/grus-core/src/readers/liberty.rs` (LibSeq tests)
- `Cargo.toml` (patch bump)
