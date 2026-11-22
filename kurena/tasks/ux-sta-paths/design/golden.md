# ux-sta-paths — Reference golden & tests

## 1. Golden generation

New committed script `tests/designs/gcd/generate_paths_ref.sh`, modelled on
`tests/designs/collect_paths.sh` (same container wrapper, same
`ghcr.io/librelane/librelane:3.0.16`, same PDK path layout). It writes two
files next to the existing gcd artifacts:

- `tests/designs/gcd/gcd.paths_max.txt`
- `tests/designs/gcd/gcd.paths_min.txt`

Inputs: `gcd.placed.def` + `gcd.routed.spef` (the placement-estimate SPEF —
the same parasitics the `placed_spef` block of `gcd.sta.reference.json` uses)
+ `gcd.sdc`. One `openroad` Tcl pass emits both reports — max first, then min
(the `find_timing_paths` invalidation comment at `generate_ref.sh:772` is
about `find_timing_paths` PathEnds, not `report_checks`; still keep the
max-first order for consistency). Command surface per corner:

```tcl
report_checks -path_delay <max|min> -group_path_count 3 \
    -unique_paths_to_endpoint -endpoint_path_count 1 \
    -fields {slew cap fanout input_pins} -digits 6 -format full
```

**Probe first** (acceptance: "probe the exact flag surface"): run
`report_checks -help` (or `sta` `-help`) inside the pinned image and confirm
`-endpoint_path_count`, `-unique_paths_to_endpoint`, `-group_path_count` all
exist and mean what is assumed — `-group_path_count 3 -unique_paths_to_endpoint`
is the spelling `collect_paths.sh` already uses to get "3 worst paths with
distinct endpoints"; if `-endpoint_path_count 1` is redundant or rejected,
drop it and record the actually-run flags in the script header and
`docs/results.md`. The committed output files must come from the committed
script run as written (E0 lesson 1): after generating, `git diff` the outputs.

N = 3 per corner. Rationale: the worst gcd setup paths share one startpoint
register, so 3 exercises distinct endpoints without depending on tie order;
file size stays ~200 lines per corner.

## 2. Tests (all in `mod tests` of `analysis.rs`, existing conventions)

### `test_gcd_critical_paths_vs_opensta`

- Load `gcd.placed.def`, `gcd.sdc`, `gcd.routed.spef` via `spef_parasitics`
  (the `placed_spef` mode), run `run_sta_with_paths(&design, &lib, &sdc,
  Some(&par), 3)`.
- Assert `critical_paths.len() == 6` (3 `max` + 3 `min`); gcd has hold
  checks (`gcd.sta.reference.json` carries `min_*` fields).
- Parse `gcd.paths_max.txt` / `gcd.paths_min.txt` with the existing
  `parse_paths` helper (reuse; extend it only if the min report's
  `Path Type: min` line needs handling — check first).
- Match our traces to ref paths by endpoint: ref `Endpoint: _16945_` is a
  cell name; ours is a check pin `_16945_/D` — map by `endpoint` starting
  with `ref_name + "/"`. Require all 3 ref paths per corner matched.
- **Pin sequence**: the ref stage pin list (`inst/pin` rows only —
  `parse_paths` already filters clock/summary lines) must equal our
  `stages` pin list verbatim, same order, where our arrivals agree with
  OpenSTA's endpoint arrivals (the standing `max(5 ps, 1%)` endpoint bound —
  on gcd `placed_spef` they do). Any sequence divergence is a named residual
  in `docs/results.md`, not a silent skip.
- **Per-stage delay & arrival**: for every aligned stage pair, assert
  `|our.delay - ref.delay| <= max(0.005, 0.01 * |ref.delay|)` and the same
  bound on `arrival` (`0.005` ns = 5 ps). Collect the max residual and print
  it for the results log.
- **Transition**: our `stages[i].transition` equals the ref stage's `^`/`v`.
- **Path-level**: `slack_ns`, `required_ns`, `data_arrival_ns` within the
  same bound vs the ref block's `slack`/`data required time`/`data arrival
  time` lines — mostly redundant with the endpoint goldens but it pins the
  `Worst` plumbing.

### `test_critical_paths_structure_gcd`

Cheap unit test on the same run: `stages[0].edge ∈ {Clock, LatchOpen, Port}`,
`stages` alternates `cell_arc`/`net` after the launch, `arrival_ns` is
monotone non-decreasing on the max corner (non-increasing on min), each
stage's `arrival_ns == prev.arrival_ns + delay_ns` within `TRACE_EPS_NS`
(except the `clock`/`port` anchor stages whose delay is the anchor itself),
and the last stage's `arrival_ns` equals the endpoint's `state` arrival. Also
assert `critical_paths` is empty from `run_sta` (`paths = 0`).

### `test_critical_paths_exceptions`

One partitioned fixture — `tests/designs/sta_exceptions/exceptions.def` +
`base.sdc` (no exceptions → single state) is NOT enough; use an SDC that
forces `run_sta_partitioned` (e.g. `fp_allregs.sdc` or `md_basic.sdc` —
pick one that leaves ≥1 reachable endpoint and ≥3 reportable endpoints,
verified while implementing). Assert the produced traces reconstruct:
cumulative arrivals match `state`-consistent stage sums and
`slack_ns` equals the endpoint row's slack. This exercises the `Worst.class`
tag; no OpenSTA comparison needed beyond existing scenario goldens.

### `test_critical_paths_hold_corner` (optional, fold into the gcd test)

Assert min-corner traces end at hold-check pins and use `arrival_min` values
consistent with `min_slack_ns` rows.

## 3. Fail-proof

Acceptance: "corrupt the trace (e.g. drop the winning-edge selection so it
picks the first incoming edge) → path test must go red; restore."

Procedure: in `trace_path`'s predecessor selection, replace the argmax/argmin
pick with the first candidate (or skip the `TRACE_EPS_NS` match check) —
run `test_gcd_critical_paths_vs_opensta`: the pin sequence must diverge and
the test goes red. Restore and re-run green. Record the red run (exact test
name + the corrupted line + observed diff) in `docs/results.md`.

## 4. JSON surface check

`sta --paths 3 --json <path>` on gcd — deserialize the file in the same test
(or a CLI-level smoke via `main.rs`'s own pattern if one exists — there is
none today, so assert on the serialized `StaReport` in the unit test:
`serde_json::to_value(&report)` contains `critical_paths` with the documented
schema keys, and a `paths = 0` report omits the key entirely).

## Files

- `tests/designs/gcd/generate_paths_ref.sh` (new)
- `tests/designs/gcd/gcd.paths_max.txt` (new, generated)
- `tests/designs/gcd/gcd.paths_min.txt` (new, generated)
- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-eda/src/main.rs`
- `docs/results.md`
