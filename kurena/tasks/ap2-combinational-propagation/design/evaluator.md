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
strings (483 pin `function` decls; `LibDb` keeps 448 — the other 460
`function :` grep hits are `power_down_function` cell attributes, not pin
decls): operators `! & | ^ ( )`, `+` (OR, on PG-pin forms like
`!VPWR + VGND`), `*` (AND, e.g. `(CLK*M0)`), constants `"0"`/`"1"`,
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
/// first-use order). `None` on any malformed input and on input over
/// the depth/node/var bounds below.
pub fn parse(text: &str) -> Option<(BoolExpr, Vec<String>)>;

/// Exact `P(expr = 1)` under input independence, `p[i]` = `P(var i = 1)`.
/// Shannon: pick the lowest-indexed free variable `x`, return
/// `p_x·prob(f|x=1) + (1−p_x)·prob(f|x=0)`; cofactors substitute and
/// simplify eagerly, so non-degenerate forms collapse early.
/// `None` when the op bound is exceeded or `p` is short.
pub fn prob(e: &BoolExpr, p: &[f64]) -> Option<f64>;

/// `P(∂f/∂x_i)` — the probability input `i` is sensitized:
/// `prob(cofactor(f,i,false) ⊕ cofactor(f,i,true))`.
pub fn sensitivity(e: &BoolExpr, i: usize, p: &[f64]) -> Option<f64>;

/// 3-valued (Kleene) evaluation: `vals[i]` is var `i`'s constant value,
/// `None` for non-constant inputs. Returns the output's definite value
/// or `None` when it depends on an unknown input. Absorbing-element
/// semantics — `And` is `Some(false)` on any `Some(false)` input,
/// `Or` is `Some(true)` on any `Some(true)`; `Xor`/`Not` have no
/// absorbing element. Used by `constant_cone`'s marking (api.md §5):
/// `or(1,X)=1` and `and(0,X)=0` mark outputs `constant` even with
/// toggling other inputs — the measured rule.
pub fn const_eval(e: &BoolExpr, vals: &[Option<bool>]) -> Option<bool>;
```

## Bounds — the "total and bounded" clause

The evaluator is recursive at two levels, so boundedness needs two
distinct caps (this is the r1 design's gap: `MAX_OPS` bounded work but
not stack):

- `MAX_DEPTH = 64` — parser *recursion* depth. `unary` recurses once
  per `!` and `atom` descends into `expr` once per `(`; a `Parser.depth`
  counter is incremented before each such recursive call (restored on
  exit) and trips `None` past the bound. Nodes are constructed only on
  the way back out of these calls, so a node count alone cannot bound
  this case — `"!" * 100_000 + "A"` would overflow the stack before any
  node exists.
- `MAX_NODES = 2048` — total AST nodes built, bumped at every `BoolExpr`
  construction. AST depth ≤ node count, so this one bound covers the
  iterative `'` postfix chain (`"A" + "'" * 100_000` builds deep trees
  without ever recursing the parser) *and* every recursive consumer:
  `cofactor`, `lowest_var`, and `const_prob` each recurse to AST depth
  (≤ 2048); `sensitivity`'s `Xor(f0,f1)` wrap adds at most one level;
  `prob_rec` recursion depth is additionally ≤ `MAX_VARS` since each
  level fixes one variable.
- `MAX_VARS = 32`, `MAX_OPS = 1_000_000` (op counter inside `prob_rec`)
  as before — `MAX_OPS` bounds Shannon's exponential *work* (a 32-var
  depth-10 expression can still expand toward 2³² leaves); it does not
  bound stack and is not asked to. The budget is a `let mut ops =
  MAX_OPS` **per `prob` call**, and `sensitivity` calls `prob`
  internally — so one gate evaluation can spend up to
  `(1 + n_inputs) × MAX_OPS`. That per-call reset is intended (the bound
  is per-call work, not a shared per-gate budget) and must carry a
  comment at the `let mut ops` site saying so.

`const_eval` recurses to AST depth only and allocates nothing — the
`MAX_NODES` parse cap already bounds it; it needs no counter of its own.

sky130 `function` strings top out at ~10 vars / ~depth 8 / ~60
characters; both caps leave two orders of magnitude of headroom while
keeping worst-case recursion trivially safe for a test-thread stack.
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

### Internal invariants — no silent unreachable paths

`prob`/`prob_rec` only call `const_prob` (var-free evaluation) after
`lowest_var` returned `None`, i.e. on trees containing no `Var` node.
`const_prob`'s `Var` arm is therefore unreachable — encode the invariant
as `debug_assert!(false, "const_prob reached a Var — lowest_var gate")`
with a `0.0` fallback, not a silent `0.0` alone and not a panic. Same
treatment for any internal state that "cannot" happen: `debug_assert`
the invariant in dev, degrade to `None`/`continue` in release. The
module stays panic-free in every build mode.

## Unit tests (in `bool_expr.rs` `#[cfg(test)]`)

- Grammar: each operator, precedence (`!A&B|C` = `((!A)&B)|C`), parens,
  `+`/`*`/`'` alternates, constants, whitespace; malformed forms return
  `None`.
- `prob`: `A&B`→`p·q`; `A|!A`→`1.0` (repeated literal — the case the
  per-operator table gets wrong); `A&A`→`p`; SOP xor `(!A&B)|(A&!B)` at
  `p=.55,q=.8`→`0.47`; mux `(A0&!S)|(A1&S)` cross-check vs. hand truth
  table.
- `sensitivity`: AND `∂/∂a`→`p_b`; XOR→`1.0`; `A&A` `∂/∂a`→`1.0`.
- `const_eval`: `and(1,X)`→`None`, `and(0,X)`→`Some(false)`,
  `or(1,X)`→`Some(true)`, `or(0,X)`→`None`, `xor(c,X)`→`None`,
  `xor(c0,c1)`→`Some(c0^c1)`, `not(Some)`→`Some(!v)`, `not(None)`→`None`;
  multi-level `(A&B)|C` at `A=0` → `Some(false)` on the left arm does
  not decide the `Or` (absorption is per-operator, not per-tree).
- Bounds — each cap must convert a would-be stack overflow into `None`:
  a 40-var expression → `None` (var bound); `"!" * 10_000 + "A"` →
  `None` (`unary` depth); `"(" * 10_000 + "A"` → `None` (`atom`→`expr`
  depth); `"A" + "'" * 10_000` → `None` (node bound — no parser
  recursion is involved here); `"A&" * 5_000 + "A"` → `None` (node
  bound). None of these may abort — that is the spec's "total and
  bounded" acceptance line made executable.

## Files

- `crates/grus-core/src/bool_expr.rs` — parser, AST, `prob`,
  `sensitivity`, `const_eval`, unit tests (new file, the whole module)
- `crates/grus-core/src/lib.rs` — `pub mod bool_expr;` (one line)
- `crates/grus-core/src/activity.rs` — sole consumer: driver-pin
  `function` → `parse` → var→pin map → `prob`/`sensitivity`;
  `const_eval` serves `constant_cone`'s 3-valued marking (api.md §5)
