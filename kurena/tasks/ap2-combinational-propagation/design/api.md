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
  stay unmatched-counted as today. Note the landed asymmetry this
  creates — a `NET` entry naming an internal net annotates through its
  emitted driver-pin key, while the same flat name in a VCD dead-ends;
  the record lives in `docs/results.md` (see §5's enumeration).

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

### The shared `comb_funcs` table

One parse per `function`, built once in `resolve_activity` before the
seed pass and consumed by both `constant_cone` (§5) and the propagation
pass:

```rust
/// A combinational driver pin's parsed `function` with its
/// function-referenced input nets in variable order.
struct CombFunc {
    expr: bool_expr::BoolExpr,
    input_nets: Vec<NetId>,
}

/// Indexed by `NetId`. `Some` for every net whose driver pin sits on a
/// `!is_sequential && !is_latch` cell and whose `LibPin.function` parsed
/// and resolved all vars to nets. `function: None` is not a failure —
/// it produces `None` without counting.
fn comb_function_table(design: &DesignDb, lib: &LibDb, unresolvable: &mut usize)
    -> Vec<Option<CombFunc>>;
```

- A var matching no sibling pin (`DesignDb::pin_by_lib_name` — moved out
  of `power.rs`), or a pin with no net, or a `parse` `None` (malformed
  or over `MAX_DEPTH`/`MAX_NODES`/`MAX_VARS` — see `evaluator.md`) →
  `None`, counted in `unresolvable_functions`. PG forms like
  `!VPWR+VGND` parse fine but their vars match no design pin and land
  here.
- Counting is at table build, for **any** origin — the table is built
  before seeds, so an annotated net's broken function counts too.
  Broadens the v0.2.15 semantics slightly (counted only `Default`
  candidates); identical counts on every committed fixture (all 0).

### Candidate nets and input resolution

A net is a propagation candidate iff `origin == Default` and
`comb_funcs[n].is_some()`. Sequential/latch drivers and
`function: None` never enter the table, so the old `None` gate ("stays
`Default`, not counted") is unchanged. An eval-side `prob`/`sensitivity`
`None` (op budget, short `p`) still lands in `unresolvable_functions`
at the eval site.

- Only *referenced* inputs gate: `ebufn`'s `(A)` ignores `TE_B`; a
  `Default` on an unreferenced input does not taint.

### Level order — own Kahn, `topological_levels` untouched

Build the candidate dependency graph over `comb_funcs`: for every
candidate net `m`, `deps(m)` = its `input_nets` deduped; `indeg(m)` =
number of `deps` that are themselves candidates; `dependents(n)`
accumulates the reverse edges. Queue = candidates with `indeg 0`, popped
in `NetId` order (deterministic output for tests). The table is shared
and immutable now, so a `pending: Vec<bool>` keeps the enqueue-once
invariant — `debug_assert!(pending[m], "candidate dequeued twice")` then
`pending[m] = false` — instead of the old `Option::take` slot: an
impossible state must not carry a release panic path. On visiting `m`:

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

## 5. Constant marking vs. the stored marginal — measured (cp.def, 2026-11-21)

Review delta: the landed `constant_cone` seeds `prob_one = 0.0` (the P5
display-artifact record) and the propagation pass consumes it as a real
marginal — the polarity question had to be measured, not inferred.
Probe fixture: `tests/designs/microbench/cp.def` — a `conb_1` feeding one
input each of `and2`/`or2`/`xor2` gates whose other input is the port
`a`, annotated `-input_ports -density 0.003 -duty 0.55` (no clock —
absolute density). `cp_probe.tcl` is committed; the verbatim output is
in `docs/results.md`.

Measured reads (LibreLane 3.0.16):

| pin | gate form | OpenSTA read |
|---|---|---|
| `u_andh/X` | `and(n_hi, a)` | `0.00000e+00 0.000 propagated` |
| `u_xorh/X` | `xor(n_hi, a)` | `3.00000e+06 0.550 propagated` |
| `u_orh/X`  | `or(n_hi, a)`  | `0.00000e+00 0.000 constant` |
| `u_andl/X` | `and(n_lo, a)` | `0.00000e+00 0.000 constant` |
| `u_orl/X`  | `or(n_lo, a)`  | `3.00000e+06 0.550 propagated` |
| `u_xorl/X` | `xor(n_lo, a)` | `3.00000e+06 0.550 propagated` |
| `u_andhh/X`| `and(n_hi, n_hi)` | `0.00000e+00 0.000 constant` |
| `u_bufh/X` | `buf(n_hi)` | `0.00000e+00 0.000 constant` |

Three measured rules fall out:

1. **The stored activity record for a constant net is `{d=0, P=0}` on
   both polarities** — the P5 display artifact is the real internal
   value. `xor(n_hi, a)` propagating `0.55` (a's marginal, computed with
   `P(HI)=0`) instead of `0.45` (`¬a`) is the discriminator; `and(n_hi,
   a)` → `0/0` confirms. `prob_one: Some(0.0)` on `Constant` nets is the
   correct seed; propagation and `eval_when` keep consuming it. No
   `conditional_power_blocks` or golden moves are possible from this
   clause — `prob_one` is unchanged.
2. **The `constant` label uses true-polarity 3-valued evaluation** —
   a gate output is `constant` iff its function has a definite value
   with constant inputs bound to their true value: `or(1,X)=1`,
   `and(0,X)=0` mark even with a toggling other input (absorption);
   `and(1,X)`, `or(0,X)`, `xor(c,X)` do not mark. The landed
   all-inputs-constant rule misses the absorptive cases (`u_orh/X`,
   `u_andl/X`, `u_andhh/X` all read `constant`).
3. **Marking is structural** — an absorptive-constant pin stays
   `constant` under `set_power_activity -pins` and `-global` (measured);
   a `propagated` pin accepts both. Marking before the seed pass gives
   exactly this precedence — unchanged code order.

### `constant_cone` rewrite — 3-valued marking

`constant_cone` takes `&comb_funcs` (§2) instead of re-deriving pins.
Internal state `vals: Vec<Option<bool>>` (the net's constant value);
the return is still the marked set (`Vec<bool>`, `vals[i].is_some()`).
Two-phase sweep to a fixed point, same shape as landed:

- Seed: `comb_funcs[n]` with `Const` expr (`function : "0"/"1"` parses
  to `BoolExpr::Const`) — the literal check folds into the same rule.
- Sweep: for each unmarked table net, `vals[n] = bool_expr::const_eval(
  &expr, &input_vals)` where `input_vals[i] = vals[input_nets[i]]` —
  `None` for unmarked inputs (Kleene semantics: `And`→false on any
  `Some(false)`, `Or`→true on any `Some(true)`, else the value iff all
  inputs are known). Monotone in the information ordering — the fixed
  point is order-independent.
- Only *function-referenced* inputs gate marking — an unreferenced
  floating pin (`ebufn`'s `TE_B`) cannot block a mark, the same rule
  propagation taint uses.
- Sequential/latch drivers never enter `comb_funcs` → never mark
  (unchanged boundary).

The stored record for a marked net is **unchanged**
`{ prob_one: Some(0.0), eff_toggles: 0.0, origin: Constant }` — the
seed-site comment gains the measurement citation (the stored marginal is
the real internal value, not a display quirk; only the marking logic
knows the true polarity).

### Consequences enumerated

- `eval_when` / `conditional_power_blocks`: no moves possible —
  `prob_one` on constants is unchanged at `Some(0.0)`. A `when` literal
  on a HI-constant net keeps weight 0 — the same stored record OpenSTA's
  own machinery reads. On newly-marked absorptive nets a `when` weight
  would change, but no committed fixture mixes `when` literals with
  constant-fed pins (`mb`/`mbw`: `a`/`b`/`teb` only; `mba`: none; no gcd
  power test exists).
- Golden moves: none expected — `mba`'s cones are all-constant-input
  (identical marking); `mb`/`mbw` have no `conb`; `git diff` empty is
  asserted on every `*.reference.json`/`mb.power.json`.
- `driven_nets_without_activity`/`origin_counts` on `mba`: unchanged
  (`constant 4` — the absorptive rule marks no new net there).
- SAIF-vs-VCD asymmetry — record only (docs): a SAIF top-scope `NET`
  entry naming an internal net resolves by net name and annotates via
  the emitted driver-pin key (`pin_signals["u_and/X"]`), while the same
  flat name in a VCD dead-ends into `dead_annotation_entries`. The
  asymmetry is constructed by the pin-namespace emit — OpenSTA's own
  `read_saif` on such a `NET` entry is unmeasured (named residual).

## Files

- `crates/grus-core/src/activity.rs` — `comb_funcs` table, 3-valued
  `constant_cone`, annotation map, Kahn order, propagation pass,
  `unresolvable_functions`/`dead_annotation_entries`
- `crates/grus-core/src/bool_expr.rs` — `BoolExpr`, `parse`, `prob`,
  `sensitivity`, `const_eval` (new)
- `crates/grus-core/src/lib.rs` — `pub mod bool_expr;`
- `crates/grus-core/src/design_db.rs` — `pin_by_lib_name` (moved helper)
- `crates/grus-core/src/readers/vcd.rs` — `SignalDecl.path`,
  `VcdActivity.pin_signals`
- `crates/grus-core/src/readers/saif.rs` — `resolve` `pin_signals` emit
- `crates/grus-core/src/power.rs` — helper callsites, diagnostics doc line
- `tests/designs/microbench/cp.def`, `cp_probe.tcl` — constant-polarity
  probe fixture (new, committed for reproducibility)
