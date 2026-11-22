# mc-xdomain — Tests & fail-proof

All comparisons at `Tol::STRICT` (1% rel / 5 ps), per endpoint and per
field (`arr`/`req`/`slack`/`slew` both sides), plus `cross_domain ==
expected`. On every committed `sta_clocks` scenario the expected count
is 0 — so `wns`/`tns`/`setup_violations`/`hold_violations` compare too
(the existing `cross_domain == 0` aggregate gate strengthens for free).

## 1. Golden-matrix updates (`test_sta_clocks_match_opensta`)

- `parse_gated` → `["t2_grp"]` (`t2_uncx` parses now): 15 `t2_*`
  scenarios on `two_clock.def` (14 evaluated + `t2_grp` parse-gated)
  plus `t2_latch` on `two_clock_latch.def`.
- The per-scenario `cross_domain` expectation stays an explicit number
  — now 0 everywhere on this design.
- No new assertions needed on the covered endpoints: the existing
  per-endpoint/per-field loop picks up `u_fx/D`, `u_fy/D`, `u_fb0/D`
  automatically once they report. Spot-check `t2_uncx` direction
  asymmetry in the diff review (A→B req 3.785446, B→A keeps 9.835446).
- `t2_mdx` is pinned twice for free: `u_fy/D`'s segment check
  (`arr 0.0` both sides, `req 0.885446`/`0.962415`) and the
  `pin_from_sig` kill — `u_fb1/D` is absent from the golden, so the
  existing "reported but absent from OpenSTA" check fails if we still
  report its clocked check.
- Rename `sta_evaluates_two_clock_same_domain` →
  `sta_evaluates_two_clock` and update its docstring — the test has
  asserted the cross-domain min-gap pairs (`u_fx/D` 0.885446,
  `u_fy/D` 1.885446) since this ticket, so `same_domain` is stale.

## 2. Unit tests — pairing helpers (`analysis.rs` `tests` module)

Pure functions, no fixture needed:

| case | expected |
|---|---|
| `grid_lcm(2.0, 1.4)` | `Some(14.0)` |
| `grid_lcm(2.0, 1.0)` | `Some(2.0)` |
| `setup_edge_pair(2.0, 1.4, 0, 0)` | `(4.0, 4.2)` |
| `setup_edge_pair(1.4, 2.0, 0, 0)` | `(9.8, 10.0)` |
| `setup_edge_pair(1.0, 2.0, 0, 0)` | `(1.0, 2.0)` — equal-grid min-gap, not edge 0 |
| `setup_edge_pair(2.0, 1.0, 0, 0)` | `(0.0, 1.0)` |
| `setup_edge_pair(2.0, 2.0, 0, 0)` | `(0.0, 2.0)` — same-domain degeneration |
| `setup_edge_pair(2.0, 2.0, 1, 0)` | `(0.0, 2.0)` — `c0 <= g` parity case; returns additive shifts, and the g=1 first edge is already inside the recorded arrival (the pair's absolute edge times are 1.0/2.0) |
| `hold_edge_pair(2.0, 1.4, 0, 0)` | `(0.0, 0.0)` |
| `hold_edge_pair(2.0, 1.4, 1, 0)` | `(6.0, 7.0)` — first coincidence at T=7.0 |
| `hold_edge_pair(1.4, 2.0, 1, 0)` | `None` — no coincident capture edge (defer path) |

Same module, `pin_segment_arrival` (`api.md` §5.4) — synthetic
`PinTimingState`s (`PinTimingState::UNREACHED` plus states with set
`dly_*`):

| case | expected |
|---|---|
| `{reached, unreached}`, max side | `ep_dly − reached.dly_max[pol]` — the unreached pin is skipped, no `+∞` term |
| `{unreached}` only, max side | `None` — the polarity's candidate drops |
| `{reached, unreached}`, min side | `ep_dly − reached.dly_min[pol]` — no `−∞` term |
| `{unreached}` only, min side | `None` |

## 3. `sdc.rs` tests

- Remove the `set_clock_uncertainty ... -from/-to` case from
  `multi_clock_bails` (it parses now).
- Add `set_clock_uncertainty_directional` (or extend an existing parse
  test table): after `create_clock cka`/`ckb`/`vclk`:
  - `-setup 0.30 -from [get_clocks cka] -to [get_clocks ckb]` → one
    `InterClockUnc { from:[cka], to:[ckb], setup:Some(0.30),
    hold:None }`.
  - `-hold 0.15 -from [get_clocks ckb] -to [get_clocks cka]` → hold
    side stored.
  - Bare flag-less form sets both `setup_ns`/`hold_ns`.
  - `-from` alone → `to` wildcard; `-to` alone → `from` wildcard.
  - Multi-name `[get_clocks {cka ckb}]` expands both rows.
  - Bare `Str` clock name accepted (write_sdc form).
  - Unknown clock name bails `unknown clock`.
  - Positional clocks + `-from` bails `cannot name clocks with
    -from/-to`; `-rise_from` bails `unsupported option`.

## 4. `uncertainty_ns` accessor tests

Two-row clock table + rules; assert:

- `(cka→ckb, setup)` returns the rule value, `(ckb→cka, setup)` and
  both hold sides return row values — replacement, not symmetric.
- `(cka→ckb, hold)` unaffected by a `-setup` rule — sides independent.
- Later rule overrides earlier for the same pair (file order, `.rev()`
  walk).
- Wildcard `-to` matches every capture; no rules → row value.

## 5. Golden-discrimination coverage (what actually goes red)

| expectation | breaks if |
|---|---|
| `t2_frac u_fx/D arr 4.277469` / `req 4.085446` | launch/capture shifts wrong, pairing skipped, `d_in` anchor |
| `t2_int u_fy/D arr 1.277469` / `req 1.885446` | edge-0 pair instead of min-gap (fold regression) |
| `t2_uncx u_fx/D req 3.785446` vs `u_fy/D 9.835446` | rule applied symmetrically, additively, or to hold |
| `t2_md* u_fx/D req 0.935446`, `u_fy/D min_req 1.162415` | delay-check endpoint still deferring, wrong anchor |
| `t2_mdx u_fy/D arr 0.0` / `req 0.885446/0.962415`; `u_fb1/D` absent | pin `-from` treated as launch selector, segment/req formula wrong, `pin_from_sig` kill missing, unreached pin producing ±∞ |
| `t2_mcp u_fx/D min_req −1.037585` | same-domain `((ns−1)−nh)·P` applied to a foreign candidate |
| every scenario `cross_domain == 0` | a `CrossDomain` skip slips through |
| single-clock designs (`gcd`, `ibex`, `aes`, `picorv32`, `sta_exceptions`, `sta_edge`, `jpeg`, …) | any golden byte change — the dom-mask branch must not fire |

## 6. Fail-proof mutation (recorded in `docs/results.md`)

Force `setup_edge_pair` to return the first pair (`m = 0` → capture
after edge 0) instead of the min-gap pair:

- expected red: `t2_int` (`u_fy/D` arr 0.277469 vs 1.277469, req
  0.885446 vs 1.885446) and `t2_frac` (`u_fx/D` arr 0.277469 vs
  4.277469) in `test_sta_clocks_match_opensta`.
- restore; confirm green; record both runs (per `AGENTS.md` §7 rule 7).

A second mutation worth noting (optional, one suffices): force
`uncertainty_ns` to skip the rule table → `t2_uncx` `u_fx/D` req reads
`ckb`'s own 0.20 → 3.885446 vs golden 3.785446.

## 7. Verification commands

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Plus the regen discipline in `fixture.md` §3 (`git diff` clean on
untouched artifacts) and the fail-proof runs above — all recorded in
`docs/results.md`.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/sdc.rs`
- `tests/designs/sta_clocks/t2_mdx.sdc`
- `tests/designs/sta_clocks/probe.sh`
- `tests/designs/sta_clocks/two_clock.probe.json`
- `tests/designs/sta_clocks/probe_t2_mdx.txt`
- `docs/results.md`
