# steps — implementation order and gates

The r2 ticket landed (v0.2.15): `bool_expr.rs` evaluator, `pin_signals`
on `vcd.rs`/`saif.rs`, `pin_by_lib_name`, Kahn propagation, the mba test
extension, the results entry. r3 is one follow-up commit (~200 lines
incl. tests + docs) closing the review items. The `cp.def` probe was run
during planning (verbatim output recorded below and destined for
`docs/results.md`) — implementation needs no further container session.

## 1. `bool_expr.rs` — `const_eval` + the ops-reset comment

- `pub fn const_eval(e: &BoolExpr, vals: &[Option<bool>]) -> Option<bool>`
  — Kleene absorbing-element eval per `evaluator.md` (And → `Some(false)`
  on any `Some(false)`; Or → `Some(true)` on any `Some(true)`; known
  value iff all inputs known). Recursion bounded by `MAX_NODES` at parse;
  no own counter.
- Comment at `prob`'s `let mut ops = MAX_OPS;`: the budget resets per
  call, `sensitivity` calls `prob` internally → per-gate worst case is
  `(1 + n_inputs) × MAX_OPS`; the bound is per-call work, intended.
- Unit tests: the `evaluator.md` `const_eval` table, including
  `(A&B)|C` at `A=Some(false), B,C=None` → `None` (absorption is
  per-operator).
- Gate: `cargo test -p grus-core bool_expr` green; clippy-clean.

## 2. `activity.rs` — `comb_funcs` table + 3-valued marking

- `CombFunc { expr, input_nets }` + `comb_function_table` per api.md §2
  (driver on `!is_sequential && !is_latch` cell; `function: None` →
  `None` uncounted; parse/var-miss → `None` +
  `unresolvable_functions += 1`, counted at build for any origin).
- `constant_cone(design, &comb_funcs) -> Vec<bool>` — the sweep
  per api.md §5 (`vals: Vec<Option<bool>>`, `const_eval`, fixed point).
  The literal `"0"/"1"` check disappears into `Const` exprs; `lib` is no
  longer needed inside (the table carries parsed exprs).
- Seed-site comment on the `Constant` arm: the stored marginal is 0.0
  for both polarities and that is the **measured internal value**
  (`cp.def` 2026-11-21 — `xor(HI,a)` propagates `a`'s `0.55`, not `¬a`'s
  `0.45`); only the marking logic uses true polarity.
- `propagate_combinational(&comb_funcs, ...)`: candidates = `Default` ∧
  table-`Some`; `pending: Vec<bool>` replaces the `Option::take` dequeue
  invariant (`debug_assert!(pending[m])` — api.md §2). Eval unchanged.
- Gate: `cargo test -p grus-core` green — mba test unchanged green
  (`constant 4` identical; no mixed gate exists there), mb/mbw unchanged.

## 3. `tests/designs/microbench/cp.def` + `cp_probe.tcl`

- `cp.def`: the probed file verbatim (conb + 8 gates + port `a` + port
  `y` on `n_xorh`; 11 nets incl. `n_bufh`).
- `cp_probe.tcl`: lib/lef/def reads, `set_power_activity -input_ports
  [get_ports a] -density 0.003 -duty 0.55`, the `get_property … activity`
  dump, then the precedence section (`-pins` and `-global` on
  `u_orh/X`/`u_andh/X`/`u_orl/X`). Header comment records the docker
  invocation (`generate_activity_ref.sh`'s container stanza).

## 4. `power.rs` — `test_constant_cone_cp_matches_opensta`

- Loads `cp.def`, `Sdc::default()`, `VcdActivity::default()`, overrides
  `input: {3e6, 0.55}`; asserts the measured per-net map verbatim
  (`golden.md` table) + `origin_counts {constant 6, input 1,
  propagated 4}` + both diagnostics `0`. Comment cites the
  `docs/results.md` entry as the source of every expected number.

## 5. Fail-proof (recorded in the entry)

- `constant_cone` reverted to all-inputs marking → cp test reds on
  `n_orh`/`n_andl` (`propagated` vs expected `constant`); restore, green.
- The r2 proofs stand (propagation off → `s_none origin_counts` red;
  corrupted golden `prob` → strict red; both recorded 2026-11-21).

## 6. Docs + version

- `docs/results.md` — new `2026-11-21 — N7 — AP2 r3: measured constant
  semantics (v0.2.16)` entry:
  - The two probe sessions' verbatim `ACTI` output (table + precedence
    reads) and the three measured rules (api.md §5).
  - `conditional_power_blocks` enumeration: no moves possible —
    `prob_one` on constants unchanged; named residual for `when`
    literals on absorptive-marked nets (no fixture exercises it).
  - **SAIF-vs-VCD asymmetry record**: a SAIF top-scope `NET` entry
    naming an internal net annotates via the emitted driver-pin
    `pin_signals` key while the same flat name in a VCD dead-ends into
    `dead_annotation_entries` — a constructed asymmetry; OpenSTA's
    `read_saif` on such a `NET` entry is unmeasured (named residual).
  - Moved goldens: none — `git diff` empty on every
    `*.reference.json`/`mb.power.json` (enumerated).
  - Fail-proof record; verify-trio output.
- `docs/roadmap.md` N7 line: extend the AP2 clause — `(v0.2.16 —
  constant marking is 3-valued absorptive; the stored constant marginal
  is the measured 0.0 — cp.def probe)`.
- `Cargo.toml` `0.2.15 → 0.2.16`; `cargo check` refreshes `Cargo.lock`.
- `mb.power.json` regen — expected `git diff` empty; `git status` shows
  only the `## Files` list.

## 7. Final verify (all recorded in the entry)

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --release -p grus-core sta`

## Acceptance cross-check

| acceptance item | where |
|---|---|
| evaluator `P(f)`/`d(f)` exact, repeated literals | landed v0.2.15 |
| missing/unparseable functions fall back without panic | `comb_funcs` build counting (api.md §2) |
| combinational level order; seq outputs never produced | landed (Kahn + `!is_sequential && !is_latch` in table build) |
| `Default` input keeps `Default` output (taint) | landed; unchanged |
| `Propagated` nets match golden at strict bound | landed; unchanged — `prob_one` on constants stays the measured 0.0 |
| `EXPECTED_DIVERGENT` verbatim (13 pins) | landed; unchanged |
| fail-proof recorded | step 5 (adds the marking revert) |
| `docs/results.md` entry + roadmap N7 | step 6 (probe verdict, SAIF/VCD record, residual/golden-move enumeration) |
| verify trio + version bump same commit | steps 6–7 |

## Files

- `crates/grus-core/src/bool_expr.rs` — `const_eval`, ops-reset comment, tests
- `crates/grus-core/src/activity.rs` — `comb_funcs`, 3-valued
  `constant_cone`, propagation pass switch to the shared table
- `crates/grus-core/src/power.rs` — `test_constant_cone_cp_matches_opensta`
- `tests/designs/microbench/cp.def`, `cp_probe.tcl` — new
- `docs/results.md`, `docs/roadmap.md`, `Cargo.toml`, `Cargo.lock`
- `kurena/tasks/ap2-combinational-propagation/design/` — this document set
