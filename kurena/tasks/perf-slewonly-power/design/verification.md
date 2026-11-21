# perf-slewonly-power — verification

The gate the plan named: slew-only `PinSlew` must equal full-run
`PinSlew` on committed designs — proven by a test that runs both paths,
not by the construction argument in `semantics.md` alone. E0-review
discipline: prove the test can fail; claim coverage only for what an
input exercises.

## 1. The parity test (the acceptance gate)

New `#[cfg(test)]` helper + tests inside `analysis.rs`'s test module
(same file, same harness style as `compare_design_files`):

```rust
fn assert_pin_slew_parity(design_name: &str, sdc_file: &str, rc_mode: Rc)
```

which loads the design exactly as `compare_design_files` does (placed
DEF + SDC + PDK libs, `Rc::{None, Placement, Spef, RoutedSpef}`), then:

```rust
let (_r, full_state, _p) =
    run_sta_detail(&design, &lib, &sdc, parasitics.as_ref(), false).expect("full STA");
let fast = sta_pin_slews(&design, &lib, &sdc, parasitics.as_ref()).expect("slew-only");
assert_eq!(full_state.len(), fast.len());
for (i, (s, p)) in full_state.iter().zip(&fast).enumerate() {
    assert_eq!(s.slew_max[RISE], p.rise_ns, "pin {i} rise");
    assert_eq!(s.slew_max[FALL], p.fall_ns, "pin {i} fall");
}
```

Value equality (`==`), not `to_bits` — the contract is power-lookup
input equality; `NEG_INFINITY` compares equal. (If a `-0.0`/`+0.0`
flake ever appears it would itself be evidence of a leg-order
difference worth investigating, not masking.)

Matrix — deliberately covers each semantic divergence class, not just
big designs:

| case | what it exercises |
|---|---|
| gcd × `Rc::{None, Placement, RoutedSpef}` | smallest; SPEF pi-model path; `power` uses `None`/`Placement` |
| picorv32 × `Rc::{None, Placement}` | medium; per goal's minimum |
| ibex × `Rc::{None, Placement}` | clock-gate latch → `latch_borrow` keys all clock-covered → exercises `slew_eval` on a cyclic clock net |
| jpeg `Rc::Placement` | the measured design; largest level structure |
| aes `Rc::Placement` | fourth committed design family |
| `gcd.propagated.sdc` (or `picorv32.propagated.sdc`) `Rc::None` | `any_propagated` → slew path *keeps* the clock block; propagated-consumer unified-slew copy must match |
| `tests/designs/microbench/{mb,mbw}` `Rc::None` | tiny; same harness the `mb.power.json` / `mbw.openroad.txt` tests read |
| `tests/designs/sta_exceptions/exceptions.def` + one SDC with `set_false_path`/`set_multicycle_path` (e.g. `fp_allregs.sdc`) | partitioned-dispatch slew invariance — proves the non-fallback path need not run `run_sta_partitioned` |
| `tests/designs/sta_edge/latch_borrow/latch_borrow.def` + its SDC | **fallback predicate**: a `latch_borrow` edge off the clock net must route through `eval_state_full`; assert parity holds (and that the predicate fired — see §3) |

If `jpeg`/`aes` push the release `sta` test suite over a comfortable
wall, the acceptance minimum is `gcd/picorv32/ibex` × both power modes —
but record which of the five ran.

## 2. Goldens and command outputs unchanged

- `cargo test --workspace` — includes `power.rs`'s OpenSTA comparisons
  (`test_saif_power_mb_matches_opensta`, `test_when_power_mbw_matches_opensta`,
  the `mb.power.json` harness) — all green with **zero** tolerance
  edits.
- `cargo test --release -p grus-core sta` — the strict endpoint
  goldens exercise `run_sta_detail` end-to-end; the refactor
  (`prepare_sta` flag, `eval_seed_state`, `eval_state_full`) must not
  change a single endpoint field.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- `git status` clean under `tests/designs/**` — no golden moves.
- Spot `cmp`: `power` stdout is not byte-stable (the `{:.2} ms`
  suffix), so byte-identity is asserted on the slew vector itself by §1
  and on the stable power fields by the workspace tests; the
  `activity`/`power:` diagnostic lines (`internal_power_without_slew`
  etc.) can be `diff`ed by hand on one run and recorded.

## 3. Fail-proof runs (required, recorded)

Two deliberate breaks, each restored green:

1. **Leg mutation**: drop the `!graph.clock_net[out_net]` guard in
   `slew_eval` (or drop the TimingType filter) → §1 tests must go red
   on designs with clock-net arcs (all of them). Record which failed.
2. **Fallback-predicate mutation**: force `uncovered_borrow = false`
   → the `latch_borrow` fixture test must go red (its data-latch D→Q
   leg then merges slew eval drops). Record; restore.

Also record *which fixture actually fires the fallback* — if the
committed `latch_borrow` DEF's borrow edge turns out to sit on a clock
net, the predicate is unexercised and the doc must say so honestly
(per AGENTS §7.4); then either pick a fixture that does
(`sta_edge/clock_gate` is *not* one — its Q is clock-netted) or note
the fallback is defensive-but-required.

## 4. Determinism

`slew_eval` reuses the `eval_sta` collect-apply discipline — same
determinism argument as perf-par-slew (disjoint writes, ordered apply,
no map-iteration dependence). Evidence: run the parity test once under
`RAYON_NUM_THREADS=1` and once normally (record both green); a
`t1`/`t10` `cmp` sweep like perf-par-slew's is optional — record if
skipped.

## 5. What the tests do NOT prove

- `slew_eval` equivalence on designs mixing `latch_borrow` off clock
  nets — by construction those take the fallback; the parity test
  asserts the fallback, not the replay, there.
- New uncommitted SDC features (e.g. a `set_clock_transition` on an
  ideal row): covered only insofar as an `sta_clocks`/`sta_edge`
  fixture exercises them; if the implementer wants belt-and-suspenders,
  add one `t2_*`-style fixture SDC to the matrix and record it.
