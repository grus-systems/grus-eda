# Path delay constraints (`set_max_delay` / `set_min_delay`)

Status: design probe complete, implementation pending. This document records the
OpenSTA semantics measured on the `sta_exceptions` fixture (see
`tests/designs/sta_exceptions/generate_ref.sh`) and the proposed implementation
shape. Nothing here is implemented yet.

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
launch-referenced **minus the same setup-side adjustments the clock check
uses**, plus N:

```
req_max_delay = launch_clk_arrival - setup_uncertainty - t_setup(endpoint, transition) + N
slack          = N - delay_max - setup_uncertainty - t_setup(endpoint, transition)
```

where `delay_max = arr_data - launch_clk_arrival` is the pure path delay
(clock arrival excluded). Verified numerically on `u_ff0 -> u_ff1/D`
(ideal clock): `0.7 - 0.05 - 0.087853 + 1.0 = 1.562147` — the two per-
transition checks differ only through `t_setup` (0.087853 rise / 0.043395
fall), sharing one launch arrival. An earlier draft of this document
misidentified `setup_req - period` (0.562147) as the launch arrival; review
corrected it — the raw clock-pin arrival (0.7 ideal, ~0.173 propagated per
`pmd_basic`) is the launch reference and the uncertainty/setup-time terms
are subtracted explicitly.

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
req_reported = delay_launch - unc_setup - t_setup + N
slack        = N - delay_max - unc_setup - t_setup   ( = req - arr )
```

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
   check (`slack = N - delay_max[pol] - unc_setup - t_setup`,
   `req = delay_launch - unc_setup - t_setup + N`).
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

Golden scenarios in `exceptions.reference.json` (65 entries: 61 evaluated +
4 OpenSTA error records): `md_*` ideal-clock set including order/precedence
variants (`md_mc_rev`, `md_overlap_rev`, `md_min_mc*`, `md_min_mcp_*`),
`mhu_*` uncertainty variants, `base_prop` / `pmd_*` propagated-clock set,
`md_early` / `md_datapath` OpenSTA-rejection records, `md_through`
qualifier-diagnostic coverage.

Honest coverage note: every fixture endpoint has a **single** launch source,
so the delay-winner vs arrival-winner divergence (the reason `delay_max`
exists) is not exercised by golden data — it is covered by construction (the
state propagates per-path like arrival does) but not by a failing-test
demonstration. A two-driver fixture would close this; flagged as follow-up.

## 5. Open questions / follow-ups

Resolved by review + additional probing (2026-11-18):

- ~~max-side launch reference~~ — corrected to raw clock-pin arrival with
  explicit `- unc_setup - t_setup` terms (review S1; verified numerically).
- ~~min-side anchoring~~ — `req_hold_base + N` verified under propagated
  clock (`pmd_min`); MCP-independence proven (`md_min_mcp_*`).
- ~~command-order precedence~~ — order-independent delay-over-MCP,
  file-order last-wins within a constraint type (new golden scenarios).

Still open:

1. `delay_launch` seed pairing: clock-pin `arrival_max` vs `arrival_min` per
   edge/transition — implement with the late arrival for max-side seeds,
   verify against `pmd_*`/`md_*` goldens.
2. Port launch reference: exact anchor for `io_clock_ref` (clk port arrival vs
   ideal edge + source latency; `ClockRef::Virtual` fallback). Candidate
   measured: 0.528237 for `d_in`.
3. Tie-breaking when two paths share `delay_max`: keep-first vs min-launch —
   unmeasured; pick keep-first (matches arrival merge) and document.
4. Skewed two-clock fixture for `set_min_delay` launch-vs-capture anchoring
   (single-clock-net fixture cannot fully discriminate); async-reset
   endpoint probe; latch-borrow delay fixture — all need DEF/SDC additions.
5. Output-port delay semantics (formulas measured ad hoc in section 1;
   dedicated scenarios + multi-source probing before implementing).
6. Whether delay coverage should split signature classes (chosen: yes) vs.
   computing delay state for all sources and masking at composition (rejected:
   per-source coverage info is lost in aggregates).
