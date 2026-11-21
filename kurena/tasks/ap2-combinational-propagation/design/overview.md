# AP2 — combinational propagation: overview

Second ticket of the AP1–AP4 sequence
(`kurena/tasks/activity-propagation/design/api.md` §Plan; measured semantics:
`docs/results.md` 2026-11-21 N7 probe, esp. P2/P4; golden:
`tests/designs/microbench/mba.activity.reference.json`, 12 sessions × 43 pins).

AP2 replaces the AP1 `Default` interim fill on nets driven by combinational
cell outputs with a propagated `{prob_one, density}` — origin `Propagated` —
per the measured **M1 local-marginal** model.

## What landed (v0.2.15)

1. **Gate-function evaluator** (`crates/grus-core/src/bool_expr.rs`).
   Parses arbitrary `LibPin.function` strings and computes, under input
   independence: `P(f)` exactly via Shannon cofactors (repeated literals and
   any expression shape — the per-operator product rules in api.md §3 are the
   same math on the common forms), and `d(f) = Σ_i d_i · P(∂f/∂x_i)` with
   `∂f/∂x_i = f(x_i=0) ⊕ f(x_i=1)` (output sensitization — measured P2).
   Total and bounded (`MAX_VARS`/`MAX_DEPTH`/`MAX_NODES`/`MAX_OPS`); a
   missing, constant, unparseable, or over-bound function returns `None`
   and the net keeps `Default`. See `evaluator.md`.
2. **Levelized fill** inside `activity.rs` — an own Kahn order;
   `sta::analysis::topological_levels` stays private. Sequential/latch
   cells are boundaries: outputs never produced (AP3), inputs ordinary.
3. **Input gating (taint)** — propagation consumes only resolvable inputs
   (`prob_one: Some`, origin ∉ {`Default`, `Unknown`}); the
   sequential-feedback cones (`u_i0/Y`, `u_x1/X`) stay `Default` until AP3.
4. **Pin-namespace annotation** — `VcdActivity`/`SaifActivity` emit
   `pin_signals` (`inst/leaf` keys); a dump entry annotates a net iff it
   names the net's driver pin; flat internal net names are dead entries.
5. **Golden coverage** — `mba` activity test extended;
   `EXPECTED_DIVERGENT` = the 13-pin AP3 set.

## r3 revision — measured constant semantics (review delta)

The review flagged that `constant_cone` seeds `prob_one = 0.0` — the
recorded accessor display artifact — and that `propagate_combinational`
consumes it as a real marginal. A scratch probe (`cp.def`, and2/or2/xor2
with one `conb` input and one annotated input — committed fixture) measured
the rule directly. Verdict (full reads in `api.md` §5 and `docs/results.md`):

- **The stored marginal is really 0.0 for both polarities** — `xor(HI, a)`
  reads `3e6/0.55` (= a, computed with `P(HI)=0`), not `0.45` (= ¬a).
  `prob_one: Some(0.0)` on constant nets is **correct**; propagation and
  `eval_when` keep consuming it. No `conditional_power_blocks` or golden
  moves are possible from this clause — `prob_one` is unchanged.
- **But the `constant` label itself uses true-polarity evaluation** —
  `or(1,X)=1` and `and(0,X)=0` mark outputs `constant` even when other
  inputs toggle (`u_orh/X`, `u_andl/X`, `u_andhh/X` all read `constant`).
  The landed all-inputs-constant `constant_cone` misses absorptive cases.
  Fix: `constant_cone` gains 3-valued function evaluation via
  `bool_expr::const_eval` (Kleene absorbing-element semantics), sharing a
  once-built `comb_funcs` table with the propagation pass.
- **Constant marking is structural**: an absorptive-constant pin stays
  `constant` under `-pins` and `-global` (measured); marking runs before
  the seed pass, so `Constant` beats annotation and global — unchanged
  precedence.
- New fixture `tests/designs/microbench/cp.def` + probe script
  `cp_probe.tcl` committed; a `power.rs` test asserts the measured
  per-net map verbatim.

Expected blast radius: **zero committed-golden movement** — `mba`'s
constant cones are all-constant-input (identical marking either way);
`mb`/`mbw` contain no `conb`; no other test asserts resolved activities.
The absorptive rule matters on real designs (`gcd` has `conb` feeding
gates — no asserting test exists there).

## What does not land

- Sequential rules: Q=D fill (`u_df/Q`, `qd`, `q` nets stay `Default`),
  `unknown` emission — AP3.
- `Sequential`/`Unknown` origins still never emitted.
- `-input_ports`/`set_power_activity -pins` per-pin user seeds, `-duty`
  on annotations — no override surface exists yet (AP4).
- x/z half-weight *seed* handling beyond `SignalActivity` internals —
  `s_xz` held verbatim and is pinned.
- `s_p40` (uncommitted 40 ns SDC), `report_activity_annotation`'s exact
  per-pin split (our `origin_counts` stays per-net), the gcd
  `report_power` re-comparison — AP4.
- True-polarity constant marginals — measured **wrong**: the stored 0.0
  is what OpenSTA propagates (r3 probe). The true constant *value* is
  used only inside the marking logic.

## Decisions recorded (rationale in `api.md`/`evaluator.md`)

- Kahn order in `activity.rs`; `topological_levels` stays private.
- Propagation runs after the seed pass and only overwrites `Default`
  entries — precedence is the seed order, unchanged.
- Taint keys on the *function-referenced* input nets only.
- `constant` marking: 3-valued `const_eval` over the shared `comb_funcs`
  table — function-referenced inputs only gate marking too (an
  unreferenced floating input does not block a mark).
- Constant stored marginal: `Some(0.0)` for both polarities (measured).
- `unresolvable_functions` counts comb-driver nets whose `function`
  fails parse or var→net resolution at table build (any origin — the
  table is built before seeds), plus eval-side `None` at propagation;
  nets with `function: None` are never counted (unchanged).

## Files

- `crates/grus-core/src/activity.rs` — `comb_funcs` table, 3-valued
  `constant_cone`, propagation pass, annotation map, diagnostics
- `crates/grus-core/src/bool_expr.rs` — parser/AST/`prob`/`sensitivity`/
  `const_eval` + unit tests
- `crates/grus-core/src/lib.rs` — `pub mod bool_expr;`
- `crates/grus-core/src/design_db.rs` — `pin_by_lib_name`
- `crates/grus-core/src/readers/vcd.rs` — `SignalDecl.path`, `pin_signals`
- `crates/grus-core/src/readers/saif.rs` — `pin_signals` emit
- `crates/grus-core/src/power.rs` — tests (mba extension + cp fixture)
- `tests/designs/microbench/mba.def` etc. — existing fixtures, read-only
- `tests/designs/microbench/cp.def` + `cp_probe.tcl` — new (committed probe)
- `tests/designs/microbench/mb.power.json` — regenerated, byte-identical
- `docs/results.md`, `docs/roadmap.md`, `Cargo.toml`, `Cargo.lock`
- `kurena/tasks/ap2-combinational-propagation/design/` — this document set
