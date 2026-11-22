# deferred-check-arcs — OpenSTA probe

Runs inside `ghcr.io/librelane/librelane:3.0.16` via the committed
`generate_ref.sh` `run_probe` per SDC (one `openroad -no_init -no_splash`
process per mode, `probe_check_arcs_<mode>.txt` committed verbatim). Stdin
Tcl rule (AGENTS.md §7.3): no `!`, write `[...] == 0`. Every `catch` error
is recorded verbatim — a "flag does not exist" line is evidence, not a
failure.

## Q0. Command/flag surface (discovery)

```
info commands *check*
info commands sta::*check*
info commands *data*
info commands *three*
info commands *non_seq*
info commands *preserve*
```

Then, inside `catch`, try each `report_check_types` flag plausibly covering
the deferred types and record which exist and what they print:

```
- data_setup   - data_hold   - recovery   - removal
- clock_gating_setup - clock_gating_hold
- max_delay - min_delay - min_period - min_pulse_width
- max_slew - max_capacitance - max_fanout - violators
```

And each `find_timing_paths -path_delay` value beyond the known
`min`/`max`/`min_max`:

```
data_setup  data_hold  non_seq_setup  non_seq_hold
three_state  recovery  removal
```

`find_timing_paths` `-path_delay data_setup|data_hold` returning PathEnds
would make `non_seq_*` a golden-able endpoint family; an error means the
data-check surface (if any) is `report_check_types`-only. Record the exact
error strings.

## Q1. `non_seq_*`: evaluated as data checks?

`dfbbp`/`dfbbn` timing groups sit on `RESET_B` (related `SET_B`) and `SET_B`
(related `RESET_B`) — the two async pins check each other. The fixture gives
them *different* launch delays (`set_n` 0.9 ns vs `rst_n` 0.5 ns) so a data
check's slack is numerically distinguishable from the recovery/removal slack
at the same pin.

- `find_timing_paths -to [get_pins u_fbp/RESET_B] -endpoint_path_count 10
  -group_path_count 100000` (and `SET_B`, and `u_fbn` both pins): more than
  one PathEnd per pin ⇒ an extra check row coexists with recovery.
- `report_checks -to [get_pins u_fbp/RESET_B] -format full
  -endpoint_path_count 10`: look for a second block whose endpoint line
  names a check kind other than recovery/removal, and note its `Path Group`
  (`asynchronous`? `data check`?), the launch pin (`set_n`-side?), and the
  `library non_seq`/`data check` constraint line.
- `report_check_types -data_setup` / `-data_hold` (if they exist per Q0):
  rows, columns, slack sign convention, which pin is `Pin` (constrained or
  related), sort order. Verbatim into `results.md`.
- If rows exist: extract the arithmetic — which arrival of the *related*
  pin anchors the required time (rise edge? max corner? with uncertainty?
  with CRPR?), which `vio_3_3_1` index is which slew (var1 = related pin
  transition, var2 = constrained pin transition per the template), and the
  slack formula. Do this by matching the printed `Slack`/`Required` against
  `report_arrival` values on both pins and the lib table values.
- If nothing surfaces anywhere, that is the "OpenSTA also ignores them"
  verdict — state it with the verbatim empty results.

## Q2. `three_state_*`: propagation arcs, checks, or ignored?

`u_ebuf` (`ebufn_1`, `TE_B`), `u_einv` (`einvn_1`, `TE_B`), `u_evp`
(`einvp_1`, `TE`) each drive a `dfxtp` `D` pin (`u_cap1`/`u_cap2`/`u_cap3`).
`A` comes from the `d_in` tree; the enable comes from port `te` via `u_teb`.

- `find_timing_paths -from [get_ports te] -endpoint_path_count 10
  -group_path_count 100000`: if enable arcs propagate, paths from `te` end
  at `u_cap*/D`. Record the PathEnd list.
- `report_checks -to u_cap1/D -format full`: the startpoint of the worst
  path shows whether `Z`'s arrival came through `A` (`d_in` tree) or `TE_B`
  (`te` port).
- `report_arrival` on `u_ebuf/Z`, `u_einv/Z`, `u_evp/Z` verbatim.
- `report_check_types` default + any three_state-ish flag from Q0: does a
  three_state check row exist anywhere?
- Transient discriminators (in-session only, like async-arc-probe Q2e):
  `set_disable_timing [get_cells u_ebuf] -from A -to Z`, then
  `find_timing_paths -to u_cap1/D` — timed ⇒ `TE_B -> Z` propagates;
  pathless ⇒ it does not. Repeat with `-from TE_B -to Z` on `u_cap3` to see
  whether the *disable* arc (`einvp` `TE`) also propagates. State that these
  commands were transient, not part of the golden SDCs.
- `te_slow.sdc` (`set_input_delay 2.5` on `te`): if enable arcs propagate,
  the `te` path dominates `Z` arrival and `u_cap*/D` max_arr shifts by
  ~2.5 ns vs `ideal.sdc`; if not, values are identical. Compare the two
  golden sections directly — this is the byte-level discriminator.

If propagation is measured, extract per type: which arc(s) propagate
(enable only? both?), the polarity mapping (`three_state_enable` is
`negative_unate` on `ebufn` — does a `TE_B` *fall* produce the `Z` rise
OpenSTA reports?), whether `three_state_disable`'s scalar-0 transition
tables yield slew 0 at `Z`, and whether the arc feeds slew propagation
downstream.

## Q3. `preserve`

Census says zero arcs in the lib — no fixture cell can carry one. Record the
census line only: "`preserve` unmeasurable on sky130_fd_sc_hd; stays counted
in `recognized_check_arcs` definitionally." No probe commands.

## Q4. `Clear`/`Preset` stay count-only

Cheap re-confirmation on this fixture (R6 already proven on `async_arcs`):
`find_timing_paths -to [get_pins u_cap1/D]` path list plus
`report_check_types` — no `Clear`/`Preset`-launched paths. The real
deliverable is our-side: `recognized_check_arcs` on this fixture contains
`Clear`/`Preset` entries (dfbb cells), pinned by the new diagnostic test.
One line in `results.md` points back at the 2026-11-21 R6 entry.

## Q5. Golden + warnings

`run_sta` per SDC emits the `check_arcs.reference.json` mode sections
(endpoints + wns/tns + violation counts + `pulse_width` rows — the
`min_pulse_width` collector is reused verbatim; clock pins on `c1` produce
rows). If Q1 uncovered a data-check surface that emits rows, a second
collector parses it into a `data_checks` section per `fixture.md` §3.
Capture any warnings at `read_sdc`/analysis time verbatim.

## Our-side runs (same fixture, record verbatim)

`grus-eda sta` on all three SDCs with `--json`, plus one `--strict`
invocation. Record: endpoint list, `pulse_width` rows, full
`recognized_check_arcs` map (count, don't predict — expected non-empty for
all deferred types on this fixture), `skipped_endpoints` if any, arc/
endpoint shares, strict verdict. These numbers land in `results.md` and the
diagnostic test's pinned map.

## Recording format

One dated `docs/results.md` entry ("deferred check arcs: non_seq /
three_state / preserve census + probe"): committed script name, the census
table, Q0's command/flag existence list, Q1–Q2 verdicts with decisive
verbatim lines (data-check rows or their absence; path counts through
enable arcs; the `te_slow` vs `ideal` endpoint diff), the `preserve` census
line, the `Clear`/`Preset` one-liner, our-side counts, and which parts of
`generate_ref.sh` ran (AGENTS.md §7.9).
