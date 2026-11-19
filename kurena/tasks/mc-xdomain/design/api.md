# mc-xdomain — Data model & eval changes

Two pieces: the `set_clock_uncertainty -from/-to` inter-clock form in
`sdc.rs`, and the cross-domain candidate evaluation in
`analysis.rs::collect_endpoints` (plus the partitioned skip pass and
the compose pass's MCP rule). Single-domain behaviour is byte-identical
by construction — every new branch keys off a `dom` mask bit that can
only exist with ≥ 2 clock rows.

## 1. `Sdc` — inter-clock uncertainty

```rust
/// One `set_clock_uncertainty -from/-to` rule, in file order. Sides are
/// resolved clock-row lists; an empty list is the wildcard (flag absent).
pub struct InterClockUnc {
    /// `-from` rows (launch side); empty = all clocks.
    pub from: Vec<usize>,
    /// `-to` rows (capture side); empty = all clocks.
    pub to: Vec<usize>,
    /// `-setup` value — `None` when the command gave `-hold`.
    pub setup_ns: Option<f32>,
    /// `-hold` value — `None` when the command gave `-setup`.
    pub hold_ns: Option<f32>,
}

// on Sdc:
/// `set_clock_uncertainty -from/-to` rules, in file order.
pub inter_clock_uncertainty: Vec<InterClockUnc>,
```

Lookup — every eval-side uncertainty read for a launch→capture pair
goes through this; it is the *only* accessor new code calls:

```rust
/// Uncertainty effective for a `launch -> capture` check: the last
/// `-from`/`-to` rule matching both sides supplies that side's value —
/// a **replacement** for the capture row's own field, not additive and
/// not symmetric (measured `t2_uncx`: `-setup 0.30 -from cka -to ckb`
/// replaces `ckb`'s 0.20 for A→B only). No match → the capture row's
/// value. Applies uniformly, same-domain included (`-from cka -to cka`
/// fires on `cka`'s own checks).
pub fn uncertainty_ns(&self, launch: usize, capture: usize, hold: bool) -> f32 {
    for r in self.inter_clock_uncertainty.iter().rev() {
        if (r.from.is_empty() || r.from.contains(&launch))
            && (r.to.is_empty() || r.to.contains(&capture))
            && let Some(v) = if hold { r.hold_ns } else { r.setup_ns }
        {
            return v;
        }
    }
    self.clocks.get(capture).map_or(0.0, |c| {
        if hold { c.hold_uncertainty_ns } else { c.setup_uncertainty_ns }
    })
}
```

Where a call site has a mask rather than one launch row (delay-check
rewrite, latch borrow cap), use the worst-over-bits form — for both
sides the largest matching value is the worst (setup subtracts, hold
adds): `max_{d ∈ mask} uncertainty_ns(d, cap, side)`.

## 2. `set_clock_uncertainty` parse extension

Current grammar: `[-setup|-hold] <ns> [clocks]`. New:

```
set_clock_uncertainty [-setup|-hold] <ns> \
    [-from <clocks>] [-to <clocks>] [<clocks>]
```

- `-from`/`-to` each consume the next arg: `Value::Clocks` (`[get_clocks
  {...}]`, multi-name → cartesian row expansion into `from`/`to`) or a
  bare `Value::Str` name (`write_sdc` form) — both resolved with
  `resolve_clock` (`unknown clock` bail preserved). Other value kinds
  bail `"-from/-to take clock objects"`.
- Duplicate `-from`/`-to` → `"takes each of -from/-to once"` (same
  wording as the exception commands).
- Other dash options (`-rise_from`, `-fall_from`, `-rise_to`,
  `-fall_to`, `-rise`, `-fall`) → `"unsupported option \`X\`"` — same
  loud surface as `set_clock_latency -early/-late`.
- `-from`/`-to` combined with a positional clock list → bail
  `"cannot name clocks with -from/-to"` (OpenSTA treats the positional
  list and `-to` as overlapping — we refuse the ambiguous form loudly).
- `-setup`/`-hold` keep mutual exclusivity; neither flag → both
  `setup_ns` and `hold_ns` are stored (`Some(v)`).
- One-sided forms are legal: `-from cka` alone means that launch clock
  against every capture (`to` wildcard), and vice versa.
- A rule stores the resolved rows at parse time — clocks named must
  already exist (same `resolve_clock` discipline as every other
  clock-targeting command).

The `multi_clock_bails` case
`"set_clock_uncertainty -setup 0.30 -from [get_clocks core_clk] -to [get_clocks vclk]"`
moves out of the bail table — it parses now. Replacement positive tests
in `tests.md` §3.

## 3. `collect_endpoints` — candidate-loop restructure

`foreign_launch` is replaced by a whole-endpoint deferral predicate
(shared with the partitioned skip pass):

```rust
/// `Some(launch names)` when any reached arrival extreme cannot be
/// evaluated against `cap_tag` — the whole endpoint then defers
/// `CrossDomain` (both max and min sides are needed to report).
///   * reached extreme, empty dom mask      -> "<undeclared>"
///   * latch-D check, any foreign bit       -> parked borrow pairing
///   * min-side (d, g) with no coincident
///     capture edge in the LCM window       -> parked hold selection
///   * non-converging `grid_lcm`            -> defensive
/// `None` = every tying (domain, parity) pairs — evaluate.
fn xdomain_defer(
    s: PinTimingState, sdc: &Sdc, cap_tag: u8, cap_period: f32,
    c0: usize, latch: bool,
) -> Option<String>;
```

Then, per `(data, check)` in the existing loop, after the `NoClock`
gate and `cap`/`cap_clk` reads:

- `cap_bit = dom_bit(cap)`; `if let Some(launch) = xdomain_defer(s, sdc,
  cap_bit, cap_clk.period_ns, c0, check.latch) { cross_domain += 1;
  skipped_endpoints.push(... "launch {launch} -> capture {cap}") ;
  continue; }`
- Max side: the `for g` parity loop becomes a `for d in
  bits(dom_max[pol])` × `for g in bits(par_max[pol] or {0})` loop:
  - `d == cap`: **existing code verbatim** (`capture_extra`, `c0 <= g`,
    `credit_for`, latch borrow branch) — the only change is the
    uncertainty read: `cap_clk.setup_uncertainty_ns` →
    `sdc.uncertainty_ns(d, cap, false)` (identical value on single-clock
    SDCs).
  - `d != cap`: `let (lsh, csh) = setup_edge_pair(pl_d, pc, g, c0)?`
    (`None` unreachable — `xdomain_defer` pre-checked);
    `arr = arrival_max[pol] + lsh`;
    `req = clk_state.arrival_min[edge] + csh − late_time −
    uncertainty_ns(d, cap, false)`; `slack = req − arr`.
    **No `credit_for`** on the foreign branch — a foreign launch never
    shares the capture clock net, so the credit is 0 by construction
    (`crpr_credit` would return 0 anyway).
- Min side, same loop shape over `(d, g)` in
  `dom_min` × `par_min`:
  - `d == cap`: existing verbatim (`req_min`, `c0 > g` realignment),
    `cap_clk.hold_uncertainty_ns` → `sdc.uncertainty_ns(d, cap, true)`.
  - `d != cap`: `hold_edge_pair` (`None` unreachable);
    `arr = arrival_min[pol] + lsh`;
    `req = clk_state.arrival_max[edge] + csh + early_time +
    uncertainty_ns(d, cap, true)`.
- `Worst` now carries a candidate-specific `arr` (the `+ m·PL` shift
  lives there); `slack` picks stay "worst across candidates".
- `dly_*`/`slew_*` copied through unchanged — the delay arrival is
  edge-0-referenced regardless of the paired edge (`pairing.md` §7).

Cross-product approximation (recorded): `dom_*` and `par_*` masks union
independently, so a tied extreme can enumerate (d, g) pairs no single
launch produced — a max side may evaluate a slightly pessimistic pair,
a min side may defer on a phantom non-coincident combo. Single-bit
masks (every committed fixture) are exact.

Latch checks: `check.latch` with foreign bits is caught by
`xdomain_defer` before the loop — the borrow/opening-edge branch never
runs cross-domain (parked; `docs/multi-clock.md`).

## 4. Output ports and gating enables

**Output port** (`cap` = `launch_domain(sdc, out_row)` tag; the anchor
row's own period/grid, virtual rows included): restructure the single
`req` computation into the same per-(d, g) candidate loop with
`phase_c = 0`:

- `d == cap_tag`: existing `req_max = latency + period − delay_max −
  uncertainty_ns(d, out_row)` etc.
- `d != cap_tag`: `setup_edge_pair(pl_d, outP, g, 0)` → `req_max =
  latency + csh − delay_max − uncertainty_ns(d, out_row, false)`;
  `hold_edge_pair` → `req_min = latency + csh − delay_min +
  uncertainty_ns(d, out_row, true)`.

Unexercised in the fixtures (all output anchors are same-domain) —
spec'd by the uniform rule, noted in `docs/results.md` as unevaluated.

**Gating enables**: same restructure — setup pairs on the `setup_edge`
grid (`phase_c = pe·PC/2`), the gate-hold side on the `hold_edge` grid
using the same "smallest edge strictly after launch" helper semantics
as `setup_edge_pair` (the gate's hold edge *follows* the launch edge —
not coincident). Unexercised; noted. On `None` from either helper the
enable defers `CrossDomain`.

## 5. Site table — uncertainty and pairing reads

| site | now | becomes |
|---|---|---|
| check closing req | `cap_clk.setup_uncertainty_ns` | `uncertainty_ns(d, cap, false)` per candidate |
| latch opening req (same-domain only) | `cap_clk.setup_uncertainty_ns` | same accessor form |
| check hold req | `cap_clk.hold_uncertainty_ns` | `uncertainty_ns(d, cap, true)` |
| gate req | `cap_clk.{setup,hold}_uncertainty_ns` | per-candidate accessor |
| output-port req | `out_clk.{setup,hold}_uncertainty_ns` | `uncertainty_ns(d, out_row, side)` |
| delay-check rewrite (compose) | `cap_clk.{setup,hold}_uncertainty_ns` | worst-over-`dom_*`-bits accessor |
| latch borrow cap (`eval_sta`) | `en_clk.setup_uncertainty_ns` | unchanged — borrow pairing is parked; only same-domain data reaches the cap in fixtures |
| check req edge selection | `c0 <= g → +period` | `d == cap`: unchanged; `d != cap`: `setup_edge_pair`/`hold_edge_pair` |
| `foreign_launch` gate | whole-endpoint defer on any foreign bit | `xdomain_defer` — only unpairable cases defer |
| compose MCP `d_min` | `((ns−1)−nh)·cap.period` | `−nh·cap.period` when the candidate's `dom_min` mask has a foreign bit (`pairing.md` §6) |
| compose MCP `d_max` | `(ns−1)·cap.period` | unchanged — applies on the paired req |

## 6. Partitioned skip pass

The `xdomain_skip` closure in `run_sta_partitioned` switches from
`foreign_launch` to `xdomain_defer` — same arguments, plus `c0` (from
`prepared.clock_parity`, the same parity decode `edge0_pol` uses) and
`check.latch` for `EndpointKind::Check` (`false` for gates/ports).
Ordering unchanged: `NoClock` → `xdomain_defer` → `endpoint_skip_reason`.
Endpoints that *did* evaluate are `reported` before the pass runs, so
the re-diagnosis only sees genuinely unpairable pins — the closure's
semantics stay "why is this endpoint unreported".

## 7. Signatures touched

- `Sdc::inter_clock_uncertainty` field; `InterClockUnc` type;
  `Sdc::uncertainty_ns`.
- `set_clock_uncertainty` arg walk — index loop instead of the flag
  filter, to consume `-from`/`-to` arguments.
- `analysis.rs`: `grid_lcm`, `setup_edge_pair`, `hold_edge_pair`,
  `xdomain_defer` (new, `foreign_launch` deleted — its
  `<undeclared>`/name-join logic moves into `xdomain_defer`).
- `collect_endpoints`: candidate loops per §3–§4 — no signature change
  (all inputs already in scope: `clock_domain`, `clock_parity` for `c0`).
- `run_sta_partitioned`: `xdomain_skip` closure body; the MCP `d_min`
  site reads `class_states[ci][ep].dom_min[pol]` for the foreign-bit
  test.
- No changes to `PinTimingState`, `StaPrepared`, report types,
  `eval_sta`'s signature, or `main.rs`.

## 8. Remaining `CrossDomain` surface — summary

| case | outcome |
|---|---|
| reached extreme, `dom` mask = 0 | `CrossDomain` (`launch <undeclared>`) |
| latch-D check, foreign launch | `CrossDomain` (parked borrow pairing) |
| min-side parity, no coincident capture edge | `CrossDomain` (parked hold selection) |
| `grid_lcm` non-convergence | `CrossDomain` (defensive) |
| everything else | evaluated per `pairing.md` |

The `--strict`/`is_clean`/`cross_domain_endpoints` accounting is
unchanged — the counter just no longer fires on the committed
scenarios.

## Files

- `crates/grus-core/src/sta/sdc.rs`
- `crates/grus-core/src/sta/analysis.rs`
