# api — `resolve_activity` AP3 deltas: `ff`/`latch` data pins, unified propagation, `unknown`

`resolve_activity`'s signature is unchanged. AP3 is a Liberty-parse
addition, a second producer table, and one generalized propagation pass
plus the leftover conversion inside `activity.rs`.

## 1. `lib_db.rs` — `LibCell` sequential fields

```rust
/// Sequential-cell data-pin reference, present on `ff`/`latch` cells
/// (the same cells `is_sequential`/`is_latch` flag).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LibSeq {
    /// `ff (v1, v2)` / `latch (v1, v2)` state variable v1 (`"IQ"`).
    /// An output pin whose `function` is exactly this name is the
    /// non-inverted state output.
    pub state_var: SymbolId,
    /// State variable v2 (`"IQ_N"`) — the inverted output; absent when
    /// the block declares one variable.
    pub state_var_n: Option<SymbolId>,
    /// The sequential data pin: `ff.next_state` / `latch.data_in` when
    /// the attribute is a bare pin name (`"D"`). `None` on expression
    /// forms — scan mux `"(D&!SCE) | (SCD&SCE)"`, data-enable
    /// `"(D&DE) | (IQ&!DE)"`, scan+enable — a named residual
    /// (census.md).
    pub data_pin: Option<SymbolId>,
}

pub struct LibCell {
    // ... unchanged fields ...
    pub seq: Option<LibSeq>,
}
```

- `LibDb` serializes into the Liberty cache (bincode) — bump
  `CACHE_VERSION` 8 → 9 in `readers/liberty.rs` (stale caches
  regenerate).
- The `LibCell` literal in `power.rs::group_of_classifies_by_cell_kind`
  gains `seq: None`.

## 2. `readers/liberty.rs` — `ff`/`latch` block-body parse

The `Token::Ident("ff")`/`("latch")` arms today collect the paren vars
and `skip_balanced_braces` the body. Replace the skip with a body loop
(same shape as `parse_pin_body`'s unknown-attribute arm):

- `Token::Ident("next_state")` (ff) / `Token::Ident("data_in")` (latch)
  → `:` → `Str`/`Ident` value → classify with `bool_expr::parse`:
  `Some((BoolExpr::Var(_), vars))` with the expression *being* a single
  var → `data_pin = Some(db.strings.intern(&vars[0]))`; any other parse
  result or `None` → `data_pin = None` (expression form — residual).
  Reusing `bool_expr::parse` defines "bare name" exactly as "a single
  Boolean variable" and inherits its bounds; `readers/liberty.rs` can
  call `crate::bool_expr::parse` (same crate).
- All other body attributes (`clocked_on`, `enable`, `clear`, `preset`,
  `clear_preset_var1/2`, …) → `skip_statement_or_group`; a `RBrace` or
  the helper's `true` return closes the `ff`/`latch` body — the cell
  body loop continues normally afterwards.
- `test_cell` groups stay skipped whole by the existing catch-all —
  their embedded `ff` copies never reach this code (census.md verifies
  cell-level counts are unaffected).
- `ff`/`latch` keep pushing the placeholder internal `LibPin`s for the
  state vars (unchanged — real pins sharing the names reuse the slots).
- `seq = Some(LibSeq { state_var: vars[0], state_var_n: vars.get(1),
  data_pin })`. A block declaring no vars (not present in sky130) →
  `seq: None` — unclassifiable, its outputs fall to `Unknown`.

## 3. `activity.rs` — the `seq_funcs` table

Parallel to `comb_funcs`, indexed by `NetId`, built in
`resolve_activity` before the seed pass:

```rust
/// A sequential output pin's copy rule: the net takes the resolved
/// activity of the cell's data-pin net — verbatim, or `1−P` on the
/// inverted state output.
struct SeqCopy {
    data_net: NetId,
    /// `true` when the driver pin's `function` names `state_var_n`
    /// (`"IQ_N"` — `Q_N` outputs on `*bp`/`*bn` cells).
    inverted: bool,
}

fn seq_function_table(design: &DesignDb, lib: &LibDb) -> Vec<Option<SeqCopy>>;
```

Per net, mirroring `comb_function_table`'s driver walk:

1. `design.net_driver(net)` → `pin_cells` → `cell_lib_ids` →
   `&lib.cells[lc]`; skip unless `lib_cell.seq` is `Some` (comb cells
   never enter — the tables are disjoint by construction since a net
   has one driver).
2. `let Some(data_sym) = seq.data_pin else continue` — expression-form
   `next_state` is never a candidate (its output nets stay `Default` →
   `Unknown`; **not** counted in `unresolvable_functions` — that counter
   is combinational-`function` failures only; the seq-expression
   residual is documented, not counted).
3. Classify the driver pin: `design.pin_lib_pins[drv]` →
   `lib.pins[lp].function`; `Some(f) == seq.state_var` →
   `inverted: false`; `Some(f) == seq.state_var_n` → `inverted: true`;
   otherwise (`None`, or a function naming neither var) → `None` —
   unclassifiable output, residual (sky130: every seq output pin's
   function is `IQ` or `IQ_N`, census.md).
4. `design.pin_by_lib_name(lib, cell, data_sym)` → `pin_nets` →
   `data_net`; a data pin absent from the cell or unconnected → `None`.

## 4. The propagation pass — unified producers

`propagate_combinational` generalizes to `propagate(&comb_funcs,
&seq_funcs, &mut nets, window_s, &mut unresolvable_functions)`. The
machinery is the landed AP2 Kahn; the deltas:

- **Candidate**: `origin == Default && (comb_funcs[n].is_some() ||
  seq_funcs[n].is_some())`.
- **Deps**: comb candidate → `input_nets` deduped; seq candidate →
  `[data_net]`. `indeg(n)` counts deps that are themselves candidates;
  `dependents` accumulates both edge kinds into the same reverse map.
- **Eval** on dequeue: comb arm unchanged (function `prob` +
  `sensitivity`, eval `None` → `unresolvable_functions` + stays
  `Default`). Seq arm: the data net's record has `prob_one: Some(p)`
  and `origin ∉ {Default, Unknown}` → write
  `ResolvedActivity { prob_one: Some(if inverted { 1.0 - p } else { p }),
  eff_toggles: data.eff_toggles, origin: Propagated }` — the counts are
  copied verbatim (`d` copies; `window_s` is shared); else stays
  `Default` (taint — same rule as comb inputs).

The fixed point is structural, not iterated: a resolved Q is an
ordinary resolved dep, so `Q → gate → D' → Q'` chains dequeue in order,
and sequential feedback cycles (`n_q0 ⇄ n_q0n`, `n_q1 ⇄ n_c1d`) are
mutual candidates that never reach `indeg 0` — they stay `Default`,
exactly like true combinational cycles.

### The `Default` → `Unknown` conversion

After the pass:

```rust
for r in &mut nets {
    if r.origin == ActivityOrigin::Default {
        *r = ResolvedActivity {
            prob_one: Some(0.0),
            eff_toggles: 0.0,
            origin: ActivityOrigin::Unknown,
        };
    }
}
```

Every leftover `Default` — feedback cycles, comb nets tainted by them,
seq outputs whose data pin is an expression form or unclassifiable,
comb eval failures, unparseable-function leftovers — lands `unknown`
`{0, 0}`, the measured P3 semantics. `Some(0.0)` (not `None`) matches
the golden's printed prob and keeps `when` literals on unknown nets
expressible at weight 0 (chosen; unmeasured — recorded in
results.md). Post-resolution, `Default` is an impossible origin —
`power.rs`'s `driven_nets_without_activity` comment updates to name
`Unknown` only.

## 5. `ActivityOverrides::input_ports` — the `s_input_ports` surface

`set_power_activity -input_ports [all_inputs]` annotates input-port
pins as `user` — pulled forward from AP4 because the spec requires all
12 sessions verbatim:

```rust
/// `-input_ports`: user annotation on input-port-driven nets —
/// `{density, P}` absolute. Port drivers only; undriven nets keep
/// `input` (measured: `s_input_ports`'s `n_float` reads `input`
/// `5e6/0.5`). Beats a dump entry on the same net (unmeasured —
/// recorded); loses to `-global` and `clock`.
pub input_ports: Option<ActivitySetting>,
```

Seed-pass arm, inserted between `global` and the dump `annotation`
arm (precedence: clock > global > **input_ports** > dump > input >
default — the s_input_ports golden reads `clock` on `clk`, `user` on
`a`/`b`, `input` on `n_float`):

```rust
} else if let Some(ip) = overrides.input_ports.filter(|_| {
    driver.is_some_and(|d| design.pin_cells[d.0 as usize].is_none())
}) {
    ResolvedActivity {
        prob_one: Some(ip.prob_one),
        eff_toggles: ip.density_per_s * window_s,
        origin: ActivityOrigin::Annotated, // golden label `user` maps here
    }
}
```

`driver.is_some_and(|d| pin_cells[d].is_none())` = "driven by an io
pin" — io-pin drivers are input ports by construction. Undriven nets
(`driver.is_none()`) skip to the `input` arm as before. API-level only —
no CLI flag, same as `global`/`input`.

## 6. Module doc / origin docs

- Header: drop "Sequential rules (AP3) are not landed yet — `Default`
  remains the interim fill"; the fill order gains "…→ seq Q=D copy and
  combinational propagation in one fixed point → leftover `Default`
  becomes `unknown`".
- `ActivityOrigin::{Sequential, Unknown}` docs: `Sequential` stays
  "declared for a possible measured source, never emitted"; `Unknown`
  becomes "unresolvable cone — emitted".

## Files

- `crates/grus-core/src/activity.rs` — `SeqCopy`/`seq_function_table`,
  `propagate` (ex-`propagate_combinational`), `Unknown` conversion,
  `input_ports` field + arm, doc updates
- `crates/grus-core/src/readers/liberty.rs` — `ff`/`latch` body loop,
  `CACHE_VERSION` 9
- `crates/grus-core/src/lib_db.rs` — `LibSeq`, `LibCell::seq`
- `crates/grus-core/src/power.rs` — `LibCell` literal, diagnostics
  comment, test rework (golden.md)
