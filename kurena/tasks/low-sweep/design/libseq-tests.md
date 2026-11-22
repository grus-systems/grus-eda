# low-sweep — `LibSeq` parser unit tests

Direct parser coverage for `parse_seq_cell_block` / `parse_seq_block_body`
(`readers/liberty.rs` ~L782–881). Today the `ff`/`latch` seq capture is
only exercised indirectly (`test_parse_sample_lib` asserts pins, never
`cell.seq`; the activity tests reach it through full pipelines). Four
new tests appended to `mod tests`, each parsing a minimal inline lib via
`parse_liberty_bytes` — no new fixtures, no PDK dependency.

All assertions go through `db.strings.resolve(sym)` on
`cell.seq.as_ref().unwrap()` fields; `data_pin`/`state_var`/`state_var_n`
are `SymbolId`s, so compare resolved names.

## 1. `test_seq_ff_vars_and_bare_next_state`

```liberty
cell (c) {
  ff ("IQ", "IQ_N") {
    clocked_on : "CLK";
    next_state : "D";
  }
  pin (CLK) { direction : input; }
  pin (D)   { direction : input; }
  pin (Q)   { direction : output; }
}
```

Assert: `cell.is_sequential`; `seq.state_var` resolves `"IQ"`;
`seq.state_var_n` is `Some` resolving `"IQ_N"`; `seq.data_pin` is `Some`
resolving `"D"` — the bare-`Var` arm of `bool_expr::parse` → `Some`.

## 2. `test_seq_latch_single_var_data_in`

```liberty
cell (c) {
  latch ("IQ") {
    enable : "GATE";
    data_in : "D";
  }
  pin (GATE) { direction : input; }
  pin (D)    { direction : input; }
  pin (Q)    { direction : output; }
}
```

Assert: `cell.is_latch`; `seq.state_var_n` is `None` (one header var);
`seq.data_pin` resolves `"D"` — covers the `data_in` attribute name and
the single-var `ff`/`latch` header.

## 3. `test_seq_expression_next_state_rejected`

Same cell as #1 but `next_state : "(D&!SCE) | (SCD&SCE)"` — the sky130
scan-mux form. Assert: `seq` is `Some` (state vars still captured) with
`data_pin` `None` — an expression parses but is not a bare `Var`, the
documented residual (`census.md`, `activity.rs` doc).

## 4. `test_seq_group_next_state_is_skipped`

Group form instead of a simple attribute:

```liberty
  ff ("IQ", "IQ_N") {
    next_state ("D") { clocked_on : "CLK"; }
  }
```

Trace: `Ident("next_state")` → `next_token` yields `LParen`, the
`Token::Colon` chain fails (the `LParen` is consumed and dropped);
`Str("D")`/`RParen` fall to `_ => {}`; `LBrace` hits
`skip_balanced_braces`; the body `RBrace` then closes normally. Assert:
parse succeeds, `is_sequential` true, `data_pin` `None`, `state_var`
still `"IQ"` — the malformed-form skip neither bails nor desyncs.
Cheap to include; keeps the edge pinned.

## Notes

- Put the four tests adjacent to `test_output_pin_function_is_interned`
  (~L2159) — the nearest existing `function`/seq-adjacent unit test.
- No `CACHE_VERSION` bump: tests read parser output, not the cache.
- Fail-proofing is inherent — e.g. dropping the `BoolExpr::Var` match
  arm makes tests 1/2 red; treat the suite as the evidence, no mutation
  run needed for a test-only change.
