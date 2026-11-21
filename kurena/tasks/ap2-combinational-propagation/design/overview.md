# AP2 — combinational propagation: overview

Second ticket of the AP1–AP4 sequence
(`kurena/tasks/activity-propagation/design/api.md` §Plan; measured semantics:
`docs/results.md` 2026-11-21 N7 probe, esp. P2/P4; golden:
`tests/designs/microbench/mba.activity.reference.json`, 12 sessions × 43 pins).

AP2 replaces the AP1 `Default` interim fill on nets driven by combinational
cell outputs with a propagated `{prob_one, density}` — origin `Propagated` —
per the measured **M1 local-marginal** model.

## What lands

1. **Gate-function evaluator** (`crates/grus-core/src/bool_expr.rs`, new
   sibling module — the spec's Files list allows it). Parses arbitrary
   `LibPin.function` strings and computes, under input independence:
   `P(f)` exactly via Shannon cofactors (repeated literals and any
   expression shape — the per-operator product rules in api.md §3 are the
   same math on the common forms), and
   `d(f) = Σ_i d_i · P(∂f/∂x_i)` with `∂f/∂x_i = f(x_i=0) ⊕ f(x_i=1)`
   (output sensitization — measured P2). Total and bounded: parse caps
   recursion depth (`!`/`(`) and AST node count, so a pathological
   `function` string returns `None` instead of overflowing the stack —
   the cap then covers every recursive consumer (`cofactor`,
   `lowest_var`, `const_prob`); `MAX_OPS` bounds Shannon work, not
   stack. A missing, constant, unparseable, or over-bound function
   returns `None`; the net keeps `Default`. See `evaluator.md`.
2. **Levelized fill** inside `activity.rs`. Combinational cells evaluate in
   topological order over the net graph. **Decided: an own Kahn order
   inside `activity.rs`** — `sta::analysis::topological_levels` stays
   private (it drives a STA `TimingGraph`/`Context` that `power` does not
   build; the net-level Kahn here is ~40 lines over `design_db` alone).
   Sequential/latch cells are boundaries: their output nets are never
   produced (AP3), their input nets are ordinary sinks/sources.
3. **Input gating (taint)** — propagation consumes only resolvable inputs
   (`Annotated`, `Clock`, `Global`, `Input`, `Constant`, `Propagated` with
   `prob_one: Some`). A gate whose function references any `Default`/
   `Unknown` input keeps `Default` output: the sequential-feedback cones
   (`u_i0/Y`, `u_x1/X`) stay unresolved until AP3.
4. **Pin-namespace annotation** — the hidden prerequisite of this ticket;
   see below.
5. **Golden coverage** — the `mba` activity test gains the VCD sessions;
   `EXPECTED_DIVERGENT` shrinks to the AP3 set (see `golden.md`; **13
   pins**, one more than the spec's shorthand — `qd` is net `q`'s output
   port and shares the `u_df/Q` residual).

## The hidden prerequisite: pin-name annotation matching

The spec's strict list includes `s_partial` and `s_full`, whose VCDs dump
flat internal net names (`n_and`, `n_inv`, …). Measured P4: OpenSTA's
`read_vcd` annotates **pins whose name appears in the dump** — flat
top-scope names match *port* pins only; flat internal net names are dead
entries. Our AP1 flat net-name match (`vcd.signals.get(net_name)`)
annotates `n_and` at the deliberately inconsistent `1e6/0.80` and would
diverge `n_or` downstream — so the match must change.

But mb/mbw must stay **byte-identical**: their cell-driven nets (`q`, `y`,
`z`) are annotated by OpenSTA through the *scoped* `inst/pin` dumps
(`_d_/Q`, `_i_/Y`, `u_t/Z`), not through the flat names. Dropping
internal-net annotation without scoped matching would send `q`/`y`/`z` to
`Default`/`Propagated`, moving `mb.power.json` and breaking
`mbw.reference.json`'s 0.1% bounds.

Resolution: annotate at the pin level, resolve per net from the driver
pin — OpenSTA's own model collapsed onto our per-net representation:

- `VcdActivity` gains `pin_signals` (hierarchical `scope/…/leaf` keys);
  `signals` is unchanged for compatibility.
- `SaifActivity::resolve` emits the same pin namespace (nested-instance
  signals as `inst/leaf`, top-scope resolved nets via their driver pin).
- `resolve_activity` builds a net→annotation map from *driver pins only*:
  a leaf name annotates the io-pin-driven net it names (input ports;
  output-port dumps like `y` are per-pin annotation our model cannot use);
  an `inst/leaf` path annotates a cell output pin's net.
- Sink-pin and unmatched dump entries are dead — exactly the `n_and` and
  port-`y` semantics, and `mba_partial2`'s scoped `u_and/X` annotates
  `n_and` for free (see `golden.md`; `s_partial2` coverage comes along if
  it holds verbatim).

Rejected alternative: leaf-name matching (flat `Q` → any driver pin
leaf-named `Q`). It reproduces every committed fixture but annotates
across instance boundaries on collisions (`u_and/X`'s dump would hit all
seven `X`-output nets in `s_partial2`) — a wrong model masquerading as a
match; the hierarchical form above is the honest version and is what AP4
needs anyway.

## What does not land

- Sequential rules: Q=D fill (`u_df/Q`, `qd`, `q` nets stay `Default`),
  `unknown` emission — AP3.
- `Sequential`/`Unknown` origins still never emitted.
- `-input_ports`/`set_power_activity -pins` per-pin user seeds
  (`s_input_ports` session), `-duty` on annotations — no override surface
  exists yet (AP4).
- x/z half-weight *seed* handling beyond the existing
  `SignalActivity::eff_toggles`/`prob_one` — `s_xz` is expected to hold
  verbatim already and is pinned if it does (AP4 otherwise).
- `s_p40` (uncommitted 40 ns SDC), `report_activity_annotation`'s exact
  per-pin split (our `origin_counts` stays per-net), `conditional_power_blocks`
  movement, the gcd `report_power` re-comparison — AP4.

## Decisions recorded (rationale in `api.md`/`evaluator.md`)

- Kahn order in `activity.rs`; `topological_levels` stays private.
- Propagation runs after the seed pass and only overwrites `Default`
  entries — precedence is the seed order, unchanged.
- Taint keys on the *function-referenced* input nets (a `Default` on an
  unreferenced pin does not taint — `ebufn`'s `(A)` ignores `TE_B`).
- Propagated values store `density × window_s` in `eff_toggles` — the
  counts representation of AP1 is untouched.
- `ActivityResolution` gains `unresolvable_functions: usize` (nets left
  `Default` because the driver `function` was missing/unparseable/
  unmapped) — the sky130 count is recorded in the results entry.
- Clock-origin nets feeding combinational cells propagate like any seed:
  a cell-driven downstream clock net becomes `Propagated {2/period, 0.5}`
  (unmeasured source label; `mba` has no clock tree — named residual).

## Changes since the rejected r1 design

- `evaluator.md`: added the missing stack bound — `MAX_DEPTH` on parser
  recursion (`!`/`(` descent) plus `MAX_NODES` on AST construction, which
  together cap `cofactor`/`lowest_var`/`const_prob`/`prob_rec` depth;
  `MAX_OPS` now documented as a work bound only. Bound-trip unit tests
  specified for each cap.
- `api.md`: documented the first-path-wins collision rule on
  `annotation.entry(net).or_insert` (sorted dump paths; deterministic,
  identical on consistent dumps); the dequeue `take()` invariant is a
  `debug_assert` + `continue`, not `expect`.
- `evaluator.md`: `const_prob`'s unreachable `Var` arm is a
  `debug_assert` with a `0.0` fallback — no silent unreachable path, no
  release panic.
- `golden.md`: noted the equal-window assumption behind the verbatim
  `s_partial == s_ports` net-vector compare (`#2000` on both fixtures;
  raw `eff_toggles` counts are only comparable at equal `window_s`).

## Files

- `crates/grus-core/src/activity.rs` — pin-namespace annotation map,
  Kahn level order, propagation pass, `unresolvable_functions`
- `crates/grus-core/src/bool_expr.rs` — new: `function` parser +
  Shannon-cofactor `prob`/`sensitivity` evaluator
- `crates/grus-core/src/lib.rs` — `pub mod bool_expr;`
- `crates/grus-core/src/design_db.rs` — `pin_by_lib_name` helper moved
  from `power.rs` (shared by the propagation input map and `when`
  evaluation; `activity.rs` cannot call upstream into `power.rs`)
- `crates/grus-core/src/readers/vcd.rs` — scope paths on decls,
  `VcdActivity.pin_signals` (reader change; not in the spec's `## Files`
  list — required by the pin-namespace match, see above)
- `crates/grus-core/src/readers/saif.rs` — `resolve` emits `pin_signals`
  (same reason)
- `crates/grus-core/src/power.rs` — test updates only (new sessions,
  strict/mapping rules, `EXPECTED_DIVERGENT`, per-pin residual table)
- `docs/results.md`, `docs/roadmap.md` — N7 entry; N7 line → AP2 done
- `kurena/tasks/ap2-combinational-propagation/design/` — these docs
- `Cargo.toml`, `Cargo.lock` — `0.2.14` → `0.2.15` in the same commit
