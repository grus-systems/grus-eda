# mc-xdomain — Edge-pair algorithms

The measured rule (`docs/multi-clock.md` §1.2): for a launch domain ≠
capture domain check, OpenSTA enumerates (launch edge, capture edge)
pairs — each launch edge with the smallest capture-domain edge strictly
after it — over the period-LCM window, and reports the pair minimising
`capture − launch`. Hold pairs stay on coincident edges.

## 1. Source-grid model

Pairing works on **source-grid edge times** — the `create_clock`
lattice, latency-free. A clock with period `P` has edges at `k·P/2`;
edges of index parity `p` (`p ∈ {0,1}`) sit at `p·P/2 + m·P`
(`m = 0,1,…`).

- **Launch parity `g`** comes from `PinTimingState::par_max[pol]` /
  `par_min[pol]` (bit `g` = a launch on clock-edge index parity `g`;
  mask `0` falls back to `g = 0`, the existing convention).
- **Capture phase `c0`** comes from `edge_parity(clk, check.edge)` —
  the source edge-index parity of the capture pin's first
  `check.edge`-polarity edge. The pin's `check.edge` edges map to
  source indices `c0, c0+2, c0+4, …` → times `c0·PC/2 + j·PC`.
- For output ports there is no clock pin; the anchor clock's rise-edge
  grid is used (`phase_c = 0`), which degenerates to today's
  `req = latency + period − delay` for a launch at edge 0.

An arrival extreme's recorded value `arr₀` is referenced to the first
edge of its parity at the launch flop's clock pin; a launch on source
edge `g·PL/2 + m·PL` reports `arr = arr₀ + m·PL` (data delay constant
across edges, ideal or propagated). The capture pin's
`clk_state.arrival_*[check.edge]` is the first such arrival; the `j`-th
adds `j·PC`. Latency never enters edge selection — it is already inside
the recorded arrivals (verified: `t2_lat` pairs identically to
`t2_frac` despite per-domain latencies).

## 2. Grid helpers (`analysis.rs`, `f64` internally)

```rust
/// `lcm(a, b)` of two clock periods via f64 Euclid with a bounded
/// iteration count and a relative remainder test (~1e-9). `None` when
/// the gcd does not converge — unreachable for f32 periods (always
/// rational), so a miss is a defensive bail, not a model case.
fn grid_lcm(a: f32, b: f32) -> Option<f64>;

/// §1.2 setup pair: every launch edge `g·pl/2 + m·pl` inside the LCM
/// window pairs with the smallest capture edge `c0·pc/2 + j·pc`
/// strictly greater; the winner minimises `capture − launch`. Returns
/// `(m·pl, j·pc)` — additive shifts on the recorded edge-0 arrivals.
/// `None` only when `grid_lcm` fails.
fn setup_edge_pair(pl: f32, pc: f32, g: usize, c0: usize) -> Option<(f32, f32)>;

/// §1.2 hold pair: the smallest `T` on both grids
/// (`g·pl/2 + m·pl == c0·pc/2 + j·pc`). Returns `(m·pl, j·pc)` for that
/// `T`. `None` when the tying parity's residue has no coincident
/// capture edge in the window — the parked hold case.
fn hold_edge_pair(pl: f32, pc: f32, g: usize, c0: usize) -> Option<(f32, f32)>;
```

Implementation notes:

- Launch-edge enumeration goes through `launch_window(g, pl, lcm)`:
  `m` from 0 while `g·pl/2 + m·pl < lcm·(1 − 1e-6)` — the boundary
  edge at LCM duplicates edge 0, and the margin absorbs the f32-scale
  noise `grid_lcm` leaves in the bound.
- `grid_lcm`'s remainder test is relative to the *input* periods
  (~1e-6 of the smaller): `1.4f32` is `1.399999976…` in f64, so an
  exact Euclid gcd would sink to the f32 rounding residue and report
  an absurd LCM — the window accepts the f32-scale gcd instead.
- Capture index: `j = floor((Tl − phase_c)/pc) + 1` clamped `≥ 0`,
  then a short `while` walk in both directions so `phase_c + j·pc` is
  the exact smallest value `> Tl` (with `eps = 1e-9·max(pl, pc)`) —
  robust to `f64` noise at coincident points (a coincident capture
  edge is never "strictly after").
- `hold_edge_pair`: enumerate `m` in the window, test whether
  `(Tl − phase_c)/pc` is integral within ~1e-6 relative; keep the
  smallest `T`. Coincident-pair slack is `T`-invariant (both sides
  shift by `T`), so the smallest `T` is the reported pair.
- Equal-gap ties on the setup side: keep the first found (smallest
  launch edge). OpenSTA's tie-break is unmeasured — noted in code.

## 3. Worked examples (verified against `two_clock.probe.json`)

`cka` 2.0 → `ckb` 1.4, `u_fx/D` (`t2_frac`, LCM 14.0), `g = c0 = 0`:

| `Tl` | `Tc` | `Tc−Tl` |
|---|---|---|
| 0 | 1.4 | 1.4 |
| 2 | 2.8 | 0.8 |
| **4.0** | **4.2** | **0.2** ★ |
| 6 | 7.0 | 1.0 |
| 8 | 8.4 | 0.4 |
| 10 | 11.2 | 1.2 |
| 12 | 12.6 | 0.6 |

→ `arr = 0.277469 + 4.0 = 4.277469`, `req = 0 + 4.2 − 0.114554 = 4.085446` ✓

`ckb` 1.4 → `cka` 2.0, `u_fy/D`: launches `{0,1.4,…,12.6}`, captures
even integers; min gap 0.2 at launch 9.8 → capture 10.0 →
`arr 10.07747`, `req 9.885446` ✓

`ckb` 1.0 → `cka` 2.0 (`t2_int` B→A, LCM 2.0): launches `{0, 1}` —
`0→2.0` (Δ2.0), `1→2.0` (Δ1.0 ★). The min-gap pair wins over edge 0
even on equal grids → `arr = 0.277469 + 1.0 = 1.277469`,
`req = 2.0 − 0.114554 = 1.885446` ✓

`d_in`(`cka`)→`u_fb0/D` (`t2_frac`): same A→B pairing → launch 4.0,
`arr = 0.7 + 4.0 = 4.7`, `req = 4.2 − 0.145238 = 4.054762` ✓

`t2_mdl` B→A (ckb lat 0.2, cka lat 0.6): launch 1.0 (source time) +
latency 0.2 → `arr = 0.477469 + 1.0 = 1.47747`; capture 2.0 + 0.6 −
0.05 − 0.114554 → `req 2.435446` ✓ (probe text shows the launch edge
at source time 1.0, network delay 0.2 → 1.20 at the pin).

## 4. Same-domain degenerate case

`setup_edge_pair` on equal grids (`pl == pc == P`, `c0 = g = 0`)
returns `(0, P)` — launch edge 0, capture edge `P`: identical to the
existing `capture_extra = period` path. In general `(g, c0)` it
reproduces the `c0 <= g → +period` rule exactly. The same-domain
branch nevertheless keeps the existing code verbatim — the helper runs
only for foreign domain bits, which is what makes single-clock goldens
byte-identical *by construction* rather than by formula equivalence.

## 5. Hold (min) side

For tying `(d, g)`:

- `d == cap`: existing code verbatim — `req = arr_max[check.edge] +
  early_time + hold_unc − credit`, `arr` realigned `+ period` when
  `c0 > g`.
- `d ≠ cap`: `(lsh, csh) = hold_edge_pair(pl, pc, g, c0)`;
  `arr = arrival_min[pol] + lsh`;
  `req = arrival_max[check.edge] + csh + early_time + unc_h(d→cap)`;
  `slack = arr − req`. `None` → the endpoint defers `CrossDomain`
  (whole-endpoint rule — both sides are needed to report).

Fixture check (`g = c0 = 0`, no inversion): `hold_edge_pair` returns
`(0, 0)` — the formula reduces to the current one verbatim, matching
goldens (`u_fy/D min_req −0.037585`, `u_fb0/D −0.048797`).

Coincidence is a *residue* property: `g·PL/2` must sit on the capture
`check.edge` grid `mod PC`. `g = 0` always coincides at `T = 0`;
`g = 1` coincides iff `PL/2 ≡ c0·PC/2 (mod PC)` — e.g. PL=2.0/PC=1.4
coincides at `T = 7.0`, PL=1.4/PC=2.0 never does (defer). Unit-tested
in `tests.md` §2.

## 6. Multicycle interplay (measured `t2_mcp`)

`set_multicycle_path -setup 2 -hold 1 -from u_fa0 -to u_fx`, `u_fx/D`
(cka→ckb, periods 2.0/1.0):

- max: paired req `0.885446` (launch 0 → capture 1.0) + `(2−1)·1.0` →
  `1.885446` — the compose pass's `d_max = (ns−1)·cap.period` applies
  on top of pairing unchanged.
- min: golden `−1.037585` = coincident req `−0.037585` **− 1.0** —
  i.e. `d_min = −nh·cap.period`. The same-domain carry term
  `((ns−1)−nh)·P` does **not** apply: the cross-domain hold pair lives
  on the coincident grid and the setup multiplier does not carry into
  it (the shifted setup edge 2.0 is not the coincident hold edge).
  `docs/multi-clock.md` §1.6 gets a one-line addendum recording this.

Compose-pass rule: for a min candidate whose tying `dom_min` mask has a
bit ≠ the capture bit, use `d_min = −nh·Pc`; same-domain masks keep
`((ns−1)−nh)·Pc`. Mixed masks take the cross-domain form (the tied
extreme is at least partly foreign — noted as conservative).

## 7. Delay-check interplay

`set_max_delay`/`set_min_delay` endpoints need no pairing, in either
`-from` form:

- **Launch selectors** (cells, ports, clock pins): the compose rewrite
  anchors req at the capture domain's edge-0 arrival
  (`clk_s.arrival_*[anchor]`, already per-domain since `mc-eval`).
  Pairing in `collect_endpoints` simply produces the candidates the
  rewrite needs; `w.dly` (the edge-0 delay arrival) is reported
  unchanged — `t2_mdl` `u_fx/D` `arr 0.877469` is the launch edge-0 +
  latency arrival, *not* the paired setup edge. No formula change in
  the rewrite itself; only the per-candidate uncertainty lookup
  (`api.md` §6).
- **Pin startpoints** (`get_pins inst/Q`, measured `t2_mdx`): there is
  no launch edge to pair at all — the check is `N` against the segment
  arrival `ep.dly − pin.dly`, `req = N ∓ library constraint` (`api.md`
  §5.3). A `-from` pin unreached in the signature class is skipped in
  the fold; no reached pin → the polarity's candidate drops (`api.md`
  §5.4).

## Files

- `crates/grus-core/src/sta/analysis.rs`
