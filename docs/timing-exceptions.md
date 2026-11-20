# Timing exceptions: `set_false_path` / `set_multicycle_path`

Design note for the N2 deferred item, revised after independent review
(Gemini 4 Hydrogen-high and SWE-2 Flash via subcon; both verdicts: core
approach sound, specification below incorporates their findings).

Today both commands are hard parse errors (`sdc.rs` test asserts
`parse_sdc("set_false_path ...").is_err()`), which blocks any stock SDC that
uses them.

## The problem

Our STA collapses each pin's timing to one worst arrival/required pair
(`PinTimingState`: per-pin rise/fall max/min plus a launch-edge-parity
bitmask). It does not retain *which* launch object produced an endpoint's
worst arrival. SDC exceptions are defined per (launch, capture) path pair,
so a `-from [get_cells u1]` exception cannot be applied to an aggregated
endpoint: filtering on the collapsed value could hide a real violation or
erase an unrelated path.

## Approach: partitioned re-analysis ("multi-run")

Run the existing engine once per launch-source *signature class* and compose
the endpoint results. The timing core's algorithms stay identical; the only
change is a launch filter plus a prepare/eval split.

### Launch sources and keys

Every path starts at one of three source kinds, all identifiable by `PinId`:

- primary-input port seeding (`state[pin]` written directly for non-clock
  inputs)
- `TimingGraph.launches` — FF `CLK -> Q` arcs; source key = `l.q` pin
- `TimingGraph.latch_opens` — latch enable-to-Q arcs; source key = `o.q` pin

Latch `D -> Q` borrow arcs are never filtered: they propagate an existing
arrival through a transparent latch and create no new path origin.

A `LaunchKey` is the `PinId` of the launched pin (Q pin, or the I/O pin for
ports). `-from [get_cells u1]` resolves to every launch key whose pin belongs
to instance `u1`; `[all_registers]` resolves to all FF/latch launch keys;
`[all_inputs]`/`[get_ports p]` resolve to seeded (non-clock) input-port keys.
Selectors resolving to zero launch keys are diagnosed, not silently ignored.

### Signature-class partition (replaces per-source runs)

For each parsed exception `ex`, let `from(ex)` be its resolved launch-key set
(or "universal" when it provably covers **every** launch source *including
input ports* — e.g. missing `-from`; `all_registers`/`all_clocks` alone do
NOT qualify because port launches exist).

Each launch key `s` gets a signature `σ(s)` = the set of exceptions whose
`from` contains `s`. Keys with identical signatures are merged into one run:
for every endpoint they have identical applicable-exception sets, so their
worst arrival can be taken jointly. Runs needed = number of distinct
signatures (the empty signature is the default group).

This collapses the common cases: `set_false_path -from [all_registers] ...`
puts every register launch in one class and ports in the default → 2 runs,
not ~1600. A pathological SDC naming every register differently still
degrades to many runs — correct but slow; measured on `picorv32`/`aes`
before shipping (see Cost).

### Prepare / eval split

`run_sta_detail` splits into:

- **prepare** (once): `Context`, `build_graph`, `topological_levels`,
  `pi_models`, `wire_delays_per_net`, `propagate_slew`,
  `propagate_clock`/`ideal_clock_parity` + clock-consumer seeding. All of
  these are arrival-filter-independent by construction (verified: clock
  propagation reads only clock-network state and port *slews*).
- **eval** (per signature class): data-pass propagation + `collect_endpoints`.

### The filter suppresses arrivals only — never slews

`PinTimingState::merge` already merges `slew_max`/`slew_min` independently
of which arrival wins (OpenSTA Graph-Based Analysis). Exceptions must not
change slews: gate delays and NLDM constraint lookups are slew-indexed, so
dropping a sibling source's slew would corrupt every downstream delay —
the review's key correctness finding.

Implementation: excluded sources still merge **slew only** — call
`merge(pol, NEG_INFINITY, INFINITY, slew_max, slew_min, 0, 0)`, which updates
just the slew fields. Concretely:

- port seeding: always merge the `port_drive` slews; merge the arrival only
  for allowed ports
- `launch_updates`/`latch_updates`: evaluate every launch (its slew feeds
  `seq_in` anyway); merge arrival fields only for allowed keys, slew always

With slews unfiltered, endpoint-pin slews — and therefore NLDM constraint
times — are identical across runs, restoring the required-time invariance
the first draft assumed (capture clock side was never filtered; data-side
slew now isn't either). Launch-parity effects on required time remain
per-arrival and are handled by keeping per-polarity candidates.

Clock-pin seeding stays unfiltered in every run: checks need capture-clock
arrivals regardless of which data sources launch.

### Endpoint identity and `-to` resolution

Endpoints are `PinId`-keyed internally: `graph.checks` data pins (setup/hold
plus recovery/removal on async pins, latch D included), `gating_enables`
enable pins (`EndpointKind::ClockGate`), and output `io_pins`. `run_sta_detail`
must return polarities keyed by `PinId` (today `Vec<EndpointPolarities>` is
parallel to the pin-name-sorted endpoints; add the id or a map).

`-to` selector resolution:

- `[get_cells u]` → every endpoint pin of instance `u`: check pins **and**
  clock-gate enable pins owned by `u`
- `[all_registers]` → check pins of sequential cells (FF + latch), **not**
  clock-gate enables, **not** output ports
- `[get_ports p]`/`[all_outputs]` → output-port endpoints; an input port or
  unknown name resolves empty → diagnostic
- missing `-to` → all endpoints

### Composition

For each endpoint `E`, collect each signature group's `EndpointPolarities`
(per-polarity `Worst{arr, req, slack, slew}` for max and min sides — compose
on these, **not** the collapsed `EndpointReport`, so the winning group's
full triple is reported consistently). Apply every exception whose `-to`
matches `E`:

- **false_path**: drop every group whose signature intersects the
  exception's `from`. A universal `from` drops all groups → the endpoint
  leaves the report, counted in `SkipReport::false_path_endpoints`
  (exempt from `is_clean` like design rules, still shown by `nonzero()`).
- **multicycle_path**: for groups inside `from`, shift the required time by
  the measured OpenSTA convention (see below) and re-derive slack; take the
  worst surviving (group, polarity) candidate per side.
- **precedence**: false_path beats multicycle on the same (source,
  endpoint). Overlapping multicycles: rule measured on OpenSTA, with a
  stated deterministic fallback — never guessed.

Per-run `SkipReport`s are **discarded**; the composed report's skips are
computed once on the composed endpoint set (an endpoint is unreached only
if no surviving group produced a candidate). Run-invariant diagnostics
(`unresolved_driving_cells`, etc.) are taken from one run.

### Multicycle semantics — measured on OpenSTA

Probe: `tests/designs/sta_exceptions/` (3-FF design with `u_ff1/D` driven
by a `mux2` from `u_ff0`/`u_ff2` plus a `sel` input, `u_ff2`'s clock split
onto a second buffer stage `c2 -> u_clkb3 -> c3`, and a second `mux2`
driving output `y2` from the same two FF sources, 111 SDC scenarios,
`generate_ref.sh` inside the pinned LibreLane image →
`exceptions.reference.json`). Period `T = 2.0 ns`. Required-time
shifts measured per endpoint (`u_ff1/D`, the only MCP target):

- **setup**: `max_req += (N_setup - 1) * T` when the exception matches.
- **hold**: `min_req += ((N_setup - 1) - H_hold) * T`, defaults
  `N_setup = 1`, `H_hold = 0`. All seven measured points fit:
  `-setup 2` alone → hold `+1T`; `-hold 1` alone → hold `-1T`;
  `-hold 0` → `0`; `-hold -1` → `+1T`; `-setup 2` + `-hold 1` (two
  commands) → `0`; `-setup 3` overwriting `-setup 2` → `+2T`.
- Bare positional `set_multicycle_path 2` and `-setup 2` are identical;
  a bare `N` means setup. `-hold`/`-setup` values may be `0` or negative
  (hold only; semantics above).
- **Precedence**: a later exception on the same path replaces an earlier
  one (`-setup 2` then `-setup 3` → `N_setup = 3`). `set_false_path`
  beats `set_multicycle_path` on the same path — the covered source's
  candidate disappears; the endpoint itself drops only when every launch
  source is covered (`u_ff1/D` survives `fp_uf0_uf1` via `u_ff2`/`sel`,
  `fp_to_y` removes `y`).
- **Endpoint coverage**: MCP applies to FF-D checks *and* output-port
  checks (`-to [get_ports y]` shifted `y` req by `+1T` on both sides).
  `-from` omitted = all launch sources.
- **OpenSTA parser limits (measured, not assumed)**: `-setup X -hold Y`
  on one command fails (`can't read "path_multiplier"`); `-both N`
  fails (`STA-0563`). The combined `-setup`/`-hold` form is valid SDC
  with semantics identical to the split form, so our parser accepts it.
  `-both` is counted in `ignored_commands` instead: valid SDC, but its
  hold-side semantics cannot be verified against OpenSTA.
- `set_false_path -from [all_registers]` does **not** cover primary-input
  launches (`fp_allregs` left the `d_in`→D hold violations intact);
  `-from [get_ports d_in]` removed exactly those endpoints. Confirms the
  signature-class design: `all_registers` is not a universal source set.
- **Pin selectors** (`get_pins`, bare `inst/pin`; 14 more scenarios):
  OpenSTA's launch objects are the sequential cells' **clock pins** —
  `-from [get_pins u_ff0/CLK]` removes the path, `-from [get_pins
  u_ff0/Q]` and `-from [get_pins u_ff0/D]` cover nothing. On the capture
  side `-to [get_pins u_ff1/D]` matches the D endpoint. `get_pins u_ff0`
  (bare instance), `get_pins d_in`/`get_pins clk` (port names) and
  `get_pins u_ff0/NOPE`/`u_nope/CLK` all resolve empty → dead exception,
  baseline timing. Bare `u_ff0/CLK`/`u_ff1/D` behave identically to the
  `get_pins` forms.

### Multicycle endpoint-kind limit (v1)

Latch endpoints evaluate slack piecewise (borrowing clamp in
`collect_endpoints`), so `slack + shift` composition is invalid there, and
clock-gate enable endpoints have their own required-time structure.
`set_multicycle_path` whose `-to` resolves to a latch-D or clock-gate
enable endpoint is **partially applied**: FF-check and output-port
endpoints get the exception, the rest are counted in a new diagnostic —
same partial-with-count precedent as `-lib_cell`/`-pin` drops. `false_path`
has no such limit (it only removes groups).

### CRPR safety

Each arrival extreme carries a `crpr_*` launch-tag mask (interned
`(clock net, clock edge)` bits plus a sentinel bit for ports, latch
clock-to-Q launches and unresolvable sources). A required time takes the
shared-net driver spread only when the mask is a single non-sentinel tag
matching the capture net and edge — contested masks and non-shared
winners get zero, the same conservative answer as the old
all-source cone consensus (`crpr_sink_net`/`CRPR_BAD`, retired
2026-11-19 with the `pcr_*` fixture). A single-signature run's arrival
must come through that class's sources, so a delay-covered class's mask
holds only the covered launch's tag — which is what lets the delay
check read the same per-path credit (`docs/path-delays.md`).

## SDC parsing

- `Sdc.exceptions: Vec<TimingException>` in file order:
  `{ kind: FalsePath | Multicycle, from: Option<Sel>, to: Option<Sel>,
  setup_n: Option<u32>, hold_n: Option<i32> }` — `hold_n` is signed because
  measured OpenSTA semantics accept `-hold -1`.
- New symbolic selectors resolved after design load: `get_cells`,
  `all_registers` (new `Value` variant with `PortSel`-style `Except`
  composition for `remove_from_collection`); `get_ports`, `all_inputs`,
  `all_outputs` reuse `PortSel`; `get_pins` and bare `instance/pin` names
  become `ObjSel::Pins`, resolved to cell pins via an `inst/pin` ->
  `PinId` map — `-from` keeps only pins that launch (clock/enable pins,
  mapped to their launch keys), `-to` keeps endpoint pins; bare names
  (`-from a`) resolve against both namespaces once the design is loaded.
- Clock-object selectors (`get_clocks`, `all_clocks`, bare clock
  `-name`s) in `-from`/`-to`: resolved to `ObjSel::Clocks(rows)` —
  `-from` covers launch keys tagged to the rows, `-to` covers endpoints
  capturing on the rows (`docs/multi-clock.md` §1.7; unknown names give
  an empty row set — a dead rule, not an error). On a name collision the
  clock namespace wins (measured `t2_nsfrom`/`t2_nsto`: a bare `-from`/`-to`
  name shared by a clock `-name` and a port resolves to the clock, no
  warning). A name that is *not* a clock `-name` still resolves to the
  port object — a clock's source-port name therefore resolves to the
  port (a resolved-but-dead `-from`, or a `-to` diagnostic, measured
  `t2_selport`/`t2_selpto`).
- Unsupported qualifiers (`-through`, `-start`, `-end`, `-rise`, `-fall`,
  `-edge`, `-lib_cell`, `-pin`, `-points`) → whole command to
  `ignored_commands`: loud in the skip summary, fails `--strict`, never
  half-applied.
- Selector subcommands that fail to evaluate inside exception arguments
  (e.g. `[get_pins ...]`, hierarchical forms) must route the command to
  `ignored_commands`, not abort `parse_sdc` — an opaque `Value` variant or
  caught evaluation, with parse tests.
- `set_multicycle_path` accepts `N` as `-setup`/`-hold` value, bare
  positional form (setup), and `-setup X -hold Y` combined (valid SDC;
  OpenSTA rejects it — see measured semantics). `-setup N` requires
  `N >= 1`; `-hold H` accepts any integer (`0` = default, negative
  tightens — measured). `-both N` is counted in `ignored_commands`: it is
  valid SDC but OpenSTA rejects it (`STA-0563`), so its hold-side
  semantics cannot be verified — it is reported, not guessed.
  `-setup`/`-hold`/`-both` flags on `set_false_path` are accepted as
  no-ops (the path is removed regardless).
- Selector sides resolving zero objects (typo'd cell, unknown
  port/pin/clock name) → `unresolved_exception_selectors` count,
  matching OpenSTA's warn-not-silent `no valid objects specified` form.
  A resolved-but-invalid `-to` object (a non-output port, or a resolved
  pin that owns no endpoint — OpenSTA `STA-1551`, measured
  `t2_selpto`/`t2_toinv`) counts there too. Resolved cells owning no
  endpoints stay silent, and a resolved-but-dead `-from` (`-from clk1` —
  the port object exists but launches nothing) is a silent dead rule,
  counted nowhere (`t2_selport`).
- `unresolved_load_selectors`: `set_load` `get_nets` names missing at
  `prepare_sta` resolution (counted per missing name, matching OpenSTA's
  per-name warning), plus resolved net annotations the parasitic path
  cannot honour — a measured net annotation replaces the net's wire
  capacitance, which the distributed RC model cannot express, so under
  `NetParasitics` every selected net counts here instead of being
  approximated.
- `unsupported_path_delay_endpoints`: endpoint candidates a covering
  `set_max_delay`/`set_min_delay` cannot replace (output ports,
  clock-gate enables, latch-D, async recovery/removal) — loudly partial,
  see `docs/path-delays.md`.
- `cross_domain_endpoints`: endpoints whose launch domain differs from
  their capture domain, deferred to mc-xdomain and named in
  `skipped_endpoints` with reason `CrossDomain` — see
  `docs/multi-clock.md`.

## Out of scope for v1 (diagnosed, not approximated)

- `-through`, `-start`, `-end`, `-rise`, `-fall`, `-edge`, `-lib_cell`,
  `-pin`, `-points`
- `set_min_delay` / `set_max_delay` — implemented as path-delay checks in
  `docs/path-delays.md`; `-through` and the other qualifiers on that
  document's unsupported list still count the whole command ignored
- multicycle on latch-D/clock-gate-enable endpoints (counted diagnostic);
  output-port endpoints **are** covered (measured)

## Cost

Prepare once + eval per signature class. Typical exception SDCs → 2–5 runs.
Pathological per-register exceptions → up to #classes runs; each eval costs
what `run_sta` costs today. Measure on `picorv32`/`aes`; if a design exceeds
a documented budget, sources that cannot reach any `-to` endpoint may be
pruned (reachability pre-pass — only if measurement demands it).

`sta_pin_slews` (power estimation) always uses the unfiltered single run —
unchanged.

## Validation

Public, reproducible, per AGENTS.md rules:

1. Exception SDC fixtures on `gcd` and `picorv32` (committed): false_path
   reg→reg, `-to [all_registers]`, `-from [all_inputs]`, multicycle setup/hold
   (post-measurement conventions), mixed FP+MC, latch-endpoint multicycle
   diagnostic, unsupported-qualifier diagnostics.
2. Reference: OpenSTA in the LibreLane container — `report_timing`
   WNS/TNS + per-endpoint slack via committed Tcl → golden file. The
   multicycle-semantics probe (`tests/designs/sta_exceptions/`, 19
   scenarios) is committed with its golden before implementation.
3. Unit tests: selector resolution, signature partitioning, slew-only
   merging for excluded sources, composition rules, ignored-command
   accounting, can-fail checks.
4. Regression: all `sta_edge` goldens + 16 corners byte-identical with no
   exceptions (filter=None path); `merge_corner_reports` endpoint sets must
   match across `--lib-hold` corners (false_path drops are corner-independent
   — verify `hold_design` shares pin identity).

## Implementation status (2026-11-18)

Implemented per this design, in three commits:

- `0711785` — SDC data model + parser: `TimingException`, `ObjSel`
  (`Ports`/`Cells`/`Names`), `PortSel`/`CellSel` with `Except` composition,
  positional/`-setup`/`-hold` counts (redundant flags after a positional
  count are no-ops), `-both` → `ignored_commands`, unsupported
  qualifiers/selectors → whole command `ignored_commands`.
- `32abb45` — `run_sta_detail` split into `prepare_sta` (once) /
  `eval_sta` (per signature class, `allowed: Option<&HashSet<u32>>`) /
  `assemble_sta_report`; excluded sources merge slew-only via
  `merge(NEG_INFINITY, INFINITY, slew…)`; `EndpointEntries` carries `PinId`.
- exception application — `run_sta_partitioned`: selector resolution
  (`resolve_from`/`resolve_to`, empty sides and unknown names counted in
  `unresolved_exception_selectors`), signature-class partitioning, per-class
  `eval_sta`, composition (false-path drop, per-field multicycle merge in
  file order, measured req shifts on FF checks + output ports, latch-D /
  clock-gate enables → `unsupported_multicycle_endpoints`), skip pass on
  the composed endpoint set.

Verification: `test_sta_exceptions_match_opensta` deserializes
`exceptions.reference.json` and compares every OpenSTA-parseable scenario
endpoint-by-endpoint (8 fields + WNS/TNS + violation counts) — 34
scenarios including the pin-selector set;
`test_sta_exception_diagnostics` covers false-path removal counts,
unresolved selectors, and `-both` accounting. `mc_setup2_hold1` (valid SDC
OpenSTA rejects) and `mc_both` are exercised for diagnostics only.
`sta_pin_slews`/power see the union state — slews are identical across
classes by construction.

## Files (expected)

- `crates/grus-core/src/sta/sdc.rs` — selectors, `TimingException`, parsing
- `crates/grus-core/src/sta/analysis.rs` — prepare/eval split, arrival-only
  filter, `EndpointPolarities` keyed by `PinId`, composition, `SkipReport`
  additions (`false_path_endpoints`, multicycle-endpoint diagnostic)
- possibly `crates/grus-core/src/sta/exceptions.rs` — resolution + composition
- `crates/grus-eda/src/main.rs` — wiring (likely unchanged if `run_sta` wraps)
- `tests/designs/**` — exception SDC fixtures + OpenSTA golden scripts
- `docs/results.md`, `docs/roadmap.md` — measurements and status

## References

- `crates/grus-core/src/sta/analysis.rs`: `TimingGraph` (~L612),
  `build_graph` (~L940), port seeding (~L1994), `propagate_slew` (~L1479),
  launch eval (~L2173), `collect_endpoints` (~L2546), `crpr_sink_net`
  (~L2490), `PinTimingState::merge` (~L122)
- `crates/grus-core/src/sta/sdc.rs`: `Value`/`PortSel` (~L10-60), `Sdc`
  (~L121), `Interp::command` (~L520)
- Review record: subcon reviews `20261118-100552-7401` (agy,
  gemini-4-hydrogen-high) and `20261118-101221-da89` (devin, swe-2-flash),
  both "reject as written; core approach sound" — findings incorporated above
- OpenSTA exception semantics: measured — `tests/designs/sta_exceptions/`
  (`generate_ref.sh` → `exceptions.reference.json`), formulas in
  "Multicycle semantics — measured on OpenSTA" above

<!--
Reviewers: design review, not code review. The snapshot has the full repo at
HEAD; verify claims against the referenced code. Prior review round
(20261118-*) found: slew coupling (fixed by arrival-only filtering +
prepare-hoisted slew/clock passes), symbolic-from unsoundness for
all_registers (fixed by signature classes + strict universal rule),
multicycle semantics guessed (now measurement-gated), latch multicycle
non-composable (now diagnosed), -to clock-gate endpoints (now included),
SkipReport composition (now specified), nested selector hard-fail (now
specified). Flag any remaining correctness hole.
-->
