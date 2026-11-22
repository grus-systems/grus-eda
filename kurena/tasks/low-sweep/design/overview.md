# low-sweep — overview

Sweep ticket: land the accumulated `low` review findings from the merged
perf and activity tickets. Mechanical code/doc fixes only — **no behavior
changes**. Each finding below names the file/anchor it lives at; the
disposition column is the per-item "fixed / skipped with reason" record
the acceptance checklist asks for.

## Rules

- No behavior changes: every committed golden stays byte-identical
  (`git status --short tests/designs` empty at the end); all verify legs
  green: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --release -p grus-core sta`.
- Code comments explain *why*, not *what* (AGENTS §4). New comments name
  the invariant, not the diff.
- One commit, `refactor:` scope; `[workspace.package] version` bumped
  patch in the same commit (0.3.7 → 0.3.8).

## Item dispositions

| # | finding (as recorded on the card) | where | disposition |
|---|---|---|---|
| D1 | `docs/results.md` perf-profile entry claims "45 cells × 5 reps = 225 runs" — `measure.sh` runs 7 base cells × 5 designs + 2 gated `sta.spef_rcx` (gcd, picorv32) + 1 gated `power.vcd` (gcd) = **38 cells / 190 runs** | `docs/results.md` (~L5777) | fix |
| D2 | perf-par-slew entry "full 45-cell matrix" — same miscount | `docs/results.md` (~L5877) | fix |
| D3 | "full 45-cell matrix" wording in design docs | `perf-slewonly-power/design/measurement.md` L14; `perf-legal-passes/design/measurement.md` L14 | fix — the legal-passes copy is the identical stale sentence; swept in under the same item |
| D4 | ~10 ms-granularity `gcd`/≤50 ms speedup figures not marked as timer-floor noise | `docs/results.md` perf-par-slew wall table (gcd `×1.25` rows), perf-slewonly-power gcd `×4.0` row, perf-parsers phase table + DEF tail | fix — see `docs-fixes.md` R2–R4. Wall cells (the `/usr/bin/time` 10 ms quantum) get the `~noise` mark; the perf-parsers ≤50 ms phase rows are `Instant`-timed with non-overlapping rep ranges, so they get a precision note, not a wrong "noise" label |
| D5 | perf-dmp-hotloop entry gaps: phase table missing (min–max) ranges per `measurement.md` skeleton; `sta.prepare.clock.walk` omission unexplained; achieved-vs-ceiling note is prose only | `docs/results.md` (~L6247) | fix — ranges recomputed from the recorded logs and embedded verbatim in `docs-fixes.md` R6 |
| D6 | `phases.md` attribution drift: `place.legal.abacus` lists `verify` (timed under `place.legal.verify`); `sta.partition.compose` lists merged pulse eval (lands under `sta.eval.endpoints.pulse`) | `kurena/tasks/perf-profile/design/phases.md` L38, L85 | fix |
| D7 | perf-parsers doc drift: `unit_scale` still std `parse::<f32>()`; `split_ascii_whitespace` note overclaims the merged-token failure mode; `steps.md` stale `u32 NAME_MAP` vs the `*N` digit-slice key | `docs/results.md` (~L6093–6099), `perf-parsers/design/spef.md` S3/S4, `perf-parsers/design/steps.md` L35 | fix |
| D8 | AP3 leftovers: `docs/` claims `!state_var` support — code matches exact `state_var_n` only; census reproduce script has literal `$PDK_ROOT` inside a quoted-heredoc Python string | `docs/results.md` (~L5289), `ap3-sequential-activity/design/census.md` L54–72 | fix — doc corrected and exact-match justified in the same sentence |
| C1 | `assert_pin_slew_parity` re-implements the `uncovered_borrow → eval_state_full`/`slew_eval` dispatch | `sta/analysis.rs` ~L2298 & ~L10004 | fix — extract a shared `slew_eval_state` helper |
| C2 | `memo_table`: `borrow_mut()` held across the `table(c)` call — re-entrancy-fragile | `sta/dmp.rs` L494–503 | fix — split the borrow; also makes a would-be re-entrant `table` panic-free, not just commented |
| C3 | `legalize.rs` pass-2/3: `moved` tuples pass literal `0` for `new_y` (dead under `move_y=false`); `moved_buf` built for `pin_count <= 1` nets `update_net_extrema` skips; pass-1 `cell_w_dbu[c2] != w1` recheck structurally unreachable | `placement/legalize.rs` ~L475, ~L887, ~L916, ~L1445, ~L1476, ~L1590 | fix — comment the contract, gate the three accept loops, `debug_assert` the width invariant |
| C4 | AP2 evaluator: op budget resets per `sensitivity` call (bound is per-call, not global) | `bool_expr.rs` | fix — the `let mut ops` site already carries the comment (L338–340, per `evaluator.md` §Bounds); add one doc line on `sensitivity` making the per-call bound explicit at the API |
| C5 | `LibSeq` direct parser unit tests (bare `Var`, `state_var_n`, expression rejection) + group-valued `next_state` edge test | `readers/liberty.rs` | fix — four new tests, `libseq-tests.md` |
| — | version bump | `Cargo.toml`, `Cargo.lock` | 0.3.7 → 0.3.8, same commit |

Every finding is accounted for above; nothing is skipped. (If
`/tmp/perf-dmp-{before,after}.log` are gone at implement time, R6's table
is still complete — the numbers are committed in `docs-fixes.md`.)

## Files

- `docs/results.md`
- `kurena/tasks/perf-profile/design/phases.md`
- `kurena/tasks/perf-slewonly-power/design/measurement.md`
- `kurena/tasks/perf-legal-passes/design/measurement.md`
- `kurena/tasks/perf-parsers/design/steps.md`
- `kurena/tasks/perf-parsers/design/spef.md`
- `kurena/tasks/ap3-sequential-activity/design/census.md`
- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/dmp.rs`
- `crates/grus-core/src/placement/legalize.rs`
- `crates/grus-core/src/readers/liberty.rs`
- `crates/grus-core/src/bool_expr.rs`
- `Cargo.toml`
- `Cargo.lock`
