# Multi-clock SDC support — measured rules and ticket sequence

Status: probe complete (2026-11-19), `mc-model` landed (2026-11-20),
`mc-eval` landed (2026-11-20), `mc-xdomain` landed (2026-11-20). `Sdc`
keeps a clock table — one row per
`create_clock`, in file order, holding name/optional port/period/
network+source latencies/setup+hold uncertainties/transition/propagated —
and every clock-targeting command resolves its names against the table.
Eval is domain-aware: each design clock seeds its own net, every data
arrival carries a launch-domain tag, and same-domain paths evaluate on the
capture domain's own lattice (period, latency, uncertainty, propagated/
ideal mode). Cross-domain setup pairs report on the min-gap edge pair
over the period LCM (§1.2) with per-direction inter-clock uncertainty
(§1.3); delay-check-covered endpoints anchor at the capture domain's
edge 0 (§1.5). A pair that still cannot be evaluated defers with a
`CrossDomain` diagnostic naming both clocks — never a silent fold. This
document remains the target spec for the remaining tickets.

Evidence: `tests/designs/sta_clocks/` — `two_clock.def` (clock domains `cka`
2.0 ns on `clk1`, `ckb` 1.4/1.0 ns on `clk2`, same-domain and both-direction
cross-domain paths), `two_clock_latch.def` (a `dlxtn` on `ckb` feeding a
`ckb` flop — every endpoint same-domain), fifteen `t2_*.sdc` scenarios on
`two_clock.def` plus `t2_latch.sdc`, `probe.sh` (runs OpenSTA in
`ghcr.io/librelane/librelane:3.0.16`), `two_clock.probe.json` +
`two_clock_latch.probe.json` + `probe_*.txt` (byte-identical on re-run).
Raw numbers and the grus-eda break table are in `docs/results.md`
(2026-11-19 and 2026-11-20 entries); scenario→question mapping is in
`kurena/tasks/multi-clock-probe/design/probe.md`.

## 1. Measured OpenSTA rules (all from container runs)

### 1.1 Cross-domain paths are timed by default

Two `create_clock`s on separate ports define two domains; every flop's
domain is the clock reaching its CLK pin (net-traced). Paths crossing
domains are checked both directions with no extra SDC — `u_fa0`(cka)→`u_fx`(ckb)
and `u_fb0`(ckb)→`u_fy`(cka) appear in the endpoint table; reports name the
launch clock at the source and the endpoint's clock at capture
(`probe_t2_int.txt`).

### 1.2 Setup pairing = most restrictive edge pair over the period LCM

For a cross-domain check OpenSTA enumerates (launch, capture) edge pairs —
each launch edge of the source domain with the smallest capture-domain edge
strictly after it — and the reported path is the pair minimising
`capture − launch`. This is the H-window hypothesis from `probe.md`;
H-first (capture always right after edge 0) is falsified.

`cka` 2.0 / `ckb` 1.4 (`t2_frac`, LCM 14.0 ns):

- A→B `u_fa0→u_fx`: reported launch 4.0 → capture 4.2 (Δ0.2 ns);
  `arr 4.277469 = 4.0 + clk→Q`, `req 4.085446 = 4.2 − setup 0.114554`.
- B→A `u_fb0→u_fy`: reported launch 9.8 → capture 10.0 (Δ0.2 ns);
  `arr 10.07747`, `req 9.885446`.
- Input check `d_in`(clocked cka)→`u_fb0`(ckb) enumerates the same way:
  launch-edge 4.0 + `in_max` 0.7 = `arr 4.7`, `req 4.054762`.

Integer ratio 2.0/1.0 (`t2_int`): B→A reports launch **1.0** → capture 2.0
(Δ1.0), not edge 0 (Δ2.0) — the min-gap pair wins even with equal grids.

Hold pairs stay on coincident launch/capture edges (edge 0 in every
scenario here); no cross-domain hold pair selection beyond the coincident
grid was exercised.

### 1.3 Uncertainty is per-clock, capture-side; `-from`/`-to` overrides pairing-wise

`t2_unc` (`-setup 0.05 cka`, `-setup 0.20 ckb`, `-hold 0.10 cka`):

- `u_fx/D` (capture ckb): `req 3.885446 = 4.2 − 0.20 − setup`
- `u_fy/D` (capture cka): `req 9.835446 = 10.0 − 0.05 − setup`
- same-domain and output checks take their own clock's value
  (`yb req 0.5 = 1.4 − 0.7 − 0.20`).

So for cross paths the applied uncertainty is the **capture** clock's — not
the launch's, the max, or the sum (0.20 vs candidates 0.05/0.25).

`t2_uncx` adds `-setup 0.30 -from cka -to ckb`: `u_fx/D req 3.785446 = 4.2 −
0.30 − setup` and `u_fb0/D req 3.754763` — the inter-clock value **replaces**
the capture clock's 0.20 for that direction only; B→A and same-domain
checks unchanged. Hold side uses the capture clock's `-hold` value
(`u_fy/D min req 0.062415 = 0 + 0.10 − hold`; `u_fx/D` −0.037585 with ckb
hold-uncertainty unset).

### 1.4 Latency is per-clock; `-source` sums with network latency

`t2_lat` (network 0.6 on cka + `-source 0.1`, network 0.2 on ckb; ideal
clocks, periods 2.0/1.4): arrivals carry the **launch** domain's latency
(`u_fx/D arr 4.977469 = 4.0 + 0.6+0.1 + 0.277469`), required times the
**capture** domain's (`req 4.285446 = 4.2 + 0.2 − setup`). Source latency
adds to network latency for a non-propagated clock (cka total 0.7).

### 1.5 Delay checks anchor at the capture domain's edge-0 arrival

`set_max_delay N`/`set_min_delay N` on a second-domain endpoint:

`req_max = N + capture_latency − capture_setup_unc − t_setup`
`req_min = N + capture_latency + capture_hold_unc − t_hold`

`t2_mdl` discriminates all three anchors (distinct latencies/uncertainties
per domain; periods 2.0/1.0):

- `u_fx/D` (capture ckb, `set_max_delay 1.0`): `req 0.935446 = 1.0 + 0.2 −
  0.15 − 0.114554` — capture domain's latency and setup uncertainty.
- `u_fy/D` (capture cka, `set_min_delay 0.5`): `req 1.162415 = 0.5 + 0.6 +
  0.10 − 0.037585` — capture domain's.
- Report text (`probe_t2_mdl.txt`) shows the `max_delay`/`min_delay` line
  followed by the capture domain's own `clock network delay`/`clock
  uncertainty` lines; no capture-clock edge line appears (the anchor is
  that domain's edge-0 arrival).
- The arrival side keeps the covered source's ordinary arrival **including
  its launch domain's latency**: `u_fx/D arr 0.877469 = 0.6 + 0.277469`.

Output-port form (`t2_mdo`, `set_max_delay 0.8 -to yb`):
`req 0.1 = 0.8 − out_delay_max 0.7`; no clock-latency/uncertainty term
(none set in that scenario).

### 1.6 Clock groups, propagation, MCP

- `set_clock_groups -asynchronous -group cka -group ckb` (`t2_grp`): all
  cross-domain checks disappear — endpoint table shrinks 8→5 (`u_fb0/D`
  input check, `u_fx/D`, `u_fy/D` gone); same-domain and same-domain-anchored
  I/O checks untouched.
- `set_propagated_clock` is per-domain (`t2_prop1`, cka only): cka-domain
  launches use propagated arrivals (`u_fa1/D arr 0.379373`) while ckb stays
  ideal (`u_fb1/D arr 0.277469`); capture sides likewise independent —
  `u_fx/D req 0.885503` (ckb ideal capture) vs `u_fy/D req 1.981787` (cka
  propagated capture).
- MCP multiplies the **capture** domain's period (`t2_mcp`,
  `set_multicycle_path -setup 2 -from u_fa0 -to u_fx`): capture moved from
  ckb edge 1.0 to **2.0** (req 0.885446→1.885446); `-hold 1` moves it back
  one ckb period (min req −0.037585→−1.037585). On a cross-domain min
  candidate the shift is `d_min = −nh·Pc` — the same-domain carry term
  `((ns−1)−nh)·P` does not apply, since the cross hold pair lives on the
  coincident grid and the shifted setup edge is not the coincident hold
  edge. A mixed tying mask takes the cross-domain form (conservative).

## 2. Where grus-eda stands (after `mc-xdomain`, 2026-11-20)

`cargo test -p grus-core multi_clock_bails` pins the remaining bail
messages verbatim; full table with commands in `docs/results.md`.

Now accepted (the probe's parse breaks are fixed):

- N `create_clock`s on ports and any number of virtual clocks append rows
  to `Sdc::clocks`; a virtual period no longer has to equal the design
  period.
- `set_clock_uncertainty`/`-latency` (`-source` included)/`-transition`/
  `set_propagated_clock` resolve `[get_clocks {a b ...}]` row-wise —
  multi-name lists apply to every selected row; uncertainty on a virtual
  clock lands on its own row (the probe's silent wrong-target bug).
  `set_clock_uncertainty -from`/`-to` stores directed inter-clock rules
  that replace the capture row's value for the matching direction and
  side only (§1.3; last matching rule wins, empty selector = wildcard).
- `set_input_delay`/`set_output_delay -clock <name>` and delay exceptions
  resolve any row, not just the design clock.

Now evaluated:

- Every design clock row seeds its own clock net — propagated via
  `propagate_clock`, ideal via the parity walk — and every data arrival
  carries a `u8` launch-domain mask alongside the parity/CRPR masks
  (winner-replace/tie-union semantics). A port's tag is its resolved
  `set_input_delay -clock` row, mapped through `launch_domain`: a virtual
  row whose period equals a design row's pairs same-domain (coincident
  edge grids), a unique virtual row stays foreign everywhere.
- Checks read the capture row's period, latency, uncertainty and
  propagated/ideal mode: register checks, latch-borrow windows,
  delay-check anchors, MCP multipliers, min-pulse-width wrap and output
  ports (`-clock` row anchoring) are all per-domain. Verified
  per-endpoint at STRICT tolerance on all parseable `t2_*` scenarios
  (`test_sta_clocks_match_opensta`); `two_clock_latch`'s whole table —
  wns/tns/violations included — matches.
- Cross-domain setup pairs evaluate on the min-gap edge pair over the
  period LCM (§1.2): launch `arr` shifts by `m·PL`, required by `j·PC`,
  uncertainty per directed pair (§1.3). Hold stays on coincident edges.
  Cross-domain endpoints covered by `set_max_delay`/`set_min_delay`
  report on the capture domain's edge-0 anchor without pairing (§1.5);
  a `get_pins inst/Q` `-from` is a path startpoint — the clocked launch
  out of that pin is replaced by pin-segment paths, and endpoints no
  covering rule's `-to` names lose the check on that side (`t2_mdx`).
- A pair that still cannot be evaluated counts
  `skipped.cross_domain_endpoints` and lands in `skipped_endpoints` with
  `reason: cross_domain` and `related: "launch <clocks> -> capture
  <clock>"`. `--strict` fails (`is_clean` counts them). The remaining
  deferrals: a reached extreme with a zero mask (`launch <undeclared>`),
  a latch-D check with a foreign launch (borrow pairing parked), a
  min-side tying parity with no coincident capture edge in the LCM
  window (hold selection parked), and a non-converging `grid_lcm`
  (defensive — unreachable for `f32` periods).

Still unsupported / remaining hazard:

- `set_clock_latency -early`/`-late`, `create_clock -waveform`,
  `set_clock_groups`, unknown `-clock` names, `set_propagated_clock` on a
  virtual clock, `create_clock` with `period 0` — all loud parse errors.
- More than 8 clock rows (design + virtual) bail before graph prep: the
  domain mask is `u8` (`sta: N clocks declared; at most 8 domains can be
  tagged`).
- Cross-domain hold selection beyond the coincident grid: parked (§1.2 —
  the fixtures hold at edge 0 or a parity-coincident edge; a parity with
  no coincident capture edge defers `CrossDomain`).

## 3. Ticket sequence

The gap is structural (one slot for every clock attribute + one lattice),
not a patch — implementation goes in five gated tickets, each its own
kurena spec:

1. **`mc-model`** — **done 2026-11-20** (`docs/results.md` entry of the
   same date). `Sdc` keeps a clock table: per-domain
   name/port/period/latencies (network+source)/uncertainties/transition/
   propagated flag; clock references are table indices (`ClockId`);
   `create_clock` accepts N design clocks and virtual clocks join the
   table. `set_clock_uncertainty`/`-latency`/`-transition` target table
   rows; multi-name lists allowed. Eval still single-domain: `sta` bails
   on >1 design clock, never silently folds. Parse + model tests in
   `sdc.rs::tests`; existing goldens bit-identical.
2. **`mc-eval`** — **done 2026-11-20** (`docs/results.md` entry of the
   same date). Domain tagging: per-port clock-net seeding (propagated and
   ideal parity walk), domain tags on launch seeds and clock pins, clock
   ports excluded from data seeding; same-domain pairing with distinct
   periods; per-domain period in MCP/borrow windows and I/O anchors.
   Cross-domain pairs defer as named `CrossDomain` skips.
3. **`mc-xdomain`** — **done 2026-11-20** (`docs/results.md` entry of the
   same date). Cross-domain launch/capture edge-pair search per the
   measured rule (§1.2: min `capture − launch` over the LCM window);
   inter-clock `-from`/`-to` uncertainty (§1.3); delay-check-covered
   cross endpoints report on the capture edge-0 anchor (§1.5).
4. **`mc-selectors`** — clock-object selectors in exceptions (`-from
   [get_clocks cka]` etc.), `set_clock_latency` multi-name (already
   row-wise after `mc-model`), `set_clock_groups` (measured: drops all
   cross checks, I/O-anchored included, §1.6), `create_clock -waveform`.
5. **`mc-golden`** — `sta_clocks` promoted to `two_clock.reference.json` +
   `test_sta_clocks_match_opensta`, closing `docs/path-delays.md` §5 item 1
   (the literal two-clock `set_min_delay` fixture is `t2_md`/`t2_mdl`).

Open sub-questions parked: cross-domain hold-pair selection beyond
coincident edges (§1.2 — a tying parity with no coincident capture edge
defers `CrossDomain`; the equal-gap setup tie keeps the smallest launch
edge, OpenSTA's tie-break unmeasured); `-waveform`-defined duty cycles;
`-early`/`-late` latency splits; generated clocks; `set_clock_latency` on
a delay check's *launch* side vs the measured capture anchor; inter-clock
uncertainty matching is on folded domain tags — a virtual clock that
folds onto a design row shares that row's `-from`/`-to` rules.
