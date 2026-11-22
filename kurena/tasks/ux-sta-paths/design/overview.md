# ux-sta-paths — Overview

E9 ticket 1 (`docs/history/run-2026-11-22-large-design.md` item 1, `docs/roadmap.md`
§E9 item 1): `grus-eda sta --paths N` emits the worst-N startpoint→endpoint path
traces per corner, and `StaReport` gains a `critical_paths` JSON block with the
same traces. A deep-slack outlier becomes actionable without leaving the tool.

The engine does not store per-pin predecessor edges: `PinTimingState` keeps the
merged arrival/slew extremes plus the `par_*`/`crpr_*`/`dom_*` winner masks, and
`eval_pin` (`analysis.rs` ~line 3163) forgets which incoming edge won. The trace
therefore **recomputes** each visited pin's winning predecessor at trace time —
same formulas (`sink_state_with_wire`, `driver_response`, `arc_legs`, derates),
same deterministic f32 arithmetic, so the recomputed argmax is the recorded
extreme bit-for-bit on every modelled path. `trace.md` is the algorithm.

## Semantics decisions (each also documented in `docs/e9-reporting.md`)

1. **Endpoint selection** — OpenSTA `report_checks -endpoint_path_count N` names
   paths *per endpoint*; the spec pins our rule as "the N endpoints with the
   worst slack, one trace per endpoint". Ranking key: the endpoint's *reported*
   check slack — `EndpointReport.max_slack_ns` for the max (setup) corner,
   `min_slack_ns` for the min (hold) corner — not `worst_*_slack_ns`. The `worst_*`
   fields fold every anchor of a multi-`-clock` output port while the stored
   `EndpointPolarities` only carries the reported (last-group) candidate, so
   ranking by `worst_*` could pick a check we cannot reconstruct. Multi-anchor
   ports are the named approximation: on them the ranking uses the last-group
   check, exactly the row `endpoints` reports.
   Endpoints with no candidate in the corner (`max_slack_ns`/`min_slack_ns` is
   `null`) do not compete. Order: slack ascending, ties broken by endpoint name
   for determinism (entries are already name-sorted by `assemble_sta_report`).
   N traces per corner — a single-`--lib` run emits N `max` + N `min` traces
   (hold may legitimately be empty when no min check exists anywhere).

2. **Corner model under `--lib-hold`** — each `run_sta_*` call produces both
   corners' paths (a `CriticalPath.corner` of `"max"` or `"min"`). The
   `merge_corner_reports` semantics already are "setup's max side + hold's min
   side"; the merge keeps `corner == "max"` paths from the setup report and
   `corner == "min"` paths from the hold report and concatenates (setup first).
   A single-lib run needs no merge and emits both corners from one state.

3. **`critical_paths` presence** — `Vec<CriticalPath>` on `StaReport` with
   `#[serde(default, skip_serializing_if = "Vec::is_empty")]`: the key is
   **absent** when `--paths` was not passed (or nothing traced), present as a
   list otherwise. Chosen over `Option<>` so the Rust type stays a plain `Vec`
   and `merge_corner_reports` concatenation needs no `Option` juggling. The
   schema (and this choice) is documented in `docs/e9-reporting.md`.

4. **Trace anchor** — stage `arrival_ns` values are the *physical* pin arrivals
   (`state.arrival_{max|min}[pol]`), while the path-level `data_arrival_ns` is
   `Worst.arr`, the value OpenSTA prints as "data arrival time". The two differ
   exactly by the endpoint pairing realignment our endpoint model applies
   (same-domain hold `+P`, cross-domain `lsh`): OpenSTA's `data_arrival_time`
   includes the same shift — `EndpointReport.max_arr_ns` is asserted against it
   today. So the trace's last-stage arrival can be one period short of
   `data_arrival_ns` on a realigned hold path; `docs/e9-reporting.md` states the
   rule instead of silently re-basing a stage.

5. **Exceptions / partitioned runs** — `run_sta_partitioned` composes endpoints
   per exception-signature class over `class_states`; tracing against the merged
   union state can pick a predecessor that belonged to a different class (a
   false-path-covered source still wins the union arrival). `Worst` therefore
   gains a `class: u16` tag (`u16::MAX` = trace against the passed-in state),
   stamped at the compose folds (`best_max`/`best_min`, the leaf-check fold, and
   the deferred-endpoint union compose — that one tags `u16::MAX` deliberately,
   its candidate was computed on the union). `run_sta_partitioned` computes
   traces *before* `class_states` drops, against `class_states[w.class]`, and
   also threads the per-class `disabled_borrow` set so a silenced `D -> Q` arc
   is not "re-won" by the trace. Details in `trace.md` §6.

6. **`-from get_pins` segment checks** — `pin_segment_arrival` currently returns
   only the winning segment value, not the pin. Extend it (or add a sibling) to
   also return the winning `-from` pin and store `from_pin: u32` on `Worst`
   (`u32::MAX` = none). A segment-check trace stops its stage walk at that pin
   and reports it as `startpoint` with `edge: "pin"`, matching OpenSTA's
   `Startpoint: <pin>` on such paths — cheap and honest rather than tracing a
   "path" whose reported arrival is a difference of two pin delays.

## What stays the same

- `run_sta`, `run_sta_quiet`, `run_sta_with_state`, `sta_pin_slews` signatures;
  callers in `main.rs`, `power.rs`, cts tests untouched except where a new
  `*_with_paths` variant is used.
- `EndpointReport` fields and the `--json` layout for every existing key;
  `critical_paths` is additive.
- `emit_sta_warnings` / `print_skip_summary` / `--strict` / `--spef-out`.
- Eval, endpoint, and skip logic: trace extraction is read-only over the
  finished state; the only engine-visible change is the two bookkeeping fields
  on `Worst` (never serialized, never part of any assertion).

## Out of scope

- Tracing *through* the clock network into the clock tree (we stop at the
  register clock pin / latch enable, like OpenSTA's own `-format full` launch
  block boundary at "clock network delay").
- Pulse-width checks (a separate report family — never enter `endpoints`,
  matching OpenSTA where they are not path endpoints either).
- N-paths-per-endpoint (`-endpoint_path_count N` true OpenSTA semantics) —
  spec'd semantic is N worst endpoints × 1 trace; noted in the doc.
- Per-domain summary / histogram (`ux-sta-summary`), `--top` (`ux-top-sdc`).

## Risks

- **Winner recompute drift**: the trace replays eval arithmetic; any divergence
  (e.g. `driver_response`'s per-call memo ordering, DMP internals) is
  deterministic — same inputs, same f32/f64 ops — so argmax selection is exact.
  Guard anyway: if no candidate lands within `TRACE_EPS_NS` of the recorded
  extreme, emit a `note` instead of fabricating a stage (acceptance's "name
  anything it cannot trace").
- **Union-state mis-trace under exceptions**: handled by the `Worst.class` tag;
  `gcd.sdc` carries no exceptions so the golden exercises the single-state path,
  and a `sta_exceptions` fixture test covers the tagged path.
- **Hold corner emptiness**: a design with no min-side candidates emits zero
  `min` paths — not an error; stdout prints nothing for that corner.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/mod.rs`
- `crates/grus-eda/src/main.rs`
- `tests/designs/gcd/generate_paths_ref.sh` (new)
- `tests/designs/gcd/gcd.paths_max.txt` (new, generated)
- `tests/designs/gcd/gcd.paths_min.txt` (new, generated)
- `docs/e9-reporting.md` (new)
- `docs/README.md`
- `docs/results.md`
- `docs/status.md`
- `Cargo.toml`
- `Cargo.lock`
