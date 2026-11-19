# check-arcs — OpenSTA surface probe

Acceptance item 1 requires the OpenSTA command and returned fields to be
identified *by running it* and recorded in `docs/results.md`. This probe is
step 0 — it runs inside `ghcr.io/librelane/librelane:3.0.16` on the new
fixture before the golden generator is finalized. Every answer below is
written into the dated `docs/results.md` entry; nothing is implemented from
guesswork.

## Probe script shape

A throwaway Tcl heredoc fed to `openroad -no_init -no_splash` (the same
container route every `generate_ref.sh` uses; AGENTS.md §7.3: stdin Tcl must
not use `!`, write `[...] == 0`). Load lib/lef/def + each fixture SDC in
turn, then run the items below. Record raw output verbatim in the results
entry where it is short.

### Q1. Command surface and fields

- `report_check_types -min_pulse_width -digits 6` — capture the full table:
  column headers, one row per (pin, pulse) or per pin, `MET`/`VIOLATED`
  markers, sort order.
- `find_timing_paths -path_delay min_pulse_width -endpoint_path_count 1 -group_path_count 100000`
  inside `catch` — if it returns PathEnds, enumerate accessors on one:
  `data_arrival_time`, `data_required_time`, `slack`, `end_transition`,
  `path_delay`, `pin`, `vertex`, `tag`, `report` (call what exists; note
  which fields a pulse-width PathEnd actually fills — `data_arrival_time`/
  `data_required_time` may be absent or mean close-edge/width).
- `info commands *pulse*` and `info commands sta::*pulse*` — list the
  programmatic surface (there may be `sta::min_pulse_width_*` procs usable
  for machine-readable extraction instead of text parsing).
- Also try `report_checks -path_delay min_pulse_width` — documented list
  stops at `min_max`, so expect an error; record the actual result either way.

### Q2. Row granularity

Does `report_check_types` emit a row for *every* (pin, pulse) check, the
worst row per pin, or only the worst row overall? The fixture has ≥5
check pins; if the default prints fewer rows than checks, try
`-violators` off/on and check whether PathEnds from Q1 give the full set.
`total_endpoints` accounting depends on this (see `api.md`).

### Q3. Constraint-table mapping

`rise_constraint` vs `fall_constraint` ↔ high vs low pulse. The lib values
differ (`dfxtp_1` at slew 0.01: 0.1687 vs 0.2082; `dfrtn_1` CLK_N: 0.1994 vs
0.2741), so the `Required Width` column at known pin slew identifies the
mapping. Candidates:

- `rise_constraint` = required **high** pulse (pulse opens on rise).
- `rise_constraint` = required **low** pulse (pulse closes on rise — the
  setup/hold convention where the constraint is indexed by the closing
  transition).

### Q4. Corner rule for `Actual Width`

The lib sets `min_pulse_width_mode : "max"`. `propagated.sdc` carries
`set_timing_derate -early 0.80 -late 1.20 -clock -cell_delay`, which opens
a real `arrival_min`/`arrival_max` spread on every clock-pin edge (proven
machinery: `sta_edge/timing_derate`, `pcr_*`). At each check pin compute
the three candidates against the golden `Actual Width`:

- **mode-max**: `arrival_max[close] − arrival_max[open]` (+period wrap).
- **mode-min**: `arrival_min[close] − arrival_min[open]`.
- **conservative mix**: `arrival_min[close] − arrival_max[open]`.

The difference is `0.4 × d(clock cell)` per traversed stage (~tens of ps —
above the 5 ps tolerance), so the scenario discriminates cleanly. If none
match, record the observed rule before implementing (AGENTS.md §7.6).

### Q5. Slew argument for `related_pin_transition`

The constraint table is 1-D on the related (= checked) pin's transition.
Candidates: open-edge slew, close-edge slew, or `slew_max`/`slew_min` of a
specific corner. `propagated.sdc` gives clock pins whose rise and fall
slews differ (rise/fall table asymmetry through `clkbuf`/`clkinv`, and the
`-cell_check` derate pair may split them further). Identify the argument by
matching `Required Width` to table lookups at each candidate slew.

### Q6. Uncertainty and `-cell_check` on `Required`

`propagated.sdc` also carries `set_clock_uncertainty 0.1` and
`set_timing_derate -early 0.90 -late 1.10 -cell_check`. If `Required Width`
equals the raw table lookup at the measured slew, neither applies; if it
differs, solve for the applied factor/offset and record the rule
(derate factor, or uncertainty added/subtracted).

### Q7. Does pulse slack feed the min/max report?

On `viol.sdc` (every check violated, ordinary endpoints also checked):
record `sta::worst_slack -min`, `sta::total_negative_slack -min`, and the
`find_timing_paths -path_delay min` endpoint list. Expected: pulse checks
are a separate family and do not enter either. If they do, `StaReport`
accounting must mirror it.

### Q8. Edge pairing under inversion

At `u_ff2/CLK` (behind `u_clkinv`) the first transition is a fall. Verify
the golden's `Actual Width` there equals `fall_arr(+period) − rise_arr`
ordered as "the next opposite edge", not `|fall − rise|`. One wrap suffices:
arrivals store the first occurrence of each transition, and the two
transitions are at most one period apart.

### Q9. Deferred-type census

While the container is up, confirm the deferred types that stay counted
have no instantiated instances in the checked designs: read our existing
skip summaries (`recognized_check_arcs` maps on gcd/picorv32/aes/ibex/jpeg
runs — `ibex` shows only `Clear`/`MinPulseWidth`/`Preset` today) and/or the
lib census (`non_seq_*`/`three_state_*`/`preserve` are on cells our
netlists do not use). Record the census result in `results.md` — if any
count is nonzero on a real design, keep deferring and say so explicitly
(goal text).

## Recording format

One dated `docs/results.md` entry: the fixture files used, the probe Tcl
(rerun via the committed `generate_ref.sh` probe mode or a pasted heredoc —
name exactly what ran), the command/fields answer (Q1–Q2), the measured
rules (Q3–Q6, Q8), the WNS inclusion verdict (Q7), the census (Q9), and the
raw `report_check_types` table excerpt. Implementation decisions in
`api.md` are then switched to the measured alternatives.
