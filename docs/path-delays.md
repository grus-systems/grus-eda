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

Required time is **replaced**, not tightened:

```
req_max_delay = launch_arrival + N
```

- `launch_arrival` = the arrival of the launching clock edge **at the source's
  clock pin** for that edge instance and clock transition. Measured on
  `u_ff0 -> u_ff1/D`: launch = 0.562147 (transition A) / 0.606605 (transition
  B); these equal `setup_req - period` in this fixture and are consistent with
  clock-pin arrivals (the 0.044 gap is the clock rise/fall arrival skew).
- Replace semantics proven by `md_loose` (`N = 5.0` >> setup budget): req became
  `0.562147 + 5.0 = 5.562147`, i.e. the looser constraint still replaced the
  clock check.
- Input-port sources work identically: `md_port` (`-from [get_ports d_in]`)
  gave `req = 0.528237 + 1.0 = 1.528237` on `u_ff0/D`, where 0.528237 is the
  launch reference for the port (the `core_clk` edge reference the input delay
  is anchored to — exact anchor TBD during implementation, candidate: clock
  arrival at the `clk` port pin).

### `set_min_delay N` (hold side)

Required time is the hold required **plus** N:

```
req_min_delay = req_hold + N
```

Measured across N in {0.0, 0.5, 1.0}, hold-uncertainty 0.1 and 0.5, and both
FF and port sources — every point satisfies `req_new = req_hold + N` exactly.
Whether OpenSTA's true formula is `launch + N + hold_margin` or literally
`req_hold + N` is indistinguishable on sky130 (hold margins ~0); we implement
the empirical rule and document the assumption.

### Output ports (`-to [get_ports ...]`)

Different anchor, measured but **out of v1 scope** (see section 4):

- max side: `req = N - output_delay_max - setup_uncertainty` (edge reference 0)
- min side: `req ~= N` (exact additive constant TBD)

Scenarios `md_toy*` were probed then removed from the golden set for v1.

### Selectors

Identical machinery to timing exceptions: `-from`/`-to` accept the same object
selectors (`get_cells`, `get_ports`, `all_registers`, `all_inputs`,
`all_registers`-style collections, `get_pins inst/pin`, bare `inst/pin`).

- `all_registers` covers sequential cells only — input-port launches are not
  covered (same as exceptions; `md_fromall` confirmed: port-launched endpoints
  kept clock checks while reg-launched ones got delay checks).
- `get_pins u_ff0/CLK` selects the launch source; `get_pins u_ff1/D` the
  endpoint — same pin semantics as exceptions.
- Omitted `-from` or `-to` = universal on that side.
- Unknown/empty selectors resolve empty (dead constraint), counted via
  `unresolved_exception_selectors`-style diagnostics.

### Multiple rules and precedence

- Two `set_max_delay` commands on the same (from, to) pair: **last wins**
  (`md_overlap`: 1.0 then 0.5 -> req used 0.5).
- `set_max_delay` and `set_min_delay` on the same pair coexist on different
  sides (`md_both`).
- `set_false_path` on the same pair removes the endpoint entirely — false
  path wins over delay constraints (`md_fp`).
- `set_multicycle_path` + `set_max_delay` on the same pair: the reported max
  required equals the pure delay value `launch + N` (`md_mc`), i.e. the delay
  check replaces whatever the multicycle-adjusted check would have been.
  Whether the MCP shift still applies "underneath" is unobservable (dominated);
  v1 applies delay **instead of** the clock-check candidate for covered pairs.

### Qualifiers (OpenSTA acceptance, measured)

| qualifier | OpenSTA result |
|---|---|
| `-rise` / `-fall` | accepted; selects the matching data transition's check (`md_maxrise` changed the reported transition) |
| `-through` | accepted; path filter — non-matching paths unaffected (`md_through` -> baseline) |
| `-early` / `-late` | **rejected** `STA-0563` |
| `-datapath_only` | **rejected** `STA-0563` |
| `-clock` | **rejected** `STA-0563` |

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
- `delay_launch_max: [f32; 2]` — the launch reference paired with the
  `delay_max` winner (clock-pin arrival for FF/latch seeds; I/O clock
  reference for port seeds).

Merge semantics mirror `arrival_max`: independent winner tracking (the
delay-max path may differ from the arrival-max path), ties keep the existing
launch (documented; exact tie-break vs OpenSTA unmeasured — flagged for
reviewers).

Reported endpoint values for a delay-covered (endpoint, transition):

```
arr_reported = delay_launch + delay_max     ( = launch + path_delay )
req_reported = delay_launch + N            (max side)
slack        = N - delay_max               ( = req - arr )
```

`min` side needs **no** new state: `req_min += N` on the existing hold
candidate per transition.

Slew needs no pairing: slew is a per-pin/per-transition quantity in our model
(and OpenSTA queries it at the endpoint vertex), identical for clock and
delay checks.

### Seeds

- FF `CLK -> Q` / latch enable -> Q: `delay = t_cq` (the arc's own derated
  delay), `delay_launch = clock_state[clk_pin].arrival_*[clk_transition]`
  (max or min side per check direction — exact choice verified against golden;
  candidate: the same arrival the launch uses for setup checks).
- Input ports: `delay = port_arrival - io_clock_ref`, `delay_launch =
  io_clock_ref` where `io_clock_ref` is the referenced clock's arrival at the
  clock port (0.528237 measured for `d_in` -> verify exact anchor in impl).

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

1. false_path covering (s, e): drop candidate (existing).
2. else max_delay covering (s, e): replace max-side candidate with delay check
   (`slack = N - delay_max[pol]`, `req = delay_launch + N`).
3. else multicycle covering (s, e): existing req shift.
4. min side independently: min_delay covering -> `req += N`; else existing
   hold/multicycle handling.

False-path precedence over delay is measured (`md_fp`); delay-over-multicycle
follows the measured `md_mc` result.

## 3. Scope

**v1 supports**

- `set_max_delay N` / `set_min_delay N`, bare value or `-setup`/`-hold`-free
  positional form, with `-from`/`-to` object selectors (full `ObjSel` reuse:
  cells, ports, `all_registers`, `all_inputs`, `get_pins`, bare `inst/pin`,
  `remove_from_collection` semantics identical to exceptions).
- Endpoint kind `Check { latch: false }` (flip-flop D pins) — both sides.
- Universal (no `-from`/`-to`) rules.
- Interaction with false_path / multicycle per section 1 precedence.
- Propagated and ideal clocks (delay-state propagation is clock-agnostic).

**v1 diagnoses (ignored_commands / dedicated counters, never silent)**

- Edge qualifiers `-rise`, `-fall`, `-rise_from`, `-fall_from`, `-rise_to`,
  `-fall_to`, `-early`, `-late`, `-edge` — per-transition machinery exists but
  qualifier semantics (which end, both-ends-same-transition?) need dedicated
  probing; deferred.
- `-through`, `-start`, `-end`, `-points`, `-lib_cell`, `-pin` — path-membership
  selectors we cannot resolve.
- `-clock`, `-datapath_only` — OpenSTA rejects these anyway (`STA-0563`); our
  parser counts them ignored for uniformity.
- Delay constraints reaching `OutputPort`, `ClockGate`, or latch-D endpoints:
  counted in a new `unsupported_path_delay_endpoints` skip field (same pattern
  as `unsupported_multicycle_endpoints`); the endpoint keeps its clock-check
  values — loudly partial, never silent.

**Explicitly out of scope (documented, not approximated)**

- Output-port delay semantics (measured formulas in section 1; need dedicated
  scenarios + multi-source probing before implementing).
- Latch-D and clock-gate delay checks.
- `-rise`/`-fall` and other edge qualifiers.

## 4. Validation plan

Golden scenarios already generated in `exceptions.reference.json` (57
scenarios): `md_*` ideal-clock set, `mhu_*` uncertainty variants, `base_prop` /
`pmd_*` propagated-clock set, `md_early` / `md_datapath` OpenSTA-rejection
records, `md_through` qualifier-diagnostic coverage.

Honest coverage note: every fixture endpoint has a **single** launch source,
so the delay-winner vs arrival-winner divergence (the reason `delay_max`
exists) is not exercised by golden data — it is covered by construction (the
state propagates per-path like arrival does) but not by a failing-test
demonstration. A two-driver fixture would close this; flagged as follow-up.

## 5. Open questions for review

1. `delay_launch` seed value: clock-pin `arrival_max` vs `arrival_min` per
   transition — which pairing reproduces OpenSTA's `req = launch + N`?
   (Plan: implement with `arrival_max` for max-side seeds, verify against
   `pmd_*`/`md_*` goldens; the propagated scenarios discriminate.)
2. Port launch reference: exact anchor for `io_clock_ref` (clk port arrival vs
   ideal edge + source latency). Candidate measured: 0.528237 for `d_in`.
3. Tie-breaking when two paths share `delay_max`: keep-first vs min-launch —
   unmeasured; pick keep-first (matches arrival merge) and document.
4. `req_min += N` vs a launch-anchored formula: indistinguishable on sky130;
   confirm no sky130 cell has hold margin large enough to discriminate, else
   probe a synthetic Liberty.
5. Whether delay coverage should split signature classes (chosen: yes) vs.
   computing delay state for all sources and masking at composition (rejected:
   per-source coverage info is lost in aggregates).
