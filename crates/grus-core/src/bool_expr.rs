//! Liberty `function` expression evaluator for activity propagation.
//!
//! Computes, under input independence, the exact `P(f = 1)` of a gate
//! function and each input's sensitization probability `P(∂f/∂x_i)` —
//! the measured M1 local-marginal model (docs/results.md 2026-11-21 N7).
//! Evaluation is Shannon expansion with eager constant folding: exact for
//! arbitrary expressions (repeated literals included — `"(!A&B)|(A&!B)"`
//! is sky130's literal `xor2` form), and identical to the per-operator
//! product rules on the once-per-variable forms.
//!
//! Everything is total: malformed input parses `None`, and a node-eval
//! budget plus a variable bound keep pathological strings from blowing
//! up the recursion. Consumers keep the `Default` fill on `None`.

/// Grammar (`! > & > ^ > |`, left-associative; `+` is OR, `*` is AND,
/// `'` postfix is NOT — the sky130 `function` alphabet plus the Liberty
/// alternates):
///
/// ```text
/// expr    := or
/// or      := xor (('|' | '+') xor)*
/// xor     := and ('^' and)*
/// and     := unary (('&' | '*') unary)*
/// unary   := '!' unary | postfix
/// postfix := atom '\''*
/// atom    := '(' expr ')' | ident | '0' | '1'
/// ident   := [A-Za-z_][A-Za-z0-9_.$]*
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum BoolExpr {
    Const(bool),
    /// Index into the caller's input list (first-use order of `parse`'s
    /// returned var names).
    Var(u16),
    Not(Box<BoolExpr>),
    And(Box<BoolExpr>, Box<BoolExpr>),
    Or(Box<BoolExpr>, Box<BoolExpr>),
    Xor(Box<BoolExpr>, Box<BoolExpr>),
}

/// Distinct variables a parsed expression may name.
pub const MAX_VARS: usize = 32;
/// Total node evaluations allowed across one `prob`/`sensitivity` call —
/// the Shannon recursion is exponential in the worst case; the bound only
/// exists so a pathological string cannot hang the caller.
const MAX_OPS: usize = 1_000_000;

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    vars: Vec<String>,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.bytes.get(self.pos).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn ident_var(&mut self, name: &str) -> u16 {
        if let Some(i) = self.vars.iter().position(|v| v == name) {
            return i as u16;
        }
        self.vars.push(name.to_string());
        (self.vars.len() - 1) as u16
    }

    fn atom(&mut self) -> Option<BoolExpr> {
        match self.peek()? {
            b'(' => {
                self.pos += 1;
                let e = self.expr()?;
                self.eat(b')').then_some(e)
            }
            b'0' | b'1' => {
                let c = self.bytes[self.pos];
                self.pos += 1;
                // A digit must not open a longer identifier-shaped token.
                if self
                    .bytes
                    .get(self.pos)
                    .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
                {
                    return None;
                }
                Some(BoolExpr::Const(c == b'1'))
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let start = self.pos;
                while self.pos < self.bytes.len()
                    && matches!(
                        self.bytes[self.pos],
                        b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'.' | b'$'
                    )
                {
                    self.pos += 1;
                }
                let name = std::str::from_utf8(&self.bytes[start..self.pos]).ok()?;
                Some(BoolExpr::Var(self.ident_var(name)))
            }
            _ => None,
        }
    }

    fn unary(&mut self) -> Option<BoolExpr> {
        if self.eat(b'!') {
            return Some(BoolExpr::Not(Box::new(self.unary()?)));
        }
        let mut e = self.atom()?;
        while self.eat(b'\'') {
            e = BoolExpr::Not(Box::new(e));
        }
        Some(e)
    }

    fn and(&mut self) -> Option<BoolExpr> {
        let mut e = self.unary()?;
        while matches!(self.peek(), Some(b'&' | b'*')) {
            self.pos += 1;
            e = BoolExpr::And(Box::new(e), Box::new(self.unary()?));
        }
        Some(e)
    }

    fn xor(&mut self) -> Option<BoolExpr> {
        let mut e = self.and()?;
        while self.eat(b'^') {
            e = BoolExpr::Xor(Box::new(e), Box::new(self.and()?));
        }
        Some(e)
    }

    fn expr(&mut self) -> Option<BoolExpr> {
        let mut e = self.xor()?;
        while matches!(self.peek(), Some(b'|' | b'+')) {
            self.pos += 1;
            e = BoolExpr::Or(Box::new(e), Box::new(self.xor()?));
        }
        Some(e)
    }
}

/// Parses a Liberty `function` string; `vars` names each distinct
/// identifier in first-use order (each maps to one `Var` index). `None`
/// on any malformed input — empty, trailing garbage, unbalanced parens,
/// unknown tokens — and on input over the variable bound.
pub fn parse(text: &str) -> Option<(BoolExpr, Vec<String>)> {
    let mut p = Parser {
        bytes: text.as_bytes(),
        pos: 0,
        vars: Vec::new(),
    };
    let e = p.expr()?;
    if p.peek().is_some() || p.vars.len() > MAX_VARS {
        return None;
    }
    Some((e, p.vars))
}

/// Substitutes `Const(value)` for variable `v`, folding eagerly so
/// degenerate forms collapse (`A|!A` cofactored is `1|!1` → `1`).
fn cofactor(e: &BoolExpr, v: u16, value: bool) -> BoolExpr {
    match e {
        BoolExpr::Const(c) => BoolExpr::Const(*c),
        BoolExpr::Var(x) if *x == v => BoolExpr::Const(value),
        BoolExpr::Var(x) => BoolExpr::Var(*x),
        BoolExpr::Not(f) => match cofactor(f, v, value) {
            BoolExpr::Const(c) => BoolExpr::Const(!c),
            BoolExpr::Not(g) => *g,
            f => BoolExpr::Not(Box::new(f)),
        },
        BoolExpr::And(a, b) => fold(
            cofactor(a, v, value),
            cofactor(b, v, value),
            BoolExpr::And,
            |a, b| a && b,
            false,
        ),
        BoolExpr::Or(a, b) => fold(
            cofactor(a, v, value),
            cofactor(b, v, value),
            BoolExpr::Or,
            |a, b| a || b,
            true,
        ),
        BoolExpr::Xor(a, b) => match (cofactor(a, v, value), cofactor(b, v, value)) {
            (BoolExpr::Const(a), BoolExpr::Const(b)) => BoolExpr::Const(a != b),
            (BoolExpr::Const(false), x) | (x, BoolExpr::Const(false)) => x,
            (BoolExpr::Const(true), x) | (x, BoolExpr::Const(true)) => BoolExpr::Not(Box::new(x)),
            (a, b) if a == b => BoolExpr::Const(false),
            (a, b) => BoolExpr::Xor(Box::new(a), Box::new(b)),
        },
    }
}

/// And/Or folding: both-const → eval, the absorbing const wins, the
/// neutral const drops, `x op x` → `x`.
fn fold(
    a: BoolExpr,
    b: BoolExpr,
    make: fn(Box<BoolExpr>, Box<BoolExpr>) -> BoolExpr,
    op: fn(bool, bool) -> bool,
    absorbing: bool,
) -> BoolExpr {
    match (a, b) {
        (BoolExpr::Const(a), BoolExpr::Const(b)) => BoolExpr::Const(op(a, b)),
        (BoolExpr::Const(c), x) | (x, BoolExpr::Const(c)) => {
            if c == absorbing {
                BoolExpr::Const(c)
            } else {
                x
            }
        }
        (a, b) if a == b => a,
        (a, b) => make(Box::new(a), Box::new(b)),
    }
}

/// The lowest-indexed free variable, `None` on a var-free expression.
fn lowest_var(e: &BoolExpr) -> Option<u16> {
    match e {
        BoolExpr::Const(_) => None,
        BoolExpr::Var(x) => Some(*x),
        BoolExpr::Not(f) => lowest_var(f),
        BoolExpr::And(a, b) | BoolExpr::Or(a, b) | BoolExpr::Xor(a, b) => {
            match (lowest_var(a), lowest_var(b)) {
                (Some(x), Some(y)) => Some(x.min(y)),
                (x, y) => x.or(y),
            }
        }
    }
}

fn prob_rec(e: &BoolExpr, p: &[f64], ops: &mut usize) -> Option<f64> {
    *ops = ops.checked_sub(1)?;
    let v = lowest_var(e)? as usize;
    let px = *p.get(v)?;
    let f1 = cofactor(e, v as u16, true);
    let f0 = cofactor(e, v as u16, false);
    // A cofactored side may still be trivially var-free after folding.
    let p1 = match lowest_var(&f1) {
        None => const_prob(&f1),
        Some(_) => prob_rec(&f1, p, ops)?,
    };
    let p0 = match lowest_var(&f0) {
        None => const_prob(&f0),
        Some(_) => prob_rec(&f0, p, ops)?,
    };
    Some(px * p1 + (1.0 - px) * p0)
}

/// Var-free evaluation; `lowest_var` returned `None` so every node is a
/// `Const` under the folding rules — any leftover structure is an error.
fn const_prob(e: &BoolExpr) -> f64 {
    match e {
        BoolExpr::Const(c) => *c as u8 as f64,
        BoolExpr::Var(_) => 0.0, // unreachable under the lowest_var gate
        BoolExpr::Not(f) => 1.0 - const_prob(f),
        BoolExpr::And(a, b) => const_prob(a) * const_prob(b),
        BoolExpr::Or(a, b) => 1.0 - (1.0 - const_prob(a)) * (1.0 - const_prob(b)),
        BoolExpr::Xor(a, b) => {
            let (a, b) = (const_prob(a), const_prob(b));
            a + b - 2.0 * a * b
        }
    }
}

/// Exact `P(expr = 1)` under input independence, `p[i]` = `P(var i = 1)`.
/// A var-free expression reads `0.0`/`1.0` directly. `None` when the op
/// budget is exceeded or `p` is short.
pub fn prob(e: &BoolExpr, p: &[f64]) -> Option<f64> {
    let mut ops = MAX_OPS;
    match lowest_var(e) {
        None => Some(const_prob(e)),
        Some(_) => prob_rec(e, p, &mut ops),
    }
}

/// `P(∂f/∂x_i)` — the probability input `i` is sensitized:
/// `prob(f(x_i=0) ⊕ f(x_i=1))`. `0.0` when `f` does not reference `i`.
pub fn sensitivity(e: &BoolExpr, i: usize, p: &[f64]) -> Option<f64> {
    let v = i as u16;
    let f0 = cofactor(e, v, false);
    let f1 = cofactor(e, v, true);
    let diff = match (f0, f1) {
        (BoolExpr::Const(a), BoolExpr::Const(b)) => BoolExpr::Const(a != b),
        (a, b) if a == b => BoolExpr::Const(false),
        (a, b) => BoolExpr::Xor(Box::new(a), Box::new(b)),
    };
    prob(&diff, p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(text: &str) -> (BoolExpr, Vec<String>) {
        parse(text).unwrap_or_else(|| panic!("`{text}` should parse"))
    }

    #[test]
    fn test_grammar_operators_and_precedence() {
        let (e, vars) = parse_ok("!A&B|C");
        assert_eq!(
            e,
            BoolExpr::Or(
                Box::new(BoolExpr::And(
                    Box::new(BoolExpr::Not(Box::new(BoolExpr::Var(0)))),
                    Box::new(BoolExpr::Var(1))
                )),
                Box::new(BoolExpr::Var(2))
            )
        );
        assert_eq!(vars, ["A", "B", "C"]);

        // XOR binds tighter than OR, looser than AND: `A^B|C&D` = (A^B)|(C&D).
        let (e, _) = parse_ok("A^B|C&D");
        assert!(matches!(e, BoolExpr::Or(_, _)));
        // First-use order is the var order.
        let (_, vars) = parse_ok("(A0&!S)|(A1&S)");
        assert_eq!(vars, ["A0", "S", "A1"]);
        // Alternates: `+` is OR, `*` is AND, `'` postfix is NOT.
        let (e, vars) = parse_ok("!VPWR + VGND");
        assert!(matches!(e, BoolExpr::Or(_, _)));
        assert_eq!(vars, ["VPWR", "VGND"]);
        let (e, _) = parse_ok("(CLK*M0)");
        assert!(matches!(e, BoolExpr::And(_, _)));
        let (e, _) = parse_ok("A'");
        assert!(matches!(e, BoolExpr::Not(_)));
        // Constants and whitespace.
        assert_eq!(parse_ok(" 0 ").0, BoolExpr::Const(false));
        assert_eq!(parse_ok(" ( 1 ) ").0, BoolExpr::Const(true));
    }

    #[test]
    fn test_grammar_rejects_malformed() {
        for bad in [
            "", " ", "(", ")", "(A", "A)", "A B", "&A", "A&", "A|", "!|A", "A!B",
            "A'B'", "01", "( )", "A#B",
        ] {
            assert!(parse(bad).is_none(), "`{bad}` must not parse");
        }
    }

    #[test]
    fn test_prob_forms() {
        let (e, _) = parse_ok("A&B");
        assert!((prob(&e, &[0.55, 0.8]).unwrap() - 0.44).abs() < 1e-12);
        let (e, _) = parse_ok("A|B");
        assert!((prob(&e, &[0.55, 0.8]).unwrap() - 0.91).abs() < 1e-12);
        // Repeated literals: the per-operator product rules get these wrong.
        let (e, _) = parse_ok("A|!A");
        assert_eq!(prob(&e, &[0.3]), Some(1.0));
        let (e, _) = parse_ok("A&!A");
        assert_eq!(prob(&e, &[0.3]), Some(0.0));
        let (e, _) = parse_ok("A&A");
        assert_eq!(prob(&e, &[0.3]), Some(0.3));
        // sky130's literal xor2/xnor2 SOP forms.
        let (e, _) = parse_ok("(!A&B) | (A&!B)");
        assert!((prob(&e, &[0.55, 0.8]).unwrap() - 0.47).abs() < 1e-12);
        let (e, _) = parse_ok("(!A&!B) | (A&B)");
        assert!((prob(&e, &[0.55, 0.8]).unwrap() - 0.53).abs() < 1e-12);
        // Mux `(A0&!S)|(A1&S)` vs a hand truth table at A0=.7 A1=.4 S=.6:
        // P = 0.7*0.4 + 0.4*0.6 = 0.52 (the terms are disjoint).
        let (e, _) = parse_ok("(A0&!S)|(A1&S)");
        assert!((prob(&e, &[0.7, 0.6, 0.4]).unwrap() - 0.52).abs() < 1e-12);
        // Constants.
        let (e, _) = parse_ok("0");
        assert_eq!(prob(&e, &[]), Some(0.0));
        let (e, _) = parse_ok("1");
        assert_eq!(prob(&e, &[]), Some(1.0));
    }

    #[test]
    fn test_sensitivity() {
        // AND: ∂f/∂a = b, ∂f/∂b = a.
        let (e, _) = parse_ok("A&B");
        assert_eq!(sensitivity(&e, 0, &[0.55, 0.8]), Some(0.8));
        assert_eq!(sensitivity(&e, 1, &[0.55, 0.8]), Some(0.55));
        // XOR: every input edge toggles the output.
        let (e, _) = parse_ok("A^B");
        assert_eq!(sensitivity(&e, 0, &[0.55, 0.8]), Some(1.0));
        assert_eq!(sensitivity(&e, 1, &[0.55, 0.8]), Some(1.0));
        // `A&A`: ∂f/∂a = 0⊕1.
        let (e, _) = parse_ok("A&A");
        assert_eq!(sensitivity(&e, 0, &[0.3]), Some(1.0));
        // Unreferenced input is never sensitized.
        let (e, _) = parse_ok("A");
        assert_eq!(sensitivity(&e, 1, &[0.3, 0.9]), Some(0.0));
        // Mux: ∂f/∂s = A0⊕A1 → P = .7+.4-2*.28 = 0.54.
        let (e, _) = parse_ok("(A0&!S)|(A1&S)");
        assert!((sensitivity(&e, 1, &[0.7, 0.6, 0.4]).unwrap() - 0.54).abs() < 1e-12);
    }

    #[test]
    fn test_bounds() {
        // Over the variable bound: no hang, no panic.
        let text = (0..40)
            .map(|i| format!("x{i}"))
            .collect::<Vec<_>>()
            .join("&");
        assert!(parse(&text).is_none());
        // Short `p` slice is a caller bug we answer with `None`.
        let (e, _) = parse_ok("A&B");
        assert_eq!(prob(&e, &[0.5]), None);
    }

    /// sky130 `function` coverage: every `LibPin.function` string in
    /// `sky130_fd_sc_hd__tt_025C_1v80.lib` must parse; variables naming no
    /// pin of their own cell (PG forms like `!VPWR+VGND`, sequential `IQ`
    /// nodes) are the only recorded miss — the propagation pass counts
    /// them `unresolvable_functions`.
    #[test]
    fn test_sky130_function_coverage() {
        let hd = crate::test_support::sky130_hd();
        let lib_path =
            crate::test_support::require(hd.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib"));
        let lib = crate::readers::load_liberty(&lib_path).expect("liberty");
        let mut decls = 0usize;
        let mut unparseable = 0usize;
        let mut funcs_with_unmapped_vars = 0usize;
        for cell in &lib.cells {
            let pins = lib.cell_pins(cell);
            for pin in pins {
                let Some(func) = pin.function else {
                    continue;
                };
                decls += 1;
                match parse(lib.strings.resolve(func)) {
                    Some((_e, vars)) => {
                        if vars.iter().any(|v| {
                            let Some(sym) = lib.strings.get(v) else {
                                return true;
                            };
                            !pins.iter().any(|p| p.name == sym)
                        }) {
                            funcs_with_unmapped_vars += 1;
                        }
                    }
                    None => unparseable += 1,
                }
            }
        }
        eprintln!(
            "sky130 function decls {decls}: unparseable {unparseable}, \
             unmapped-var {funcs_with_unmapped_vars}"
        );
        // The raw .lib holds 483 `function :` lines; `LibDb` keeps 448 of
        // them (pins the Liberty reader does not model are dropped), and
        // every kept decl parses with vars resolving to sibling pins.
        assert_eq!(unparseable, 0, "sky130 function decls must all parse");
        assert_eq!((decls, funcs_with_unmapped_vars), (448, 0));
    }
}
