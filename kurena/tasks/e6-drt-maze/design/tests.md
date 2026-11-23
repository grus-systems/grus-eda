# e6-drt-maze — Test plan

All additions land in `crates/grus-core/tests/drt.rs` (existing E6
gate: same `load`/`load_guide`/`pin_label`/`require` helpers,
fail-loud on missing inputs). Real-design tests iterate
`["gcd", "picorv32"]`; `picorv32.placed.def` is gitignored —
generate via `tests/designs/picorv32/generate_ref.sh` if absent
(container run, mention before running). A `load_routed_inputs`
helper wraps `load` + `load_guide` + `build_route_graphs` +
`via_table` + `route_nets`.

## Synthetic fixture — `test_maze_cases`

One hand-built LEF + DEF (pattern after `REGION_LEF`/`REGION_DEF`
and `VIA_FIXTURE_*`): met1 `HORIZONTAL` + met2 `VERTICAL`, a shared
crossing, one cell with signal pin `A` (met1 rect) and pin `B`
(met2 rect), one cell with a macro `OBS` rect, an io port, and
`TRACKS` on both layers. Guide text is a string literal per case,
exactly as `test_region_escape_fixture` builds it.

1. **Straight route** — two pins whose attach nodes share one met1
   line; guide covers the span. Assert: one net routed, single
   `WireSeg` on met1 spanning the two pins' tracks, `vias` empty,
   both pins `connected`, `collisions` empty.
2. **Forced via transition** — driver attaches met1 only, sink's
   rect is met2-only; the guide covers both layers. Assert: the
   route holds ≥1 `ViaPlace` on the met1–met2 pair whose `(x,y)` is
   a shared crossing, and both pins connected.
3. **Detour around a blocked interval** — a macro `OBS` rect covers
   the direct met1 line between the pins. Assert: still fully
   connected; the committed wires use a jog (route cost or segment
   count strictly greater than the unobstructed control from case
   1); and `overlap_census`-style checking vs the OBS rect confirms
   no committed interval enters its spacing band (the OBS is a hard
   block, so the assertion is on the emitted intervals, not on
   trust).
4. **Provably unroutable** — guide text omits the box covering the
   sink's area, so the region splits into two components (or the
   pin's rect holds no covered line at all — `Unattachable`).
   Assert: `route_nets` returns promptly (bounded by `max_iters` —
   the harness completing is the "no infinite loop, no panic"
   evidence), `unrouted` contains the net with the pin named and the
   right `FailKind`, and `nets` excludes it.

## Real-design census — `test_maze_census`

Per design, `route_nets` over the committed guide, then print:
guided nets, routed, member pins connected, projected-attachment
count, unrouted nets **by name with per-pin labels and `FailKind`**,
collision count, iterations used, and wall-clock ms
(`std::time::Instant`, printed for `results.md`).

Asserts:

- `unrouted` entries each carry ≥1 `(pin, FailKind)` — every failure
  is named (acceptance).
- Each unrouted net is **classified**: the test replicates the
  planning probe (is the net's `{li1,met1,met2}` guide-box union
  disconnected while the full union connects — the met3-bridge
  class) and requires every `NoPath` net to be either met3-bridged
  or individually recorded in the test comment after the first real
  run. Any unrouted net outside the recorded set fails loudly.
- `nets.len() + unrouted.len() == join.nets.len()` — total
  accounting, no silent drops.
- Nonzero evidence: `nets` nonempty with nonempty wires on both
  designs.

Planning-time bound (`overview.md`): gcd ≤ 3 (`_126_`, `_155_`,
`resp_msg[5]`), picorv32 ≤ 209 — the pinned count is whatever the
first real run names; if it lands at 0, pin 0.

## Occupancy fail-proof — `test_maze_occupancy`

On gcd (the smaller design): run `OccMode::Enabled` and
`OccMode::Disabled`, keeping everything else identical. Assert the
outcomes **measurably differ** — either the route digests differ or
the post-hoc collision counts differ (a `Disabled` run can only
commit equal-or-more overlaps; the assertion is "different", the
direction prints for `results.md`). Optionally also assert the
`Disabled` run's `overlap_census` is nonzero if that is what the
run shows — recorded, not assumed.

## Determinism — `test_maze_determinism`

Two full `route_nets` calls per design on the same inputs:
`assert_eq!(r1, r2)` on the whole `RouteDb` and
`assert_eq!(route_digest(&r1), route_digest(&r2))`, gated on
`!r1.nets.is_empty()`.

## In-module unit tests (`drt/maze.rs`)

- Band math: same-line near/abut/far intervals against a committed
  wire — inside `w + s` conflicts, exactly `w + s` does not
  (strict `< s`).
- Perpendicular crossing: a committed horizontal wire blocks a
  vertical candidate through its footprint, not beside it.
- Projection fallback: an escaped pin (strict set empty, one axis
  covered) yields same-line nodes; a rect with no covered track at
  all yields `Unattachable`.
- PQ order: equal-cost pops return the lower node id.

## Files

- `crates/grus-core/tests/drt.rs`
- `crates/grus-core/src/drt/maze.rs`
