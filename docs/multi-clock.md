# Multi-clock SDC support — measured rules and ticket sequence

Status: probe complete (2026-11-19), `mc-model` landed (2026-11-20).
`Sdc` now keeps a clock table — one row per `create_clock`, in file order,
holding name/optional port/period/network+source latencies/setup+hold
uncertainties/transition/propagated — and every clock-targeting command
resolves its names against the table. Eval is still single-domain: `sta`
bails on more than one design clock (`analysis.rs`: "sta: N design clocks
declared ...; multi-clock analysis is not implemented yet"). This document
remains the target spec for the remaining tickets.

Evidence: `tests/designs/sta_clocks/` — `two_clock.def` (clock domains `cka`
2.0 ns on `clk1`, `ckb` 1.4/1.0 ns on `clk2`, same-domain and both-direction
cross-domain paths), twelve `t2_*.sdc` scenarios, `probe.sh` (runs OpenSTA in
`ghcr.io/librelane/librelane:3.0.16`), `two_clock.probe.json` + `probe_*.txt`
(byte-identical on re-run). Raw numbers and the grus-eda break table are in
`docs/results.md` (2026-11-19 entry); scenario→question mapping is in
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
  one ckb period (min req −0.037585→−1.037585).

## 2. Where grus-eda stands (after `mc-model`, 2026-11-20)

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
- `set_input_delay`/`set_output_delay -clock <name>` and delay exceptions
  resolve any row, not just the design clock.

Still unsupported / remaining hazard:

- `set_clock_latency -early`/`-late`, `set_clock_uncertainty -from ... -to
  ...`, `create_clock -waveform`, `set_clock_groups`, unknown `-clock`
  names, `set_propagated_clock` on a virtual clock, `create_clock` with
  `period 0` — all loud parse errors.
- Eval: `sta` on an SDC with more than one design clock bails before
  graph preparation — `sta: 2 design clocks declared (cka, ckb);
  multi-clock analysis is not implemented yet` — instead of folding the
  unseeded domain onto the first clock's lattice. Verified by
  `analysis.rs::tests::sta_bails_on_a_two_clock_sdc` and a scratch run with
  the gate disabled (all 8 endpoints reported against `cka`, domains
  folded).
- A single-clock SDC on a multi-clock DEF still silently folds the
  undeclared domain's pins onto domain 0's lattice (no warning);
  detection needs domain tags — `mc-eval` owns it.

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
2. **`mc-eval`** — domain tagging: per-port clock-net seeding (propagated
   and `ideal_clock_parity`), domain tags on launch seeds and clock pins,
   non-first clock ports excluded from data seeding; same-domain pairing
   with distinct periods; per-domain period in MCP/borrow windows and
   I/O anchors.
3. **`mc-xdomain`** — cross-domain launch/capture edge-pair search per the
   measured rule (§1.2: min `capture − launch` over the LCM window);
   inter-clock `-from`/`-to` uncertainty (§1.3).
4. **`mc-selectors`** — clock-object selectors in exceptions (`-from
   [get_clocks cka]` etc.), `set_clock_latency` multi-name (already
   row-wise after `mc-model`), `set_clock_groups` (measured: drops all
   cross checks, I/O-anchored included, §1.6), `create_clock -waveform`.
5. **`mc-golden`** — `sta_clocks` promoted to `two_clock.reference.json` +
   `test_sta_clocks_match_opensta`, closing `docs/path-delays.md` §5 item 1
   (the literal two-clock `set_min_delay` fixture is `t2_md`/`t2_mdl`).

Open sub-questions parked for the implementation tickets: cross-domain
hold-pair selection beyond coincident edges (§1.2 — fixture holds at
edge 0); `-waveform`-defined duty cycles; `-early`/`-late` latency splits;
generated clocks; `set_clock_latency` on a delay check's *launch* side vs
the measured capture anchor.
