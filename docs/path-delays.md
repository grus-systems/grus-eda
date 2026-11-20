# Path delay constraints (`set_max_delay` / `set_min_delay`)

Status: **implemented** (v1 2026-11-18; output-port endpoints 2026-11-19;
multi-launch port coverage 2026-11-19; async-check and latch-D endpoints
2026-11-19) —
parser and signature-class
composition live in `crates/grus-core/src/sta/{sdc,analysis}.rs`; every
committed `md_*`/`pmd_*`/`mhu_*`/`m2_*`/`pm2_*`/`mdo_*`/`pmo_*`/`ml_*`/`pml_*`/`pmp_*`/`pcr_*`
scenario
matches OpenSTA
(`test_sta_exceptions_match_opensta`). This document records the OpenSTA
semantics measured on the `sta_exceptions` fixture (see
`tests/designs/sta_exceptions/generate_ref.sh`) and the implementation shape.
Sections 3's "v1 diagnoses" and "explicitly out of scope" lists still apply;
the remaining follow-ups in section 5 are open.

## 1. What OpenSTA does (measured)

`set_max_delay N` / `set_min_delay N` create per-(source, endpoint, data
transition) **path-delay checks** (`is_path_delay` in `find_timing_paths`).
For every covered path OpenSTA **replaces** the ordinary clock check
(`is_check`) or output-delay check (`is_output_delay`) — the original check
elements disappear from the path list, they do not coexist.

Each covered (source -> endpoint) pair yields **two** delay checks, one per
data transition (rise and fall), each with its own launch reference and data
arrival. `EndpointPolarities { max/min: [Option<Worst>; 2] }` already keeps
per-transition candidates, so this maps onto the existing structure.

### `set_max_delay N` (setup side)

The clock check is **replaced** by a delay check whose required time is
capture-referenced **minus the same setup-side adjustments the clock check
uses**, plus N:

```
req_max_delay = capture_clk_arrival_early - setup_uncertainty - t_setup(endpoint, transition) + N
arr_delay     = the covered path's ordinary data arrival
                (its own launch_clk_arrival_late + data delay)
slack         = req_max_delay - arr_delay
```

Each covered (source, transition) pair produces a candidate carrying its
own launch; the reported check keeps the worst covered candidate — i.e.
the covered-source **arrival** winner, which under a skewed clock tree
need not be the largest pure path delay. Measured on the diverging-winner
fixture (`pm2_both` vs `pm2_prop`, `m2_max0` vs `m2_max2`): `u_ff0` holds
the larger pure delay (~13.5 ps via the `A1` mux input) yet `u_ff2`'s ~69
ps later launch wins the `pm2_both` report (0.786764 vs 0.731424). The
capture term is the endpoint register's **own** clock-pin early arrival —
the same `arrival_min[check.edge]` the setup clock check builds its
closing edge on, minus the period the clock check adds. Verified
numerically on `u_ff0 -> u_ff1/D` (ideal clock, `md_basic`/`m2_max0`):
`0.7 - 0.05 - 0.099944 + 1.0 = 1.550056` — the two per-transition checks
differ only through `t_setup` (the golden reports the rise winner; the
pre-mux fixture measured 0.087853 rise / 0.043395 fall when `u_ff1/D` saw
the bare `u_ff0 -> D` arc), sharing one launch arrival. An earlier draft
of this document misidentified `setup_req - period` as the launch arrival
(0.550057 on the current fixture); review corrected it — the raw
clock-pin arrival (0.7 ideal; under propagation each clock net carries
its own — `pm2_both` separates `c1`/`c3` by ~69 ps) is the launch
reference and the uncertainty/setup-time terms are subtracted explicitly.

The capture-vs-launch distinction is invisible while every clock pin shares
one net arrival — the split-clock-buffer fixture (`pm2_both`) separates
them: `u_ff1/D` reports `req = 1.013333` (anchored at `u_ff1/CLK` on `c1`),
not the launch-referenced ~1.082 the arrival winner `u_ff2`'s launch on
`c3` (~69 ps later, behind the extra `u_clkb3` stage) would give. The CRPR
credit **is** applied to delay checks (measured 2026-11-19 on the
`pcr_*` spread fixture, section 4): `pcr_max`'s `u_ff0 -> u_ff1` check
reports `req = capture_early - unc - t_setup + N + S` where
`S = 0.031507` is the early/late spread at the shared clock-net driver
`u_clkb/X`, and `pcr_min` shows the hold delay check's required time
moves by `-S`; `pcr_off` (covered launch on `c3`, sharing only the
`clk` root with `c1`) confirms no credit on a non-shared path.

- Replace semantics proven by `md_loose` (`N = 5.0` >> setup budget): the
  covered `u_ff0` source's req became `capture - unc - t_setup + 5.0` —
  slack so loose the report's min-slack pick now falls to a surviving
  `u_ff2`/`sel` clock check (reported req 2.550057). On the pre-mux
  fixture `u_ff0` was the only source and the report showed the delay
  check itself (`1.562147`-style req); the replacement is still exercised
  by `md_tight`/`md_basic`, where the delay check wins.
- Input-port sources work identically: `md_port` (`-from [get_ports d_in]`)
  gave `req = 0.528237 + 1.0 - adjustments` on `u_ff0/D`; the port launch
  reference is the I/O clock edge the input delay is anchored to. Under
  `set_propagated_clock` the `pmp_*` scenarios measured the launch anchor
  at the SDC `-source` latency: `pmp_port`'s `u_ff0/D` reports the
  ordinary propagated port seed `arr 0.8` (`0.1` source latency + `0.7`
  input delay), not the `1.4` an ideal-latency anchor (`source + network`)
  would give nor the ~`0.87` a capture-pin-propagated anchor would; and
  `pmp_port2` (network latency 0.6 -> 1.1) is byte-identical to
  `pmp_port`, so the anchor carries no network-latency term. On this
  fixture that anchor is numerically identical to the propagated arrival
  at the `clk` port pin — `clk` is the propagation root, so its pin sees
  source latency plus zero insertion delay; separating the two named
  candidates needs insertion upstream of a root and is out of scope.
  The delay check shares the port's ordinary propagated arrival — the
  same `input_clock_latency_ns` seed `base_prop` reports.

### `set_min_delay N` (hold side)

The hold check is replaced by a check whose required time is the **base
hold required time (without multicycle adjustments) plus N**:

```
req_min_delay = req_hold_base + N
```

Measured across N in {0.0, 0.5, 1.0}, hold-uncertainty 0.1 and 0.5, port and
FF sources, and propagated clock (`pmd_min`: `0.248340 + 0.5 = 0.748340`
exact). Multicycle independence proven by `md_min_mcp_h1`/`md_min_mcp_s2`:
with `-hold 1` or `-setup 2` on the same pair, the delay required stayed
`0.778288 + 0.5` — MCP shifts do **not** feed into the delay check (the
delay check replaces the MCP-adjusted clock check entirely on that side).

`pm2_min2` separates the launch (`u_ff2` on `c3`, behind two clock-buffer
stages) from the capture (`u_ff1` on `c1`) by ~69 ps and the formula still
reproduces exactly, so the capture-anchored `req_hold_base + N` is
verified for skewed launches on one clock domain; a literal
two-clock-domain fixture is listed as follow-up work in section 5.

### Output ports (`-to [get_ports ...]`)

Supported since 2026-11-19 (`mdo_*`/`pmo_*` committed scenarios). The port
delay check is referenced to the delay edge itself — no latency or period
term, and `set_propagated_clock` does not move the anchor (`pmo_*` reproduces
the ideal-clock required times exactly):

- max side: `req = N - output_delay_max - setup_uncertainty` — `mdo_max05`/
  `mdo_max1`/`mdo_max2`/`mdo_max3` read `N - 0.75` at all four N points;
  `mdo_max3`'s delay req (2.25) is looser than the ordinary output req
  (1.95) and still replaces it — same replace semantics as FF checks.
- min side: `req = N - output_delay_min + hold_uncertainty`. The preamble's
  0.1/0.1 values make the two terms cancel, so `mdo_min0` alone reads
  `req = N`; the attribution probes `mdo_mindl` (delay_min 0.4 -> req -0.3)
  and `mdo_minhu` (hold_unc 0.35 -> req +0.25) isolate each term.
- The candidate keeps its ordinary arrival and slew. `-from` coverage is the
  launch-source model unchanged: `mdo_from1` (`-from u_ff1`) and
  `mdo_from2` (`-from {u_ff0 u_ff1}`) apply the check, `mdo_from0`
  (`-from u_ff0` — its fanout ends at `u_ff1/D`) leaves `y` byte-identical
  to `base`. `all_outputs` resolves (`mdo_all`), and the measured precedence
  false_path > delay > multicycle holds on the port side (`mdo_fp` removes
  `y`; `mdo_mc`/`mdo_mc_rev` order-independently keep the delay req;
  `mdo_min_mch`/`mdo_min_mcs` keep the min delay req off MCP shifts;
  `mdo_both` composes per side).

Coverage note: `y` is driven by `u_ff1/Q` alone, so on `y` the
covered-candidate winner rule is exercised per transition only. The
second output `y2` (`u_mux2` driven by `u_ff0/Q` and `u_ff2/Q`,
`sel`-launched too) covers it per source: `ml_both` merges both FF
launches in one delay check and reports the arrival winner `u_ff0`
(1.508460 — identical to the isolated `ml_max0` check, 16.2 ps above
`ml_max2`); `pml_both` shows the propagated-clock winner flips to
`u_ff2` (1.015388, equal to `base_prop`'s ordinary arrival winner at
`y2`). `ml_all` (`-to [all_outputs]`) also covers the `sel` launch at
`y2`, so its report is `sel`'s candidate (1.533129), not `ml_both`'s —
universal `-from` coverage measured there. `ml_min`/`pml_min` carry the
merged min-side check (`pml_min` reports the delay check's hold
violation, slack -0.109318).

### Asynchronous-check pins (recovery/removal)

Measured 2026-11-19 on `sta_edge/async_reset` (`u_ff0/RESET_B`, `dfrtp`)
and the latch cells of `sta_edge/latch_borrow` (`delay_*` committed
scenarios): OpenSTA **applies** the delay check on both sides — the
recovery arc plays `late_arc`, the removal arc `early_arc`, and the FF
formulas flow through verbatim:

- max side: `req = capture_clk_arrival_min[edge0] - unc_setup -
  t_recovery + N` — `delay_max` on `u_ff0/RESET_B` reads
  `0.7 - 0.05 + 0.250132 + 1.0 = 1.900132`. The anchor is the **first
  edge reaching the clock pin** (`edge0`), not `check.edge`: on the
  `dlrbp`'s `u_rbp/RESET_B` the recovery check fires FALL yet the delay
  req anchors at RISE (`0.912087` under `delay_allregs`), and the two
  coincide on `dfrtp` where `check.edge` is RISE already.
- min side: `req = req_removal_base + N` — `delay_min` reads
  `1.132095 + 0.5 = 1.632095`.
- `-from` restricts coverage per launch source (`delay_from` covers the
  `rst_n` port seed identically); `-to [get_cells u_ff0]` covers the
  cell's check pins (`delay_cell` moves `u_ff0/D` *and*
  `u_ff0/RESET_B`). Negative controls (ad hoc, not committed):
  `-from [get_ports d_in]` and `-from [get_pins u_ff0/CLK]` leave the
  endpoint byte-identical to baseline.
- Propagated clock moves the anchor with the propagated capture arrival
  (`delay_prop`: req `1.404679`).

### Latch-D pins (borrow semantics)

Measured 2026-11-19 on `sta_edge/latch_borrow` (`delay_*` committed
scenarios): OpenSTA **applies** the delay check on both sides, anchored
at the delay edge (the first clock edge at the latch's enable pin,
`lat0`) with the latch's own borrow window:

- max side: `req = N + lat0 - unc_setup + actual_borrow` where
  `actual_borrow = clamp(arr_dly - base, 0, max_borrow)`,
  `base = N + lat0 - unc_setup`, and `max_borrow = pulse - t_setup` —
  the transparent window minus the data setup, exactly the borrow cap of
  the clock check. `report_checks` decomposes it directly (`u_rtn/D`:
  `max_delay 1.00 + crpr 0.00 + borrow 2.32` where
  `max borrow 2.32 = pulse 2.50 - setup 0.18`; OpenSTA prints the setup
  term negated as "library setup time -0.18"). `u_lp/D` reads
  `req = arr_dly = 3.156276` (borrow interior), `u_ln/D` reads the base
  `0.90` (`arr_dly` before the window), and `u_ln2`/`u_rbn`/`u_rtn`/
  `u_xbn` clamp at `base + pulse - t_setup` under `delay_allregs`.
- min side: `req = N + lat0 + t_hold + unc_hold` — delay-edge anchored,
  not closing-edge (`delay_min` on `u_lp/D`: `0.5 + 0 - 0.033557 + 0.1 =
  0.566443`).
- **Delay-check arrival (`arr_dly`)**: a path launched through a latch's
  open edge reports its launch at the delay edge — the clock-to-Q delay
  alone, without the data's open-edge wait. Under `delay_allregs`,
  `u_f0/D` (driven by `u_ln/Q`) reports `arr 0.361663`, exactly its
  baseline `2.861663` minus the `u_ln` open-edge arrival `2.5`. Flop and
  port launches keep their clock edge, so `dly == arr` there. The min
  side also reports the raw delay arrival — no launch-parity
  realignment (`u_lp/D` reads `3.156276`, not the hold-aligned
  `8.156276`). `slack = req - arr_dly` rigidly.
- **Covered `-to` silences the borrow arc class-wide**: a `set_max_delay`
  covering a latch's D pin stops the transparent `D -> Q` forward for
  *every* downstream path of the signature class — covered endpoints
  included (`delay_from`: `u_f2/D` loses the `p_lp -> u_lp D -> Q`
  contribution and drops to the uncovered clock-to-Q launch, `arr
  2.562374 -> 0.244362`). `set_min_delay` coverage does not disable.
  Slew still propagates (slew is physical, not path-scoped).
- `all_registers` covers every sequential check pin — FF-D, latch-D and
  latch `RESET_B` rows together (`delay_allregs`); `delay_prop` shows
  `lat0` is the propagated first-edge latency at the clock pin.

### Selectors

Identical machinery to timing exceptions: `-from`/`-to` accept the same object
selectors (`get_cells`, `get_ports`, `all_registers`, `all_inputs`,
`all_registers`-style collections, `get_pins inst/pin`, bare `inst/pin`,
and since `mc-selectors` `get_clocks`/`all_clocks`/bare clock names —
a clock `-from` keeps the ordinary clocked launch, `docs/multi-clock.md`
§1.7).

- `all_registers` covers sequential cells only — input-port launches are not
  covered (same as exceptions; verified for exceptions by the committed
  `fp_allregs`/`mc_allregs` scenarios where port-launched endpoints kept
  their checks, and ad hoc for delay constraints).
- `get_pins u_ff0/CLK` selects the launch source; `get_pins u_ff1/D` the
  endpoint — same pin semantics as exceptions.
- Omitted `-from` or `-to` = universal on that side.
- Unknown/empty selectors resolve empty (dead constraint), counted via
  `unresolved_exception_selectors`-style diagnostics.

### Multiple rules and precedence

- Two `set_max_delay` commands on the same (from, to) pair: **last wins in
  file order, both directions** (`md_overlap` 1.0->0.5 used 0.5;
  `md_overlap_rev` 0.5->1.0 used 1.0). Same folding applies independently to
  `set_min_delay`.
- `set_max_delay` and `set_min_delay` on the same pair coexist on different
  sides (`md_both`).
- `set_false_path` on the same pair wins over delay constraints: the
  covered source contributes neither the delay check nor its clock check
  (`md_fp`). Endpoint removal happens only when every launch source is
  covered — `u_ff1/D` survives `fp_uf0_uf1`/`md_fp` via `u_ff2`/`sel`,
  while `fp_to_y` empties `y`.
- Delay vs multicycle on the same pair is **order-independent**: the delay
  check replaces the clock check on its side regardless of command order
  (`md_mc`, `md_mc_rev` — max req stayed `capture - unc - t_setup + N`).
  On the min side the delay check uses the **pre-MCP** hold required time
  (`md_min_mcp_h1`, `md_min_mcp_s2` — MCP hold/setup shifts invisible).
  Composition rule: for a delay-covered (s, e) pair, apply the delay
  replacement **instead of** the multicycle shift on that side.

### Qualifiers (OpenSTA acceptance, measured)

| qualifier | OpenSTA result |
|---|---|
| `-rise` / `-fall` | accepted ad hoc; appears to select the matching data transition's check (uncommitted probe — deferred with other qualifiers) |
| `-through` | accepted; path filter — non-matching paths unaffected (`md_through`; note its `u_buf0` target does not exist in `exceptions.def`, so it exercised the non-matching case) |
| `-early` / `-late` | **rejected** `STA-0563` (`md_early` committed as error record) |
| `-datapath_only` | **rejected** `STA-0563` (`md_datapath` committed) |
| `-clock` | **rejected** `STA-0563` (ad hoc probe) |

## 2. Implementation shape

Per covered (class, endpoint, transition) the delay check is the clock
check's candidate with a different required time:

```
arr_reported = the class's ordinary max arrival at the endpoint
               (each covered source's launch already paired with its
               data delay by the per-class propagation)
req_reported = capture_arrival_min[check.edge] - unc_setup - t_setup + N
               + crpr_credit
slack        = req_reported - arr_reported
```

with the capture arrival read at the endpoint register's own clock pin —
it coincides with a launch reference only when launch and capture clocks
see equal arrivals. `crpr_credit` is the same shared-net driver spread
the clock check adds: the pin's `crpr_max` tag mask records which
launch produced the extreme (bits interned per run as `(clock net,
clock edge)` pairs, bit 0 a no-credit sentinel for ports/latch
launches/contested or unresolvable sources), and the covered class's
filter leaves that mask holding only the covered launch's tag. The min
side needs no term of its own — `req_hold_base` already carries `-S`
from the class's hold required time. No per-pin delay domain is
needed: the reported arrival is the covered-source **arrival** winner,
which the signature-class eval run computes directly.

An earlier v1 tracked a separate delay domain on `PinTimingState`
(`delay_max` = max `arrival - launch` per transition, plus
`delay_launch_max` = the delay winner's launch reference) and composed
`arr = delay_launch_max + delay_max`. The 2026-11-19 diverging-winner
regen falsified that pairing: OpenSTA's `pm2_both` reports `u_ff2`'s
arrival (0.786764), not the delay winner `u_ff0`'s (0.731424) — the delay
domain was retired (fields, seed pairing, merge terms and the
latch-borrow delay reconstruction all removed; see `docs/results.md`).

`min` side needs no special state either: the delay check uses the
**pre-MCP** hold required time — `req_min_delay = req_hold_base + N` per
transition, applied instead of the multicycle shift for covered pairs.

Slew needs no pairing: slew is a per-pin/per-transition quantity in our model
(and OpenSTA queries it at the endpoint vertex), identical for clock and
delay checks.

The latch-D rules need one arrival-like quantity of their own: the
**delay arrival** `dly` — `PinTimingState.dly_max`/`dly_min`, merged
independently like slew (it can pick a different source than the `arr`
winner). For FF and port launches `dly = arr`; a latch open-edge launch
(`graph.latch_opens`) strips its clock-edge arrival so the path reports
at the delay edge. Transparent `D -> Q` borrow arcs forward `dly_max`
through the same borrow `cap()` as `arrival_max`; `dly_min` forwards
nothing (`INFINITY` — the early side's clock-to-Q-only rule).
`collect_endpoints` carries `dly` inside `Worst` next to `arr`, and each
endpoint kind substitutes `arr = dly` when composing its delay check —
a no-op for everything except through-latch launches.

A `set_max_delay` rule whose `-to` covers a latch D pin also adds its
`(D, Q)` borrow arc to a per-signature-class `disabled_borrow` set: the
eval run forwards slew but no arrival through it, reproducing the
measured class-wide silencing (uncovered endpoints included). Async
checks need no new branch of their own — `late_arc`/`early_arc` already
resolve to recovery/removal — except the anchor: the delay req is
anchored at `edge0_pol(clk)`, the first edge reaching the clock pin
(decoded from `clock_parity`, falling back to RISE on contested parity),
not `check.edge`.

### Signature classes

Delay coverage joins the exception signature: a source's signature is the set
of constraint rules (false_path / multicycle / max_delay / min_delay) whose
`-from` matches it. Sources with different coverage must land in different
eval classes because at an endpoint a covered source contributes a
delay-check candidate **instead of** a clock-check candidate — mixing them in
one aggregate would lose the distinction. This may split classes vs. the
exceptions-only partitioning; typical SDCs have few rules so run count stays
small. (Alternative — share arrival runs across delay-coverage variants and
only re-run delay state — rejected for v1 complexity.)

### Composition order per (class, endpoint, transition)

Per side independently (max and min compose separately):

1. false_path covering (s, e): drop candidate (existing).
2. else max_delay covering (s, e): keep the covered candidate's slew,
   report `arr = w.dly` (identical to `w.arr` unless the path launched
   through a latch open edge), and replace the required time per
   endpoint kind: FF-D/async-check
   `req = capture_clk_arrival_min[anchor] - unc_setup - t_late + N
   + crpr_credit` (`anchor = check.edge` for FF-D, `edge0_pol` for
   async; `t_late` = `t_setup` or `t_recovery`), latch-D the clamped
   borrow window of section 1, `slack = req - arr`.
3. else multicycle covering (s, e): existing req shift.
4. min side: min_delay covering (s, e) -> FF-D/async-check `req =
   req_early_base + N` (the pre-MCP hold or removal required) and
   latch-D `req = N + lat0 + t_hold + unc_hold`, both reported against
   `w.dly` with no launch-parity realignment; else existing
   hold/multicycle handling.

Output-port endpoints compose the same way, with the edge-referenced
required times of section 1 substituted in steps 2 and 4
(`req = N - output_delay_max - unc_setup`, `req = N - output_delay_min +
unc_hold` — no capture term, so nothing is read from `check_by_pin`).

Multiple max_delay (or min_delay) rules on overlapping pairs fold by
**file-order last-wins per (from, to) signature** before composition.
False-path precedence over delay is measured (`md_fp`); delay-over-multicycle
is order-independent and measured (`md_mc`, `md_mc_rev`,
`md_min_mcp_h1`, `md_min_mcp_s2`).

## 3. Scope

**v1 supports**

- `set_max_delay <signed f32>` / `set_min_delay <signed f32>` — positional
  delay value — with `-from`/`-to` object selectors (full `ObjSel` reuse:
  cells, ports, `all_registers`, `all_inputs`, `all_outputs`, `get_pins`,
  bare `inst/pin`, `remove_from_collection` semantics identical to
  exceptions).
- Endpoint kind `Check { .. }` carrying a check arc — flip-flop D-pin
  setup/hold, asynchronous recovery/removal, and latch-D checks
  (`delay_*` committed scenarios on the `async_reset`/`latch_borrow`
  fixtures).
- Endpoint kind `OutputPort` — the edge-referenced formulas in section 1
  (`mdo_*`/`pmo_*` committed scenarios).
- Universal (no `-from`/`-to`) rules.
- Interaction with false_path / multicycle per section 1 precedence.
- Propagated and ideal clocks.

**v1 diagnoses (ignored_commands / dedicated counters, never silent)**

- Edge qualifiers `-rise`, `-fall`, `-rise_from`, `-fall_from`, `-rise_to`,
  `-fall_to`, `-edge` — per-transition machinery exists but qualifier
  semantics need dedicated probing; deferred. These get a **dedicated**
  unsupported-flag list for delay commands (the exception flag list does not
  cover them).
- `-early`, `-late` — OpenSTA rejects these for delay commands (`STA-0563`,
  `md_early` committed); counted ignored for uniformity.
- `-through`, `-start`, `-end`, `-points`, `-lib_cell`, `-pin` —
  path-membership selectors we cannot resolve.
- `-clock <name>` (consumes its argument), `-datapath_only` — OpenSTA
  rejects these anyway (`STA-0563`); counted ignored.
- Delay constraints reaching `ClockGate` endpoints: counted in the
  `unsupported_path_delay_endpoints` skip field (same pattern as
  `unsupported_multicycle_endpoints`); the endpoint keeps its clock-check
  values — loudly partial, never silent.

**Explicitly out of scope (documented, not approximated)**

- Clock-gate-enable delay checks.
- `-rise`/`-fall` and other edge qualifiers.

## 4. Validation plan

Golden scenarios in `exceptions.reference.json` (111 entries: 107 evaluated +
4 OpenSTA error records): `md_*` ideal-clock set including order/precedence
variants (`md_mc_rev`, `md_overlap_rev`, `md_min_mc*`, `md_min_mcp_*`),
`mhu_*` uncertainty variants, `base_prop` / `pmd_*` propagated-clock set,
`md_early` / `md_datapath` OpenSTA-rejection records, `md_through`
qualifier-diagnostic coverage, the `m2_*`/`pm2_*` two-source set, and the
`mdo_*`/`pmo_*` output-port set (N sweeps, min-side attribution probes,
`all_outputs`/`-from` coverage, precedence, and propagated-clock anchors —
section 1), the `ml_*`/`pml_*` multi-launch port set (merged two-source
checks at `y2`, per-source probes, `all_outputs`, min side — section 1),
and the `pmp_*` propagated-clock port-launch set (`set_max_delay` /
`set_min_delay` `-from [get_ports d_in]`, the `-from [all_inputs]` sweep,
and the network-latency null-diff probe — section 1). The `pcr_*` set
adds clock-net early/late spread via `set_timing_derate -early 0.80` /
`-late 1.20` on `-clock -cell_delay` (spread `S = 0.4 * d(u_clkb)` =
0.031507 ns at the `c1` driver): `pcr_clk` proves the spread by the
opposite-direction `max_req`-down/`min_req`-up pair, `pcr_clkfp` forces
the shared-tree `u_ff0` launch to win `u_ff1/D` (per-path credit on the
ordinary check), `pcr_max`/`pcr_min` measure the credit on the delay
checks themselves, and `pcr_off` is the non-shared-path control.

Since 2026-11-19 the fixture drives `u_ff1/D` from a `mux2` with two FF
inputs (`u_ff0`, `u_ff2`) and a constrained input port (`sel`), assigns
`u_ff0` the higher-capacitance mux input `A1` (the larger pure delay,
~13.5 ps over `A0`), and delays `u_ff2`'s clock by an extra buffer stage
(`c2 -> u_clkb3 -> c3`, ~81 ps of added launch delay — `u_ff2`'s launch
lands ~69 ps after `u_ff0`'s on `c1`). `m2_both`/`pm2_both` merge two
covered sources in one delay check (a single `-from {u_ff0 u_ff2}`
command — the signature-class form that actually merges); `pm2_both` under
`set_propagated_clock` showed the required time anchors to the **capture**
clock pin, not the launch's. `pm2_min2` covers the min side under skewed
launches. `pmd_multi` (two commands) and `md_nofrom` (universal `-from`)
became multi-source delay scenarios for free. A second `mux2` (`u_mux2`,
same A0/A1 orientation) drives the new output port `y2` from `u_ff0/Q` and
`u_ff2/Q` — with `sel` on its select — reproducing the diverging-winner
geometry at a port for the `ml_*`/`pml_*` scenarios.

Honest coverage note: the merged delay check runs on golden data with the
delay/arrival winners split — `pm2_both` reports the arrival winner
`u_ff2` (0.786764) where `pm2_prop` reports `u_ff0`'s isolated delay check
(0.731424), while the pure-delay order is `u_ff0` > `u_ff2` (`m2_max0`
1.277188 vs `m2_max2` 1.263656 under a shared ideal launch). The 55.3 ps
split falsified the earlier delay-winner pairing — `arr = delay_launch +
delay_max` went red by exactly that gap — and established that OpenSTA
reports the covered-source arrival winner (a wrong-arrival mutation was
also checked red, per the fail-proof rule). The `pmp_*` set exercises
the propagated input-port launch anchor:
`pmp_port`/`pmp_minport` report the ordinary propagated port seed
(0.8/0.1) on the delay check and `pmp_port2`'s null diff proves the
anchor carries no network-latency term — with the caveat that the SDC
`-source` latency and the propagated `clk`-port-pin arrival coincide
numerically here because `clk` is the propagation root. On the port side
the same rule is now exercised per source: `y2`
is driven from two FF launches through `u_mux2`, and `ml_both` /
`pml_both` merge them in one delay check against the per-source probes
`ml_max0`/`ml_max2` (see the coverage note in section 1).

## 5. Open questions / follow-ups

Resolved by review + additional probing (2026-11-18):

- ~~max-side launch reference~~ — corrected to raw clock-pin arrival with
  explicit `- unc_setup - t_setup` terms (review S1; verified numerically).
- ~~min-side anchoring~~ — `req_hold_base + N` verified under propagated
  clock (`pmd_min`); MCP-independence proven (`md_min_mcp_*`).
- ~~command-order precedence~~ — order-independent delay-over-MCP,
  file-order last-wins within a constraint type (new golden scenarios).

Resolved by the v1 implementation (2026-11-18; superseded — the
`delay_launch`/`delay_max` machinery named below was retired with the
delay domain on 2026-11-19, see section 2; the signature-class split
stays):

- ~~`delay_launch` seed pairing~~ — the late clock-pin arrival
  (`clk_state.arrival_max[edge]`) for sequential launches; verified against
  `pmd_*`/`md_*` goldens.
- ~~Port launch reference~~ — `input_clock_latency_ns`, the same I/O clock
  anchor the port's own input delay uses (`md_port`/`md_minport` match;
  the propagated-clock anchor is measured by `pmp_*`, resolved below).
- ~~`delay_max` tie-breaking~~ — keep-first implemented, matching the
  arrival merge order; OpenSTA tie-break remains unmeasured.
- ~~Signature-class split~~ — implemented as chosen: delay coverage joins
  the exception signature.

Resolved by the two-source fixture (2026-11-19):

- ~~two-driver fixture for the delay merge~~ — `m2_both`/`pm2_both` merge
  two covered sources in one delay check on golden data; `pmd_multi` and
  `md_nofrom` are multi-source for free.
- ~~skewed-launch `set_min_delay` anchoring~~ — `pm2_min2` covers a min
  delay check whose launch (`u_ff2` on `c3`) is skewed ~69 ps from the
  capture clock (`c1`); a literal two-clock-domain fixture stays open.
- ~~max-side launch-vs-capture anchoring~~ — `pm2_both` measured the
  required time anchored to the endpoint's own clock-pin early arrival
  (~8.5 ps skew then, ~69 ps now); fixed req composition accordingly.

Resolved by the diverging-winner regen (2026-11-19):

- ~~delay-vs-arrival winner pairing~~ — `u_ff0` owns the larger pure delay
  (~13.5 ps via `A1`) while `u_ff2`'s extra clock-buffer stage makes it
  the arrival winner (~69 ps launch skew, 55.3 ps report split on
  `pm2_both` vs `pm2_prop`). The golden falsified the
  `delay_launch_max + delay_max` composition: OpenSTA reports the
  covered-source arrival winner, so the delay domain was retired and the
  delay check now keeps the covered candidate's ordinary arrival.

Resolved by the output-port probe set (2026-11-19):

- ~~output-port delay semantics~~ — edge-referenced required times measured
  and implemented: `req_max = N - output_delay_max - unc_setup`,
  `req_min = N - output_delay_min + unc_hold` (`mdo_*` sweeps,
  `mdo_mindl`/`mdo_minhu` attribution); `set_propagated_clock` does not
  move the anchor (`pmo_*` identical to the ideal required times).

Resolved by the multi-launch port fixture (2026-11-19):

- ~~multi-driver output-port winner rule~~ — `y2` (`u_mux2` from
  `u_ff0/Q`/`u_ff2/Q`) exercises the covered-candidate winner per source:
  `ml_both` reports the `u_ff0` candidate (decomposed by `ml_max0`/
  `ml_max2`, 16.2 ps split), `pml_both` the propagated arrival winner
  `u_ff2`; `ml_min`/`pml_min` cover the merged min-side check and
  `ml_all` the `-to [all_outputs]` selector.

Resolved by the propagated port-launch set (`pmp_*`, 2026-11-19):

- ~~propagated-clock input-port launch anchor~~ — the delay check shares
  the port's ordinary propagated arrival (`pmp_port`: `u_ff0/D` arr 0.8;
  `pmp_minport`: arr 0.1), i.e. the SDC `-source` latency anchor our
  `input_clock_latency_ns` already returns — the ideal-latency
  (arr 1.4) and capture-pin-propagated (arr ~0.87) anchors are
  falsified, and `pmp_port2` (network latency 0.6 -> 1.1) is
  byte-identical, so no network term is carried. Caveat recorded in
  section 1: `-source` latency and the propagated `clk`-port-pin arrival
  are numerically identical on this fixture (`clk` is the propagation
  root) — the two named candidates were not separable. `pmp_alls`
  (`-from [all_inputs]`) additionally covers the port launch at the
  skewed `u_ff2` capture pin and the `sel` launch at `u_ff1/D` and `y2`
  (`y2` reports a real −0.683130 delay-check violation); `clk` is inert
  as a launch source in both engines.

Resolved by the `pcr_*` spread fixture (2026-11-19):

- ~~CRPR credit on delay checks~~ — `set_timing_derate -early 0.80` /
  `-late 1.20 -clock -cell_delay` puts `S = 0.031507` of spread on the
  `c1`/`c3` drivers (`pcr_clk` opposite-direction required-time pair).
  OpenSTA credits delay checks the same shared-net driver spread the
  clock check adds (`pcr_max` req `+S`, `pcr_min` req `-S`; `pcr_off`
  non-shared control `+0`) — and it credits **per path**: `pcr_clkfp`
  shows the winning `u_ff0` launch credited on `u_ff1/D` even though
  the endpoint's other sources are not shared. Implemented as
  per-extreme launch-tag masks (`crpr_max`/`crpr_min`, interned
  `(clock net, clock edge)` bits, sentinel for non-FF/contested
  sources) replacing the all-source cone consensus; the delay check
  adds the same credit term. See `docs/results.md`.

Resolved by the async/latch delay probe set (2026-11-19):

- ~~async-reset endpoint probe~~ — OpenSTA **applies** `set_max_delay`/
  `set_min_delay` on recovery/removal pins: recovery plays `late_arc`,
  removal `early_arc`, and the required anchors at the first clock edge
  (`edge0`), not `check.edge`. Committed: `delay_max`, `delay_min`,
  `delay_from`, `delay_cell`, `delay_prop` on `sta_edge/async_reset`.
- ~~latch-borrow delay fixture~~ — OpenSTA **applies** both sides with
  borrow semantics at the delay edge: `req = N + lat0 - unc +
  clamp(arr_dly - base, 0, pulse - t_setup)` (max) and `req = N + lat0 +
  t_hold + unc_hold` (min); covered `set_max_delay` `-to` selectors
  silence the `D -> Q` borrow arc for the whole signature class;
  through-latch launches re-base their arrival to the delay edge.
  Committed: `delay_max`, `delay_min`, `delay_ln`, `delay_from`,
  `delay_allregs`, `delay_prop` on `sta_edge/latch_borrow`.

Known gaps in that resolution (reachable but ungoldened — review
2026-11-19):

- the `D -> Q` borrow cap clamps `dly_max` through the same
  absolute-time window as `arrival_max` (`open + k*period + pulse -
  unc - t_setup`). Exact wherever `dly == arr` — every goldened case,
  since covered launches are port/flop seeds — but a through-latch
  launch feeds a launch-edge-relative `dly` into the absolute
  window; whether OpenSTA re-bases before clamping is unverified.
  The fixture has no latch-Q -> latch-D chain, so no covered
  endpoint reports a borrow-capped `dly`.
- the delay check's CRPR credit reads `crpr_max`/`crpr_min` — the
  *arrival* winner's launch tag — while the reported `arr` is the
  independently merged `dly` winner. The winners diverge only via a
  through-latch launch and the credit is nonzero only under
  `set_propagated_clock` + `set_timing_derate`; `pcr_*` has no
  latches and `latch_borrow` no derate, so the divergent case is
  ungoldened.
- `set_min_delay` is golden-verified only on `dlxtp` latch-D
  (`u_lp/D`) and `dfrtp` `RESET_B` (`u_ff0/RESET_B`); the same code
  serves `dlxtn` latch-D and latch-cell `RESET_B` removal
  (`u_rbn`/`u_rbp`/`u_rt*`, covered max-side only by
  `delay_allregs`) unverified.

Still open:

1. A literal two-clock-domain `set_min_delay` anchoring fixture —
   answered and covered by `mc-xdomain` (v0.2.2, 2026-11-20): on a
   second-domain endpoint OpenSTA anchors the delay-check required time
   at the **capture domain's** edge-0 arrival — `req = N +
   capture_latency ∓ capture_uncertainty ∓ t_check`, with the launch
   domain's latency on the arrival side only (`t2_md`/`t2_mdl`/
   `t2_mdx`, `docs/multi-clock.md` §1.5). `t2_mdx` additionally measured
   the `get_pins inst/Q` `-from` startpoint form: the pin replaces its
   clocked launch with pin-segment paths (`arr` from the pin,
   `req = N ∓ constraint`, no clock anchor). `mc-selectors` (v0.2.4)
   added the internal-path-endpoint form: a `-from [get_clocks cka]`
   delay rule covering a dangling launch reports the leaf as
   `u_fy/Q (internal path endpoint)` with `req = N` and no clock terms
   (`t2_selmd`, `docs/multi-clock.md` §1.7). Golden coverage runs at
   STRICT in `test_sta_clocks_match_opensta`; formal promotion to
   `two_clock.reference.json` stays with `mc-golden`.
