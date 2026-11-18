# Path delay constraints (`set_max_delay` / `set_min_delay`)

Status: **implemented v1** (2026-11-18) — parser, delay-domain state and
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
arr_delay     = launch_clk_arrival_late + delay_max
slack         = req_max_delay - arr_delay
```

where `delay_max = arr_data - launch_clk_arrival` is the pure path delay
(clock arrival excluded) and the capture term is the endpoint register's
**own** clock-pin early arrival — the same `arrival_min[check.edge]` the
setup clock check builds its closing edge on, minus the period the clock
check adds. Verified numerically on `u_ff0 -> u_ff1/D` (ideal clock):
`0.7 - 0.05 - 0.087853 + 1.0 = 1.562147` — the two per-transition checks
differ only through `t_setup` (0.087853 rise / 0.043395 fall), sharing one
launch arrival. An earlier draft of this document misidentified
`setup_req - period` (0.562147) as the launch arrival; review corrected it
— the raw clock-pin arrival (0.7 ideal, ~0.173 propagated per `pmd_basic`)
is the launch reference and the uncertainty/setup-time terms are subtracted
explicitly.

The capture-vs-launch distinction is invisible while every clock pin shares
one net arrival — the split-clock-buffer fixture (`pm2_both`) separates
them: `u_ff1/D` reports `req = 1.013333` (anchored at `u_ff1/CLK` on `c1`,
~8.5 ps later than the delay winner `u_ff2`'s launch on `c2`), not the
launch-referenced `1.0048`. The CRPR credit the clock check adds is not
applied to delay checks (unexercised: the delay domain tracks the winner's
launch time, not its clock net, and this fixture's clock nets carry zero
late/early spread).

- Replace semantics proven by `md_loose` (`N = 5.0` >> setup budget): req
  became `launch - unc - t_setup + 5.0`, i.e. the looser constraint still
  replaced the clock check.
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
FF sources, and propagated clock (`pmd_min`: `0.257286 + 0.5 = 0.757286`
exact). Multicycle independence proven by `md_min_mcp_h1`/`md_min_mcp_s2`:
with `-hold 1` or `-setup 2` on the same pair, the delay required stayed
`0.778491 + 0.5` — MCP shifts do **not** feed into the delay check (the
delay check replaces the MCP-adjusted clock check entirely on that side).

Whether OpenSTA's true formula is literally `req_hold_base + N` or a
launch-anchored expression that coincides on sky130 is indistinguishable on
this fixture (hold margins ~0, single clock net); a dedicated skewed
two-clock fixture is listed as follow-up work in section 5.

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
- `set_false_path` on the same pair removes the endpoint entirely — false
  path wins over delay constraints (`md_fp`).
- Delay vs multicycle on the same pair is **order-independent**: the delay
  check replaces the clock check on its side regardless of command order
  (`md_mc`, `md_mc_rev` — max req stayed `launch - unc - t_setup + N`).
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

## 2. Why this needs new STA state

The delay check needs, per (class, endpoint, transition):

```
slack = N - max_over_covered_sources( arr_data(s) - launch(s) )
```

`arr_data(s) - launch(s)` is the **path delay excluding clock arrivals** — it
propagates exactly like arrival (same arc delays) but seeds at the cell arc
delay (`t_cq`) instead of `clk_arrival + t_cq`. Under a propagated clock,
different sources have different launch times, so the max-delay winner can
differ from the max-arrival winner — the existing aggregated `arrival_max`
cannot answer this. Under an ideal clock all launches coincide and the two
winners always agree, but we do not special-case that.

### Proposed state addition

`PinTimingState` gains two per-transition fields:

- `delay_max: [f32; 2]` — max over paths of `(arrival - launch)`, propagated
  with the same arc/net delays as `arrival_max` but seeded at arc delay only.
- `delay_launch_max: [f32; 2]` — the **raw** launch clock arrival paired with
  the `delay_max` winner (clock-pin arrival for FF/latch seeds; I/O clock
  reference for port seeds). Raw, not uncertainty/setup-adjusted — those
  terms are applied at the endpoint where `t_setup` is known.

Merge semantics mirror `arrival_max`: independent winner tracking (the
delay-max path may differ from the arrival-max path), ties keep the existing
launch (documented; exact tie-break vs OpenSTA unmeasured — flagged for
reviewers).

Reported endpoint values for a delay-covered (endpoint, transition `pol`),
with `t_setup` = the endpoint check arc's setup (late) time for `pol`:

```
arr_reported = delay_launch + delay_max          (the delay-winner's arrival)
req_reported = capture_arrival_min[check.edge] - unc_setup - t_setup + N
slack        = req_reported - arr_reported
```

with the capture arrival read at the endpoint register's own clock pin —
it coincides with `delay_launch` only when launch and capture clocks see
equal arrivals (measured on `pm2_both`).

`min` side needs **no** new state: the delay check uses the **pre-MCP** hold
required time — `req_min_delay = req_hold_base + N` per transition, applied
instead of the multicycle shift for covered pairs.

Slew needs no pairing: slew is a per-pin/per-transition quantity in our model
(and OpenSTA queries it at the endpoint vertex), identical for clock and
delay checks.

### Latch borrow interaction (review finding)

`eval_sta` clamps latch-borrow arrivals against absolute clock-edge arrivals.
Delay-domain values cannot be clamped directly, so at latch boundaries the
code reconstructs the absolute arrival as `delay_launch + delay_max`, applies
the same borrow clamp, and re-derives `delay_max`. The delay fields therefore
propagate through latch arcs with borrow semantics preserved; this must be
verified against an OpenSTA fixture containing a latch on a delay-constrained
path (fixture follow-up — the current `sta_exceptions` DEF has no latches).

### Seeds

- FF `CLK -> Q` / latch enable -> Q: `delay = t_cq` (the arc's own derated
  delay), `delay_launch = clock_state[clk_pin].arrival_*[edge]` — the raw
  clock-pin arrival for the launching edge instance and clock transition
  (max side uses the late arrival; exact min/max pairing verified against
  golden during implementation).
- Input ports: `delay = port_arrival - io_clock_ref`, `delay_launch =
  io_clock_ref` where `io_clock_ref` is the referenced clock's arrival at the
  clock port (0.528237-equivalent measured for `d_in`; `ClockRef::Virtual`
  fallback TBD).

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
2. else max_delay covering (s, e): replace max-side candidate with the delay
   check (`req = capture_clk_arrival_min[check.edge] - unc_setup - t_setup + N`,
   `arr = delay_launch_max[pol] + delay_max[pol]`, `slack = req - arr`).
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
- Propagated and ideal clocks (delay-state propagation is clock-agnostic).

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
inputs (`u_ff0`, `u_ff2`) and a constrained input port (`sel`), and splits
`u_ff2`'s clock onto a second `clkbuf` (`c2`). `m2_both`/`pm2_both` merge
two covered sources in one delay domain (a single `-from {u_ff0 u_ff2}`
command — the signature-class form that actually merges); `pm2_both` under
`set_propagated_clock` showed the required time anchors to the **capture**
clock pin, not the delay winner's launch (~8.5 ps skew between `c1`/`c2`).
`pm2_min2` covers the min side under skewed launches. `pmd_multi` (two
commands) and `md_nofrom` (universal `-from`) became multi-source delay
scenarios for free.

Honest coverage note: the merged delay domain now runs on golden data and
a deliberately mis-paired launch turns `pm2_both` red (fail-proof checked).
However the delay winner and the arrival winner did **not** diverge in this
fixture — `u_ff2`'s path delay advantage (~10 ps via `A1`) exceeds its
earlier launch (~8.5 ps on `c2`), so `u_ff2` wins both. A golden that
separates the two winners (e.g. a larger `c1` load) remains possible
follow-up; the pairing is verified for the mis-paired-launch direction
only. The delay check's CRPR credit is likewise unexercised (zero
late/early spread on this fixture's clock nets).

## 5. Open questions / follow-ups

Resolved by review + additional probing (2026-11-18):

- ~~max-side launch reference~~ — corrected to raw clock-pin arrival with
  explicit `- unc_setup - t_setup` terms (review S1; verified numerically).
- ~~min-side anchoring~~ — `req_hold_base + N` verified under propagated
  clock (`pmd_min`); MCP-independence proven (`md_min_mcp_*`).
- ~~command-order precedence~~ — order-independent delay-over-MCP,
  file-order last-wins within a constraint type (new golden scenarios).

Resolved by the v1 implementation (2026-11-18):

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
  two covered sources in one delay domain on golden data; `pmd_multi` and
  `md_nofrom` are multi-source for free. Diverging delay/arrival winners
  remain unexercised (section 4 note).
- ~~skewed-launch `set_min_delay` anchoring~~ — `pm2_min2` covers a min
  delay check whose launch (`u_ff2` on `c2`) is skewed from the capture
  clock (`c1`); a literal two-clock-domain fixture stays open.
- ~~max-side launch-vs-capture anchoring~~ — `pm2_both` measured the
  required time anchored to the endpoint's own clock-pin early arrival
  (~8.5 ps skew); fixed req composition accordingly.

Still open:

1. A literal two-clock-domain `set_min_delay` anchoring fixture;
   async-reset endpoint probe; latch-borrow delay fixture — all need
   DEF/SDC additions.
2. Output-port delay semantics (formulas measured ad hoc in section 1;
   dedicated scenarios + multi-source probing before implementing).
3. A fixture where the delay winner and the arrival winner actually
   diverge (skew > path-delay difference, e.g. a larger `c1` load), so the
   `delay_launch_max` pairing is discriminated on golden data in both
   directions; CRPR credit on delay checks needs a fixture with nonzero
   clock-net late/early spread.
4. Propagated-clock input-port launch anchor (see above).
