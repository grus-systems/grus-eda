# Path delay constraints (`set_max_delay` / `set_min_delay`)

Status: **implemented v1** (2026-11-18) — parser and signature-class
composition live in `crates/grus-core/src/sta/{sdc,analysis}.rs`; every
committed `md_*`/`pmd_*`/`mhu_*` scenario matches OpenSTA
(`test_sta_exceptions_match_opensta`). This document records the OpenSTA
semantics measured on the `sta_exceptions` fixture (see
`tests/designs/sta_exceptions/generate_ref.sh`) and the implementation shape.
Sections 3's "v1 diagnoses" and "explicitly out of scope" lists still apply;
the follow-ups in section 5 are open.

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
credit the clock check adds is not applied to delay checks (unexercised:
the per-class merge does not keep the winning path's clock net, and this
fixture's clock nets carry zero late/early spread).

- Replace semantics proven by `md_loose` (`N = 5.0` >> setup budget): the
  covered `u_ff0` source's req became `capture - unc - t_setup + 5.0` —
  slack so loose the report's min-slack pick now falls to a surviving
  `u_ff2`/`sel` clock check (reported req 2.550057). On the pre-mux
  fixture `u_ff0` was the only source and the report showed the delay
  check itself (`1.562147`-style req); the replacement is still exercised
  by `md_tight`/`md_basic`, where the delay check wins.
- Input-port sources work identically: `md_port` (`-from [get_ports d_in]`)
  gave `req = 0.528237 + 1.0 - adjustments` on `u_ff0/D`; the port launch
  reference is the I/O clock edge the input delay is anchored to (exact
  anchor TBD during implementation — candidate: referenced clock's arrival
  at the clock port pin; `ClockRef::Virtual` needs a defined fallback).

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

Different anchor, measured ad hoc but **out of v1 scope** (see section 3):

- max side: `req = N - output_delay_max - setup_uncertainty` (edge reference 0;
  verified at N = 0.5/1.0/2.0)
- min side: `req ~= N` (exact additive constant TBD)

The `md_toy*` probes were run ad hoc and intentionally left out of the
committed golden set for v1 (same for `md_fromall`, `md_maxrise`, `md_fall` —
see section 3 scope notes).

### Selectors

Identical machinery to timing exceptions: `-from`/`-to` accept the same object
selectors (`get_cells`, `get_ports`, `all_registers`, `all_inputs`,
`all_registers`-style collections, `get_pins inst/pin`, bare `inst/pin`).

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
slack        = req_reported - arr_reported
```

with the capture arrival read at the endpoint register's own clock pin —
it coincides with a launch reference only when launch and capture clocks
see equal arrivals. No per-pin delay domain is needed: the reported
arrival is the covered-source **arrival** winner, which the
signature-class eval run computes directly.

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
2. else max_delay covering (s, e): keep the covered candidate's ordinary
   arrival and slew, replace its required time with
   `req = capture_clk_arrival_min[check.edge] - unc_setup - t_setup + N`,
   `slack = req - arr`.
3. else multicycle covering (s, e): existing req shift.
4. min side: min_delay covering (s, e) -> `req = req_hold_base + N` (the
   pre-MCP hold required); else existing hold/multicycle handling.

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
- Endpoint kind `Check { latch: false }` for **flip-flop D-pin setup/hold
  checks only** — asynchronous recovery/removal checks share that variant
  and are excluded (counted in `unsupported_path_delay_endpoints`).
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
- Delay constraints reaching `OutputPort`, `ClockGate`, latch-D, or
  async-recovery/removal endpoints: counted in a new
  `unsupported_path_delay_endpoints` skip field (same pattern as
  `unsupported_multicycle_endpoints`); the endpoint keeps its clock-check
  values — loudly partial, never silent.

**Explicitly out of scope (documented, not approximated)**

- Output-port delay semantics (measured formulas in section 1; need dedicated
  scenarios + multi-source probing before implementing).
- Latch-D and clock-gate delay checks.
- `-rise`/`-fall` and other edge qualifiers.

## 4. Validation plan

Golden scenarios in `exceptions.reference.json` (72 entries: 68 evaluated +
4 OpenSTA error records): `md_*` ideal-clock set including order/precedence
variants (`md_mc_rev`, `md_overlap_rev`, `md_min_mc*`, `md_min_mcp_*`),
`mhu_*` uncertainty variants, `base_prop` / `pmd_*` propagated-clock set,
`md_early` / `md_datapath` OpenSTA-rejection records, `md_through`
qualifier-diagnostic coverage, and the `m2_*`/`pm2_*` two-source set.

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
became multi-source delay scenarios for free.

Honest coverage note: the merged delay check runs on golden data with the
delay/arrival winners split — `pm2_both` reports the arrival winner
`u_ff2` (0.786764) where `pm2_prop` reports `u_ff0`'s isolated delay check
(0.731424), while the pure-delay order is `u_ff0` > `u_ff2` (`m2_max0`
1.277188 vs `m2_max2` 1.263656 under a shared ideal launch). The 55.3 ps
split falsified the earlier delay-winner pairing — `arr = delay_launch +
delay_max` went red by exactly that gap — and established that OpenSTA
reports the covered-source arrival winner (a wrong-arrival mutation was
also checked red, per the fail-proof rule). The delay check's CRPR credit
remains unexercised (zero late/early spread on this fixture's clock
nets).

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
  anchor the port's own input delay uses (`md_port`/`md_minport` match). The
  propagated-clock port anchor (source latency vs propagated clock-port
  arrival) is unverified — no golden exercises it.
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

Still open:

1. A literal two-clock-domain `set_min_delay` anchoring fixture;
   async-reset endpoint probe; latch-borrow delay fixture — all need
   DEF/SDC additions.
2. Output-port delay semantics (formulas measured ad hoc in section 1;
   dedicated scenarios + multi-source probing before implementing).
3. CRPR credit on delay checks needs a fixture with nonzero clock-net
   late/early spread.
4. Propagated-clock input-port launch anchor (see above).
