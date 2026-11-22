# e5-cts-e2e — Tests

All in `cts/mod.rs`'s `#[cfg(test)] mod tests`, sharing the
`cts_sta_run` runner and the `CtsTol`/`CtsRefJson` glue of
`harness.md`. Two gates, one determinism test, one decomposition print
per design.

## Gate tests (criteria 1–3 + honesty print)

```rust
#[test]
fn test_gcd_cts_e2e() { cts_e2e("gcd", CtsTol::GCD_CTS); }

#[test]
fn test_picorv32_cts_e2e() { cts_e2e("picorv32", CtsTol::PICORV32_CTS); }
```

`cts_e2e` asserts, in order:

1. `report.endpoints` == golden `endpoints` set (count + names, both
   directions).
2. Per-endpoint 8 fields within `tol` (criterion 3).
3. WNS/TNS within `tol`.
4. Clock-pin set == original sinks == golden `clock_pins`; every pin
   reached.
5. Skew: `|our_spread − (latency_max − latency_min)| <=
   max(0.025, 0.25·ref_spread)` (criterion 1).
6. Insertion: `|our_lat_max − latency_max_ns| <= 0.15·ref`
   (criterion 2).
7. Per-pin clock sanity: `|our_lat(p) − ref arr_rise_max_ns(p)| <=
   max(0.025, 0.15·ref_pin)` (sanity, not criterion).
8. Honesty print: `CtsReport` vs `CtsRefJson` census table
   (criterion 4 — print only).

`CtsTol::GCD_CTS` / `PICORV32_CTS` per `harness.md` §5. On first run,
if a criterion bound fails, the failure is the measurement — then
decompose (`decomposition.md`), name the cause, hold the floor in a
documented `PICORV32_CTS_*`/`GCD_CTS_*` constant. The constants live in
the tests module with the same cause-naming doc comments the
`Tol::*_GUIDE` constants carry in `analysis.rs`.

## Determinism test

```rust
#[test]
fn test_gcd_cts_ours_def_deterministic() { ... }
#[test]
fn test_picorv32_cts_def_deterministic() { ... }
```

- Run `run_cts` → `emit_def_string` twice in one process; assert
  `emit1 == emit2` (both designs — the constructor's sorted partition
  and `BTreeMap` census make this an expectation, and
  `write_def_full` embeds no timestamp).
- gcd additionally: `emit1 == std::fs::read_to_string(
  design_dir/gcd.cts.ours.def)` — the committed artifact *is* what
  `grus-eda cts` produces; a tree change without a regenerated DEF
  goes red here. picorv32's ours-def is gitignored, so the file arm is
  gcd-only; the emit-pair arm still covers its determinism.

## Decomposition prints (recorded, not gated)

```rust
#[test]
fn test_gcd_cts_decomposition() { cts_decomposition("gcd"); }
#[test]
fn test_picorv32_cts_decomposition() { cts_decomposition("picorv32"); }
```

Produces the per-axis worst-5 tables of `decomposition.md`:
`engine` (our STA on `<d>.cts.def` vs golden), `tree-shape` (our STA
on ours-def vs our STA on ref-def), `total` (our STA on ours-def vs
golden), and the `reference-setup` clock-pin rows. Requires
`<d>.cts.def` via `require()` — loud when the gitignored
`picorv32.cts.def` has not been regenerated (`generate_cts_ref.sh
picorv32`; the worktree seed supplies it when the main checkout has
it).

## Fail-proof (AGENTS.md §7.7)

After the gates pass, mutate and confirm red, then restore:

- **Mutation**: point the e2e runner's input at `<d>.placed.def`
  *without* calling `run_cts` (skip step 3 of the runner — a plausible
  "forgot the tree" mixup). On gcd the gate must go red: insertion
  delay collapses to ~0 vs 0.3145 ns (criterion 2) and endpoint
  arrival/required fields shift ~0.3 ns (criterion 3). Restore.
- If the mutation somehow stays green on picorv32's wider floors that
  is a finding about the bound's discrimination power — recorded in
  `docs/results.md`, not patched over.
- Second recorded mutation (cheap, recommended): swap `latency_max`
  for `latency_min` in the insertion bound → the gcd gate goes red on
  a ~1 ps miss of criterion 2's complement, proving the clock-pin
  surface is actually read.

## Contingency — polarity-tie escalation

`EndpointReport` fields already equal the analysis harness's
`pick_worst_polarity` default; the only uncovered case is a ≤2 ps
rise/fall slack tie where OpenSTA's reported row carries the other
polarity's tuple. If a gate failure is *diagnosed* as that (slack tie
on our side, fields off by a transition's worth):

1. `sta/analysis.rs`: `#[cfg(test)] pub(crate) fn run_sta_detail` and
   `pub(crate)` on `EndpointPolarities`, `Worst`,
   `pick_worst_polarity` — the `run_sta_with_state` precedent.
2. Local `resolve_tied_polarity` in `cts::tests` (the analysis
   helper's body, ~20 lines).
3. Record the diagnosis and the file addition in the commit message —
   it is outside the goal's Files but inside "whatever scoring glue it
   needs".

Do not add the escalation preemptively; an unused `pub(crate)`
widening is exactly the dead surface the codebase rules reject.

## Out-of-band verification (recorded, not cargo tests)

- `tests/designs/generate_cts_ours.sh gcd` rerun → `git diff` on
  `gcd.cts.ours.def` + `gcd.cts.ours.report.json` empty.
- `tests/designs/generate_cts_ref.sh picorv32` rerun (to produce the
  gitignored `picorv32.cts.def`) → `git diff` on the committed
  `picorv32.cts.reference.json` + `picorv32.cts.sta.reference.json`
  empty; any drift is investigated, not committed blind (E0 lesson 1).
- `cargo test --workspace`, `cargo clippy --workspace --all-targets --
  -D warnings`; `cargo test --release -p grus-core sta` when the
  sta-regression runner is selected.
