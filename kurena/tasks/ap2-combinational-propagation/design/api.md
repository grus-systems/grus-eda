# api — `resolve_activity` AP2 deltas: pin-namespace annotation + levelized fill

`resolve_activity`'s signature and `estimate_power` are unchanged; AP2 is
entirely inside the resolution pipeline plus the two producer-side emit
changes.

## 1. Pin-namespace annotation

OpenSTA annotates *pins* by dump name; our model resolves per *net*, so a
net's annotated value is its **driver pin's** annotation (P4: net
switching consumes the driver pin's value; a sink pin's annotation is a
per-pin property our model cannot represent — `s_full`'s port `y`).

### `readers/vcd.rs` — `VcdActivity.pin_signals`

`SignalDecl` gains `path: String` = the `$scope` chain at declaration
joined with `/` plus the leaf name (`tb/dut/_d_/Q`, `tb/dut/n_and`).
`VcdActivity` gains:

```rust
/// Per-pin annotation candidates, keyed by hierarchical path
/// (`tb/dut/u_and/X`). Every declaration yields an entry; declarations
/// sharing an identifier code share the retained (shallowest) stream's
/// activity — the existing alias semantics, made visible.
pub pin_signals: HashMap<String, SignalActivity>,
```

`signals` is **unchanged** (flat leaf names; shallowest-decl-wins on id
reuse) — the `mb` saif test reads `vcd.signals` by name and stays
byte-identical. Vector signals expand to `path[bit]` like `signals`.
`pin_signals` emits an entry for *every* declaration, including
top-scope ones (`tb/dut/a`) — the matcher handles them uniformly, and
id-sharing (shadowed) decls get an alias entry copying the shared
stream's final `SignalActivity`.

### `readers/saif.rs` — `resolve` emits `pin_signals`

- Top-scope signal resolving to a design net: `signals[net_name]`
  unchanged (the `mb` saif test reads `activity.signals` by name), **plus**
  a `pin_signals` entry keyed by the net's driver pin — `{port}` for an
  io-pin driver (`tb/dut/a` → `a`), `{cell}/{leaf}` for a cell-pin driver
  (`tb/dut/q` → `_d_/Q`). A SAIF `NET` entry annotates the pin driving
  it — `mb`'s `q` lands on `_d_/Q` either way (the `_d_` `NET` block
  emits the same key), so this only matters when no instance block
  exists.
- Nested-instance signals (`_d_` scope, `Q`): `pin_signals["{inst}/{leaf}"]`
  regardless of sink/driver — the matcher drops sinks, which also fixes
  today's latent sink-overwrite hazard (`_i_/A` used to overwrite
  `_d_/Q`'s entry on net `q`).
- Emit order: top-scope first, then instances — on a key collision the
  instance value wins (identical in consistent fixtures; recorded).
- `unmatched_scopes`/`unmatched_signals` counting is unchanged (a scope
  name missing from `cell_of` counts and emits nothing — `ghost` still
  exercises the counter).
- SAIF top-scope `NET` entries whose name is neither a net nor a port
  stay unmatched-counted as today; a `NET` named like an internal net but
  carrying no instance coverage now dead-ends the same way VCD flat names
  do (SAIF-vs-VCD top-scope asymmetry is unmeasured — record in results).

### `activity.rs` — the match

Replace the `vcd.signals.get(net_name)` arm with a precomputed
`annotation: HashMap<NetId, SignalActivity>` built before the per-net
pass, from **`vcd.pin_signals` only** (`signals` stays a compat/debug
view — the saif test's cross-check reads it, `resolve_activity` does
not):

```rust
/// Dump path → candidate driver pin:
///   last two `/`-components = `inst`/`leaf` → cell `inst`'s pin `leaf`;
///   else last component → io pin `leaf`.
/// A pin annotates its net iff `design.pin_is_driver[pin]` — input ports
/// and instance output pins drive; output ports and instance input pins
/// do not (their dump entries are dead for net resolution).
fn annotation_net_of(design: &DesignDb, path: &str) -> Option<(PinId, NetId)>;
```

- For each `(path, sa)` in `vcd.pin_signals`: try the last-two-components
  `cell/pin` rule first — if both cell (flat `cell_names` lookup; deeper
  instance hierarchy is unrepresentable in `design_db`, recorded
  limitation) *and* pin resolve, the entry is decided here:
  `pin_is_driver` → `(pin, net)`, non-driver → dead (no fall-through —
  `u_and/D`'s stream must never annotate an unrelated port named `D`).
  Only when cell or pin fails to resolve does the last component fall
  through to the `io_pins` name match → `pin_is_driver` → `(pin, net)`
  (a `dut`-scoped leaf is not a cell pin — `tb/dut/a` lands on io `a`;
  `tb/dut/y` lands on io `y`, non-driver → dead).
- Collision rule — **first path wins**: distinct dump paths can resolve
  to the same driver pin (e.g. `tb/dut/u_and/X` and `tb/other/u_and/X`
  both match the flat `u_and/X` — hierarchy deeper than the flat cell
  namespace collapses to the same pin). The `HashMap` insert is
  `annotation.entry(net).or_insert(*sa)` over `dump_paths` iterated in
  sorted order, so the lexicographically smallest path wins and the
  choice is deterministic. On consistent dumps the colliding streams
  are VCD aliases of the same identifier code and hold identical
  activity, so the rule only bites on pathological dumps — document it
  in a comment at the `or_insert` site either way.
- Entries mapping to nothing
  (non-driver io pins, input instance pins, unknown names) are dead —
  count them into a `dead_annotation_entries: usize` on
  `ActivityResolution` for the results entry (`s_full`: 15 dead of 18 —
  14 names matching no pin + output port `y`'s non-driver io pin;
  `s_partial`: 1 dead of 4 — `n_and`).
- A `Constant`-cone net with a dump hit is still `Constant` — precedence
  order is unchanged (the annotation map is only consulted at the
  annotation arm).

## 2. Propagation pass (after the seed pass)

The seed pass is unchanged: it still writes `Default` onto every unseeded
cell-driven net. AP2 then overwrites `Default` entries it can derive —
precedence is still "seed order, then propagated", matching the measured
model (constants, clocks, globals, annotation, inputs all win over
propagation).

### Candidate nets and input resolution

A net is a candidate iff `origin == Default` and its driver pin belongs
to a cell whose `LibCell` is `!is_sequential && !is_latch`. For the
driver pin's `LibPin.function`:

- `None` (dff `Q`, PG pins) → stays `Default`, not counted (no function
  to evaluate).
- parse → `BoolExpr` + var names; each var maps to the cell's design pin
  whose `lib.pins[lp].name` equals it — `power.rs`'s private
  `sibling_pin_by_lib_name` **moves** to `DesignDb::pin_by_lib_name`
  (`pub(crate)`, same body; `activity.rs` sits upstream of `power.rs` so
  it cannot call into `power`, and a duplicated copy would drift — the
  two `power.rs` callsites switch to the method). A var matching no
  pin, a pin with no net, or a `parse` `None` (malformed or over the
  `MAX_DEPTH`/`MAX_NODES`/`MAX_VARS` bounds — see `evaluator.md`) →
  stays `Default`, counted in `unresolvable_functions` (the "sky130 lib
  count" the acceptance asks to record — measure on `tt_025C_1v80`; PG
  forms like `!VPWR+VGND` parse fine but their vars match no design pin
  and land here). An eval-side `prob`/`sensitivity` `None` (op budget,
  short `p`) lands in the same counter.
- Only *referenced* inputs gate: `ebufn`'s `(A)` ignores `TE_B`; a
  `Default` on an unreferenced input does not taint.

### Level order — own Kahn, `topological_levels` untouched

Build the comb-net dependency graph once: for every candidate net `m`
with driver cell `c`, `deps(m)` = the distinct nets of `c`'s
function-referenced input pins; `indeg(m)` = number of `deps` that are
themselves candidates; `dependents(n)` accumulates the reverse edges.
Queue = candidates with `indeg 0`, popped in `NetId` order (deterministic
output for tests). A candidate enqueues at most once (its `indeg` reaches
0 exactly once), so the dequeue `take()` of its slot is never empty —
encode that invariant as `let Some(cand) = candidates[m].take() else {
debug_assert!(false, "candidate dequeued twice"); continue; }`, not
`expect`: an impossible state must not carry a release panic path. On
visiting `m`:

- Every function var's input net has `origin ∉ {Default, Unknown}` and
  `prob_one.is_some()` → evaluate `P(f)`, `d(f)`; write
  `Propagated { prob_one: Some(P), eff_toggles: d(f) * window_s }`.
- Else → stays `Default` (taint; `u_i0/Y`, `u_x1/X` on `n_q0`/`n_q1`).
- Decrement dependents either way — visit order is scheduling only;
  a dependent whose input stayed `Default` taints on its own eval.
- Candidates never dequeued (true combinational cycles — none in `mba`,
  possible in general) keep `Default`; counted in `unresolvable_functions`
  too? — **no**: cycles are not function failures; they just stay
  `Default` (decision: no separate counter, `driven_nets_without_activity`
  already sees them).

Verified against the golden cone: `n_and` ready at seed (deps `a`,`b`
are seeds) → `4.05e6/0.44`; `n_or` unlocks when `n_and`,`n_inv` produced
→ `4.92e6/0.552`; `y` on `a`,`b` → `6e6/0.47`; `n_flt` on `n_float`
(`Input` seed) → `5e6/0.5`; `n_q0n`,`n_c1d` ready immediately (deps are
seq-driven `Default` seeds, not candidates) → tainted → `Default` ✓.

### `ActivityResolution` gains

```rust
/// Nets left `Default` because the driver pin's `function` was
/// unparseable, over a parse/eval bound, or named vars matching no
/// input pin of the cell (AP2 diagnostic — sky130 coverage recorded
/// in docs/results.md).
pub unresolvable_functions: usize,
/// Dump entries matching no driver pin (dead annotations — flat
/// internal names, sink pins, unknown names). Diagnostic only.
pub dead_annotation_entries: usize,
```

`origin_counts` is unchanged code — `propagated` appears automatically.
`driven_nets_without_activity` (`power.rs`) counts `Default`/`Unknown`
driven nets — it shrinks by the propagated count on partial VCDs; the
movement is recorded in `results.md`.

## 3. `power.rs` — mechanical helper move only

`estimate_power`, `PowerCx`, `switching_w`, `pin_internal_power`,
diagnostics plumbing: untouched. Non-test diff is the
`sibling_pin_by_lib_name` → `DesignDb::pin_by_lib_name` callsite switch
(two call sites) plus a one-line `driven_nets_without_activity` doc
update naming `Propagated` as a derived source. Test changes are in
`golden.md`.

## 4. Propagated-value check vs. golden (per session)

| net | function | `s_none` (5e6/0.5 seeds) | `s_input` (1e7/0.4) | `s_ports` (a 3e6/.55, b 3e6/.8) |
|---|---|---|---|---|
| `n_and` | `(A&B)` | `5e6`/`0.25` | `8e6`/`0.16` | `4.05e6`/`0.44` |
| `n_inv` | `(!A)` on `b` | `5e6`/`0.5` | `1e7`/`0.6` | `3e6`/`0.20` |
| `n_or`  | `(A)|(B)` | `6.25e6`/`0.625` | `1.16e7`/`0.664` | `4.92e6`/`0.552` |
| `y`     | `(!A&B)|(A&!B)` | `1e7`/`0.50` | `2e7`/`0.48` | `6e6`/`0.47` |
| `n_flt` | `(A)` on `n_float` | `5e6`/`0.5` | `1e7`/`0.4` | `5e6`/`0.5` |

Every entry equals the golden read verbatim — the M1 model reproduced by
the evaluator, not re-measured.

## Files

- `crates/grus-core/src/activity.rs` — annotation map, Kahn order,
  propagation pass, `unresolvable_functions`/`dead_annotation_entries`
- `crates/grus-core/src/bool_expr.rs` — `BoolExpr`, `parse`, `prob`,
  `sensitivity` (new)
- `crates/grus-core/src/lib.rs` — `pub mod bool_expr;`
- `crates/grus-core/src/design_db.rs` — `pin_by_lib_name` (moved helper)
- `crates/grus-core/src/readers/vcd.rs` — `SignalDecl.path`,
  `VcdActivity.pin_signals`
- `crates/grus-core/src/readers/saif.rs` — `resolve` `pin_signals` emit
- `crates/grus-core/src/power.rs` — helper callsites, diagnostics doc line
