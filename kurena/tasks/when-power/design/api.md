# API design — lib_db.rs, readers/vcd.rs, readers/saif.rs, power.rs

All shapes below implement the **H1 marginal** verdict
(`overview.md`/`probe.md`): a `when` block contributes its table energy ×
the owning pin's net transitions × `P(when)`, with `P` = product of the
literal pins' measured state probabilities. Where the probe could return
a different verdict, the call site is shaped so the flip is one line —
marked *(verdict-dependent)*.

## `crates/grus-core/src/readers/vcd.rs`

`SignalActivity` gains state durations (seconds over the annotation
window; x/z time enters neither):

```rust
pub struct SignalActivity {
    pub rise: u64,
    pub fall: u64,
    /// Seconds in state 0 / state 1 within the dump window. `t0+t1 == 0`
    /// means the state was never defined — probability is unknown, not ½.
    pub t0_s: f64,
    pub t1_s: f64,
}
```

`P(high)` is `t1/(t0+t1)` at the consumer — do not precompute a ratio
here (the `0/0` case must stay distinguishable from "annotated, constant").

Duration bookkeeping (O(1) per change event; `parse_vcd` already tracks
`cur_ts` and per-id `last_state`):

- Per identifier add `since: Vec<u64>` (last-change ts per bit) and
  `dur: Vec<[u64; 3]>` (ticks accumulated in state 0/1/other per bit),
  alongside `last_state`.
- `apply_change`/`apply_vector_change`: for each bit, charge
  `dur[old_state] += cur_ts - since[bit]`, set `since[bit] = cur_ts`,
  then update `last_state`. Unchanged bits also advance `since` — their
  state continues.
- After the body loop: for every kept `decls` entry, close each bit
  (`dur[last_state] += last_ts - since`), then fold into `signals` by the
  decl's name — scalar → `name`, vector → `name[i]` per bit — with
  `t0_s = dur[0] * timescale_s`, `t1_s = dur[1] * timescale_s`, via
  `entry(name).or_default()` so constant signals get `{0,0,t0,t1}`.
- Semantic shift to record: `signals` now holds every *declared* signal,
  so `driven_nets_without_activity` means "net not annotated" (constant
  driven nets no longer count). No committed test asserts it nonzero.

`timescale_s`/`duration_s`/`signals` map shape: unchanged. Unit tests:
durations on a mini VCD (known T0/T1, x time excluded, constant signal
emitted, vector per-bit names), plus the existing edge tests updated for
the two new fields.

## `crates/grus-core/src/readers/saif.rs`

`annotate()` fills the new fields from SAIF marginals —

```rust
signals.insert(name, SignalActivity {
    rise, fall,
    t0_s: f.t0 * self.timescale_s,
    t1_s: f.t1 * self.timescale_s,
});
```

`SaifFields`/`resolve` signature/diagnostics unchanged. Duty was already
parsed for the odd-`TC` split; the power model now consumes it for `when`
probabilities — update the module doc ("duty is parsed and carried but
not consumed" → consumed by `when` evaluation; flat leakage stands unless
P4 says otherwise).

## `crates/grus-core/src/lib_db.rs`

```rust
/// One literal of a `when` state expression.
pub struct WhenLit {
    /// Liberty pin name of the same cell.
    pub pin: SymbolId,
    /// `!`-negated.
    pub inverted: bool,
}

/// Parses `when` text into a conjunction of literals: `lit (& lit)*`,
/// `lit` = optional `!` + pin name. Returns `None` for any other form
/// (`|`, `^`, parens, empty terms, unknown pin names) — the caller counts
/// the block inexpressible.
pub fn parse_when_expr(expr: &str, strings: &StringInterner)
    -> Option<Vec<WhenLit>>;
```

`LibPinInternalPower.when` stays `Option<SymbolId>` (raw text) — no
Liberty-reader or LibDb-serialization change (cache version untouched).
Unit tests: `"A"`, `"!A"`, `"A&!TE_B"`, whitespace variants → literals;
`"A|B"`, `"(A&B)|C"`, `"A^B"`, `""`, `"A&"` → `None`.

## `crates/grus-core/src/power.rs`

`PowerCx` gains `net_p1: Vec<Option<f64>>` — per-net `P(net=1)`, built in
`estimate_power` next to `net_activity` from `vcd.signals[name]`:
`Some(t1/(t0+t1))` when `t0+t1 > 0`, else `None`.

`pin_internal_power` — replace the early `continue` on `ip.when`:

```rust
let weight = match &ip.when {
    None => 1.0,
    Some(sym) => match eval_when(cx, cell, *sym) {
        Some(p) => p,
        None => {
            cx.diagnostics.conditional_power_blocks += 1;
            continue;
        }
    },
};
…
groups[group].internal_w += energy_j * count as f64 * weight / cx.duration_s;
```

```rust
/// Marginal probability a `when` expression holds, under the measured
/// rule: product over literals of the literal pin's net state
/// probability (independence — OpenSTA's own marginal model; see
/// docs/results.md). `None` = inexpressible.
fn eval_when(cx: &PowerCx, cell: CellId, when: SymbolId) -> Option<f64> {
    let lits = parse_when_expr(cx.lib.strings.resolve(when), &cx.lib.strings)?;
    let mut p = 1.0f64;
    for lit in lits {
        let pin = sibling_pin_by_lib_name(cx.design, cx.lib, cell, lit.pin)?;
        let net = cx.design.pin_nets[pin.0 as usize]?;
        let p1 = cx.net_p1[net.0 as usize]?;
        p *= if lit.inverted { 1.0 - p1 } else { p1 };
    }
    Some(p)
}
```

`conditional_power_blocks` doc becomes: "`internal_power` `when` blocks
skipped because the expression is outside the literal-conjunction
grammar, names no sibling pin, or its pin's net has no measured state
duration." One counter covers all three — the JSON schema and the
`main.rs` diagnostic line are unchanged (`main.rs` untouched).

*(verdict-dependent)* **Count-net knob**: the block keeps counting on the owning pin's net
(`count_net` from `pin`). If P1 returns "related pin", that one line
switches to `cx.design.pin_nets[trigger]` — name the choice in the commit
and `results.md`.

*(verdict-dependent)* **Joint-verdict contingency**: if P2/P3 return H2, SAIF `when` blocks
are inexpressible by construction (stay counted); the VCD side would need
per-edge condition sampling — out of this shape, rescope with the user.

Update the module doc's `when` paragraph (currently "skipped and counted
— needs value-correlated activity") to the measured rule.

## `crates/grus-eda/src/main.rs`

Unchanged — the diagnostics line already prints
`conditional_power_blocks` under its narrowed meaning.

## Tests

- `lib_db`/`vcd`/`saif` unit tests as above (no PDK needed).
- `power.rs` `test_when_power_mbw_matches_opensta` (`fixture.md` §Test):
  STRICT golden compare + `conditional_power_blocks == 0` + duty sanity
  assertions + fail-proof break/restore.
- Existing tests keep passing: `mb`/`mb.saif` fixtures have no `when`
  cells; `SignalActivity` gains fields so literal constructors get
  `..Default::default()`.
