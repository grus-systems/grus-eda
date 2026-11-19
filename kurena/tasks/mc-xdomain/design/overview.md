# mc-xdomain — Overview

Third ticket of the multi-clock sequence (`docs/multi-clock.md` §3).
`mc-eval` tagged every launch with its clock domain and made
launch/capture pairs in different domains defer as named `CrossDomain`
skips. This ticket evaluates those pairs per the measured OpenSTA rules
(`docs/multi-clock.md` §1.2–§1.5, all verified against the committed
`tests/designs/sta_clocks` probe artifacts):

1. **Setup edge pairing** (§1.2). For a check whose launch domain ≠
   capture domain, enumerate (launch edge, capture edge) pairs — each
   launch edge of the tying parity with the smallest capture-domain
   `check.edge` edge strictly after it — over the period-LCM window, and
   report the pair minimising `capture − launch`. The reported arrival
   shifts by the winning launch edge (`arr = arr₀ + m·PL`); the required
   time shifts by the winning capture edge (`req = clk_arr + j·PC −
   t_setup − uncertainty`). Verified numbers: `t2_frac` `u_fx/D` launch
   4.0/capture 4.2 (`arr 4.277469`, `req 4.085446`), `u_fy/D` 9.8→10.0;
   `t2_int` B→A picks 1.0→2.0, not edge 0.
2. **Hold stays on coincident edges** (§1.2). The min side pairs launch
   and capture edges at the same source time. For the committed fixtures
   every tying launch is parity 0 (non-inverting clock nets), so the
   pair is edge 0 ↔ edge 0 and the current formula applies verbatim —
   `u_fy/D` `min_req −0.037585` is identical to a same-domain check. A
   tying launch parity with no coincident capture edge inside the LCM
   window defers as `CrossDomain` (the deeper hold-pair selection is
   unmeasured; recorded as a parked item in `docs/multi-clock.md`).
3. **Inter-clock `set_clock_uncertainty -from/-to`** (§1.3). New parse:
   per directed (from, to) pair the value **replaces** the capture
   clock's uncertainty for that direction only — not additive, not
   symmetric. `t2_uncx`: `u_fx/D req 3.785446 = 4.2 − 0.30 − setup`
   while B→A keeps `cka`'s own 0.05. Hold side uses `-hold`.
4. **Delay-check-covered cross endpoints report** (§1.5 + the `mc-eval`
   review finding). `set_max_delay`/`set_min_delay` required times
   already anchor at the capture domain's edge-0 arrival
   (`run_sta_partitioned` reads `clk_s.arrival_*[anchor]` +
   `cap_clk.*`) — they need no edge pairing. Once `collect_endpoints`
   produces candidates for cross-domain endpoints, the existing compose
   rewrite produces the measured numbers verbatim (`t2_md`/`t2_mdl`:
   `u_fx/D req 0.935446`, `u_fy/D min_req 1.162415`). Un-skip exactly
   that: a cross endpoint reports when every reached extreme pairs —
   delay-covered sides never needed pairing in the first place.
5. **Golden coverage** on `sta_clocks`: `t2_frac`/`t2_int` cross-domain
   endpoints, `t2_unc`/`t2_uncx` direction-specific uncertainty,
   `t2_md`/`t2_mdl`/`t2_mdo` delay-check endpoints, plus a new `t2_mdx`
   scenario covering a cross endpoint with **both** sides delay-covered
   (the literal "covered on both sides" case — `fixture.md`). All
   compared against the committed probe JSON at `Tol::STRICT`, one
   fail-proof mutation recorded.

## Remaining `CrossDomain` deferrals (never fold)

The deferral machinery stays; it fires only when a side genuinely
cannot be evaluated:

- a reached extreme with an empty domain mask (`<undeclared>` — the
  fold `mc-eval` exists to catch);
- a **latch-D check** with a foreign launch (cross-domain borrow
  pairing is parked — the fixtures have none);
- a **min-side tying launch parity with no coincident capture edge**
  inside the LCM window (parked, §1.2's unexercised branch);
- a non-converging period-LCM search (defensive bound — unreachable for
  `f32` periods, which are always rational).

`cross_domain_endpoints` stays counted, `--strict`-gated and reported
verbatim; on every committed `sta_clocks` scenario it is 0 after this
ticket.

## Out of scope

- `mc-selectors`: `get_clocks` selectors in exceptions,
  `set_clock_groups` (`t2_grp` stays parse-gated), `create_clock
  -waveform`, `set_clock_latency -early/-late`, generated clocks.
- `mc-golden`: promotion to `two_clock.reference.json`.
- Cross-domain latch borrow pairing (deferred, parked).
- Cross-domain gating-enable and output-port pairing — the code path
  generalizes uniformly but no committed fixture exercises it (all
  `sta_clocks` output ports anchor same-domain).
- Virtual-clock-distinguished inter-clock uncertainty: `-from`/`-to`
  matching is on folded domain tags (`api.md` §4).

## Golden-identity invariants

Single-design-clock SDCs must stay byte-identical by construction:

- The pairing path runs only when a `dom` mask bit differs from the
  capture tag. With one design row every mask is `{0}` and `same-domain`
  is always the branch taken — the existing `c0 <= g`/`c0 > g`
  arithmetic is untouched on that branch.
- `uncertainty_ns(launch, cap, side)` falls back to the capture row's
  field when no `-from`/`-to` rule matches; `inter_clock_uncertainty`
  is empty on every existing SDC → identical reads.
- `setup_edge_pair` degenerates to the current rule on equal grids
  (verified, `pairing.md` §4) but is deliberately *not* used on the
  same-domain branch.
- The hold (`min`) side keeps the existing formula; the foreign branch
  adds a coincident-edge evaluation that reduces to it at (m=0, j=0).
- `foreign_launch` is replaced by `xdomain_defer`, which returns
  `None` for every mask combination a single-domain SDC can produce.

One intended non-golden-visible change: `t2_uncx` now parses (it was a
loud bail); its `multi_clock_bails` case moves to a positive parse
test.

## Risks

- The `dom_*`/`par_*` masks are independent unions over tying launches;
  pairing iterates their cross product, which can contain (domain,
  parity) combinations no real launch produced. Conservative worst-slack
  evaluation over the product is honest but can over-defer the min side
  (non-coincident phantom combos) — noted in `api.md` §3.
- A launch extreme can tie several domains; per-bit pairing then keeps
  the worst-slack candidate — the reported pair belongs to one domain,
  matching OpenSTA's worst-path report.
- `f32` period arithmetic: pairing computes edge grids in `f64` inside
  `grid_lcm`/`*_edge_pair` and casts the returned shifts to `f32`; the
  strict tolerance (1% rel / 5 ps) absorbs the conversion.
- `probe.sh` regeneration is additive-only for existing artifacts —
  `git diff` must show the 14 existing scenario blocks and `probe_*.txt`
  byte-identical (AGENTS.md §7 rule 1).

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/sdc.rs`
- `tests/designs/sta_clocks/t2_mdx.sdc`
- `tests/designs/sta_clocks/probe.sh`
- `tests/designs/sta_clocks/two_clock.probe.json`
- `tests/designs/sta_clocks/probe_t2_mdx.txt`
- `tests/designs/README.md`
- `docs/multi-clock.md`
- `docs/results.md`
- `docs/roadmap.md`
- `docs/path-delays.md`
- `Cargo.toml`
- `Cargo.lock`
