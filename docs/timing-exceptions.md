# Timing exceptions: `set_false_path` / `set_multicycle_path`

Design note for the N2 deferred item. Today both commands are hard parse errors
(`sdc.rs` test asserts `parse_sdc("set_false_path ...").is_err()`), which blocks any
stock SDC that uses them.

## The problem

Our STA collapses each pin's timing to one worst arrival/required pair
(`PinTimingState`: per-pin rise/fall max/min plus a launch-edge-parity bitmask).
It does not retain *which* launch object produced an endpoint's worst arrival.
SDC exceptions are defined per (launch, capture) path pair, so a `-from
[get_cells u1]` exception cannot be applied to an aggregated endpoint: filtering
on the collapsed value could hide a real violation or erase an unrelated path.

## Approach: partitioned re-analysis ("multi-run")

Instead of teaching `PinTimingState` about path identity (a rewrite of the
verified R2–R9 core: propagated clocks, latch borrowing, async reset, CRPR),
run the existing engine once per launch-source group and compose the endpoint
results. The timing core stays byte-identical; only a launch filter is added.

### Launch sources

Every path starts at one of three source kinds, all identifiable by `PinId`:

- primary-input port seeding (`state[pin]` written directly for non-clock inputs)
- `TimingGraph.launches` — FF `CLK -> Q` arcs; source key = `l.q` pin
- `TimingGraph.latch_opens` — latch enable-to-Q arcs; source key = `o.q` pin

Latch `D -> Q` borrow arcs stay unfiltered: they propagate an existing arrival
through a transparent latch, they do not create a new path origin.

A `LaunchKey` is the `PinId` of the launched pin (Q pin, or the I/O pin for
ports). `-from [get_cells u1]` resolves to every launch key whose pin belongs to
instance `u1`; `-from [all_registers]` resolves to all FF/latch launch keys;
`-from [all_inputs]`/`[get_ports p]` resolve to I/O pin keys.

### Exception universe

`E_set` = union of the *explicitly resolved* `-from` launch keys across all
exceptions. From-sets that resolve to "every launch source" (e.g.
`all_registers` when the design has only registers, `all_clocks` on a
single-clock design) stay symbolic predicates and do not expand `E_set` — they
need no per-source runs.

Runs needed: `1 + |E_set|`.

- **default run**: every launch source except `E_set` members launches.
- **source run `s`**: only `s` launches (all ports, FFs, latch opens except `s`
  are suppressed at seeding/eval time).

In source run `s`, the worst arrival at endpoint `E` is exactly the worst
`s -> E` path arrival: path arrival depends only on its own launch edge and arc
delays, never on other sources. In the default run it is the worst path from
any non-exception source.

### Filtering mechanics

`run_sta_detail` gains a launch filter applied at exactly three points:

1. input-port seeding loop (~line 1994): skip ports not in the allowed set
2. `launch_updates` (~line 2173): skip `LaunchArc`s whose `q` is not allowed
3. `latch_updates` (~line 2207): skip `LatchOpen`s whose `q` is not allowed

Everything else is untouched: `build_graph`, levelization, clock propagation /
ideal-clock seeding, `collect_endpoints`, CRPR. Clock-pin seeding intentionally
keeps *all* clock pins (checks still need capture-clock arrivals regardless of
which data sources launch).

To avoid rebuilding the graph per run, split `run_sta_detail` into a prepare
phase (graph + levelized, built once) and an eval phase taking the filter.
`run_sta*` public entry points keep their signatures.

### Endpoint identity

Endpoints are `graph.checks` data pins (setup/hold and recovery/removal),
clock-gate enable pins, and output ports — all `PinId`-keyed internally.
`-to` selectors resolve to a set of endpoint `PinId`s:

- `[get_cells u2]` → every check pin of instance `u2` (D, async set/reset, latch D)
- `[get_ports p]` / `[all_outputs]` → output `PinId`s
- `[all_registers]` → all sequential-cell check pins
- a missing `-to` means "all endpoints"; a missing `-from` means "all sources"

`run_sta_detail` already returns `Vec<EndpointPolarities>` parallel to the
sorted `endpoints` vec; the combine layer needs the `PinId` alongside — extend
the return (or `EndpointReport`) with the pin id rather than matching on the
rendered `inst/pin` string.

### Composition rules

For each endpoint `E`, gather the group's worst candidates and apply every
exception whose `-to` matches `E`:

- **false_path**: drop every group fully covered by the exception's `-from`
  set. The default group can only be dropped by a universal `-from`. If all
  groups drop, the endpoint leaves the report (counted in a new
  `SkipReport::false_path_endpoints`, never silently).
- **multicycle_path** `-setup N` / `-hold N`: paths in `-from` get required
  time shifted by `(N-1) * period` (setup: capture edge N cycles later; hold:
  early edge correspondingly earlier — exact convention verified against
  OpenSTA before coding). Equivalent to adding `(N-1)*T` to that group's slack.
  `-setup` and `-hold` are independent.
- **precedence**: false_path beats multicycle on the same (source, endpoint);
  overlapping multicycles on one group need an OpenSTA-measured rule (validate
  empirically, do not guess).

Per endpoint the worst slack is the min over surviving groups of
`group_slack + shift(group)`, for both setup (max) and hold (min) sides.
`EndpointReport`'s collapsed max/min triples suffice because the required time
is identical across runs (capture side is unfiltered) and the multicycle shift
is uniform per group.

### CRPR safety

`crpr_sink_net` traces the endpoint's comb cone structurally. It returns a
value only when *every* launch in the cone agrees on (clock net, edge); a
per-source run's arrival must come through that same source, so the traced
value either matches it or the trace returns `CRPR_BAD` (no credit) — identical
to the unfiltered run. No change needed; a regression test covers it.

## SDC parsing

- New `Value::Cells`-style symbolic selectors: `get_cells`, `all_registers`
  stay unresolved until design load (they are design-dependent, unlike
  `get_ports`). `remove_from_collection` learns the new variant.
- `Sdc.exceptions: Vec<TimingException>` stores `{ kind, from: Option<Sel>,
  to: Option<Sel>, setup_n, hold_n }` in file order.
- Unsupported qualifiers (`-through`, `-start`, `-end`, `-rise`, `-fall`,
  `-edge`, `-lib_cell`, `-pin`, `-points`) move the whole command to
  `ignored_commands` — loud in the skip summary and failing `--strict`, never
  silently half-applied. Rationale: applying the exception minus the qualifier
  would over-apply it; ignoring it entirely only risks false violations, the
  conservative direction.

## Out of scope for v1 (diagnosed, not approximated)

- `-through`, `-start`, `-end`, `-rise`, `-fall`, `-edge`, `-lib_cell`, `-pin`,
  `-points` on either command
- `set_min_delay` / `set_max_delay` (different check type; separate milestone)
- clock-object `-from`/`-to` selectors beyond what resolves unambiguously on
  the single-design-clock model (multi-clock SDC is already unsupported)

## Cost

`1 + |E_set|` STA evals. Typical exception SDCs name a handful of synchronizer
or CDC registers → a few extra runs, each the same cost as today's `run_sta`.
A pathological `all_registers` `-from` that does not cover every launch source
could mean hundreds of runs; measure on `picorv32`/`aes` and document. If it
is too slow, batch sources that share identical exception membership per
endpoint — a later optimization, not v1.

## Validation

Public, reproducible, per AGENTS.md rules:

1. Craft exception SDCs on `gcd` and `picorv32` (committed fixtures):
   false_path reg→reg, false_path `-to [all_registers]`, multicycle `-setup 2`,
   multicycle `-hold`, `-from [all_inputs]`, mixed FP+MC on one endpoint.
2. Reference: OpenSTA inside the LibreLane container — `report_timing` WNS/TNS
   and per-endpoint slack for the same DEF/SDC, emitted by a committed Tcl
   script into a golden file.
3. Unit tests: selector resolution, group filtering (a source run sees only its
   own arrivals), composition rules, ignored-command accounting, and a
   can-fail check (remove the filter → test must go red).
4. Regression: all existing `sta_edge` goldens and corner tests must stay
   byte-identical when no exceptions are present (filter = None path).

## Open questions for review

1. Is per-source-run arrival truly independent of other sources in *this*
   engine (latch borrow, clock gates, CRPR, derates)? Any hidden coupling?
2. `EndpointReport`/`EndpointPolarities` composition: is collapsing to the
   worst triple per group sufficient for multicycle `-rise`/`-fall` later, or
   must v1 keep per-polarity candidates?
3. Multicycle required-time shift sign/convention — is `(N-1)*T` right for
   both `-setup` and `-hold`, and how does OpenSTA order `-setup`/`-hold`/`-both`?
4. Precedence when several exceptions match one (source, endpoint) pair.
5. Cost model: is `1 + |E_set|` runs acceptable, or should universal-from
   exceptions shortcut harder?
6. Anything in `collect_endpoints` (clock-gate enables, output ports, async
   checks) that breaks when arrivals come from a single-source run?
7. Should exceptions also apply to the hold corner (`--lib-hold` second run)
   identically, or are there interactions?

## Files (expected)

- `crates/grus-core/src/sta/sdc.rs` — selectors, `TimingException`, parsing
- `crates/grus-core/src/sta/analysis.rs` — prepare/eval split, launch filter,
  exception composition, `SkipReport` additions
- possibly `crates/grus-core/src/sta/exceptions.rs` — resolution + composition
- `crates/grus-eda/src/main.rs` — wiring (likely unchanged if `run_sta` wraps)
- `tests/designs/**` — exception SDC fixtures + OpenSTA golden script
- `docs/results.md`, `docs/roadmap.md` — measurements and status

## References

- `crates/grus-core/src/sta/analysis.rs`: `TimingGraph` (~L612),
  `build_graph` (~L940), port seeding (~L1994), launch eval (~L2173),
  `collect_endpoints` (~L2546), `crpr_sink_net` (~L2490)
- `crates/grus-core/src/sta/sdc.rs`: `Value`/`PortSel` (~L10-60), `Sdc` (~L121),
  `Interp::command` (~L520)
- OpenSTA exception semantics: SDC `set_false_path` / `set_multicycle_path`
  (to be measured against the pinned OpenSTA in the LibreLane image, not
  assumed)
- Prior art inside this repo: `docs/e3-sta.md` open tasks, R0–R10 / N1–N7
  results in `docs/results.md`

<!--
Reviewers: this is a design review, not a code review. The snapshot contains
the full repository at HEAD; read the referenced code to verify the claims
above (especially the "Launch sources", "Filtering mechanics", "Composition
rules" and "CRPR safety" sections). Answer the seven open questions and flag
any correctness hole in the multi-run approach. Do not modify files.
--><|end_message|><|message_model|><|content_thinking|>Now commit the doc, write the review brief, and launch both reviews.<|end_message|><|message_model|>exec<|content_invoke_tool_json|>{