# evaluator — `bool_expr.rs`, the `LibPin.function` evaluator

New module `crates/grus-core/src/bool_expr.rs` (`pub mod bool_expr` in
`lib.rs`, matching the no-glob convention). Consumed only by
`activity.rs` in AP2.

## Why Shannon cofactors, not per-operator rules

The api.md §3 per-operator table (AND `∏p`, OR `1−∏(1−p)`, XOR `p⊕q`,
INV `1−p`, BUF identity) is exact only when every variable occurs once.
sky130's `function` strings are SOP/POS forms — `xor2_1` is literally
`"(!A&B) | (A&!B)"`, `xnor2_1` `"(!A&!B) | (A&B)"` — and mux/majority
forms repeat literals (`"(A0&!S) | (A1&S)"`). Shannon expansion is exact
for arbitrary expressions under input independence and reduces to the
per-operator table on the common forms; it is the same math, once.

## Grammar (covers the sky130_fd_sc_hd alphabet)

Measured inventory of `sky130_fd_sc_hd__tt_025C_1v80.lib` `function`
strings (943 decls): operators `! & | ^ ( )`, `+` (OR, on PG-pin forms
like `!VPWR + VGND`), `*` (AND, e.g. `(CLK*M0)`), constants `"0"`/`"1"`,
identifiers `[A-Za-z_][A-Za-z0-9_]*` (`A1`, `B1_N`, `SLEEP_B`, `CIN`,
`VPWR`, `D_N`). No `'` postfix in this lib; keep it in the grammar anyway
(Liberty allows `X'` for NOT — one clause).

```text
expr  := or
or    := xor (('|' | '+') xor)*
xor   := and ('^' and)*
and   := unary (('&' | '*') unary)*
unary := '!' unary | postfix
postfix := atom '\''*
atom  := '(' expr ')' | ident | '0' | '1'
ident := [A-Za-z_][A-Za-z0-9_.$]*
```

Precedence `! > & > ^ > |`, left-associative within a level. Whitespace
tolerated anywhere. Trailing garbage, unknown tokens, unbalanced parens,
empty input → parse `None` (total — never panics).

## AST and evaluation

```rust
pub enum BoolExpr {
    Const(bool),
    Var(u16),                 // index into the caller's input list
    Not(Box<BoolExpr>),
    And(Box<BoolExpr>, Box<BoolExpr>),
    Or(Box<BoolExpr>, Box<BoolExpr>),
    Xor(Box<BoolExpr>, Box<BoolExpr>),
}

/// Parses `text`; `vars` is the ordered set of identifier names the
/// caller resolves to pins (each distinct name → one `Var` index, in
/// first-use order). `None` on any malformed input.
pub fn parse(text: &str) -> Option<(BoolExpr, Vec<String>)>;

/// Exact `P(expr = 1)` under input independence, `p[i]` = `P(var i = 1)`.
/// Shannon: pick the lowest-indexed free variable `x`, return
/// `p_x·prob(f|x=1) + (1−p_x)·prob(f|x=0)`; cofactors substitute and
/// simplify eagerly, so non-degenerate forms collapse early.
/// `None` when the op bound is exceeded.
pub fn prob(e: &BoolExpr, p: &[f64]) -> Option<f64>;

/// `P(∂f/∂x_i)` — the probability input `i` is sensitized:
/// `prob(cofactor(f,i,false) ⊕ cofactor(f,i,true))`.
pub fn sensitivity(e: &BoolExpr, i: usize, p: &[f64]) -> Option<f64>;
```

Bounds: variable count ≤ 32 and a total op counter (e.g. 1_000_000 node
evaluations) — sky130 functions top out at ~10 vars; the bound only
exists so a hostile/pathological string cannot blow the recursion out.
`Const`/`Var`-free results shortcut to `0.0`/`1.0`.

## Output activity (the measured M1 model)

Given resolved inputs `{p_i, d_i}` (densities in transitions/second):

- `P(f) = prob(f, p)` — checked: `u_and/X` `0.44`,
  `u_xor/X` `0.47` (s_ports).
- `d(f) = Σ_i d_i · sensitivity(f, i, p)` — checked: `u_and/X`
  `3e6·0.8 + 3e6·0.55 = 4.05e6`; `u_or/X` `4.05e6·0.8 + 3e6·0.56 =
  4.92e6`; `u_xor/X` `3e6 + 3e6 = 6e6` (every input edge toggles —
  `∂(xor)/∂x = 1`). Bit-matches the golden at print precision.
- Constants collapse: `f` var-free → `P ∈ {0,1}`, `d = 0`.
- An input with `prob_one: None` (never-defined annotated dump) makes the
  output unresolvable — the caller keeps `Default`.

## Unit tests (in `bool_expr.rs` `#[cfg(test)]`)

- Grammar: each operator, precedence (`!A&B|C` = `((!A)&B)|C`), parens,
  `+`/`*`/`'` alternates, constants, whitespace; malformed forms return
  `None`.
- `prob`: `A&B`→`p·q`; `A|!A`→`1.0` (repeated literal — the case the
  per-operator table gets wrong); `A&A`→`p`; SOP xor `(!A&B)|(A&!B)` at
  `p=.55,q=.8`→`0.47`; mux `(A0&!S)|(A1&S)` cross-check vs. hand truth
  table.
- `sensitivity`: AND `∂/∂a`→`p_b`; XOR→`1.0`; `A&A` `∂/∂a`→`1.0`.
- Bound: a 40-var expression → `None` (no hang, no panic).

## Files

- `crates/grus-core/src/bool_expr.rs` — parser, AST, `prob`,
  `sensitivity`, unit tests (new file, the whole module)
- `crates/grus-core/src/lib.rs` — `pub mod bool_expr;` (one line)
- `crates/grus-core/src/activity.rs` — sole consumer: driver-pin
  `function` → `parse` → var→pin map → `prob`/`sensitivity`
