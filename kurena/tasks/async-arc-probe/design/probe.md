# async-arc-probe — OpenSTA probe (Q1–Q6)

Runs inside `ghcr.io/librelane/librelane:3.0.16` via the committed
`generate_ref.sh` (`run_probe` per SDC). Stdin Tcl rule from AGENTS.md §7.3:
no `!` — write `[...] == 0`. Every `catch` result and every table is pasted
verbatim into the dated `docs/results.md` entry; nothing is implemented from
guesswork.

## Q0. Command surface (discovery, cheap)

Inside `catch`/redirects, record which of these exist and what they return on
this fixture — they decide how raw arrivals at `u_a*/Q*` are read in Q2d:

```
info commands sta::*arrival*
info commands sta::*vertex*
info commands sta::*pin_*
info commands *unconstrained*
info commands *check_types*
```

Vertex objects (from `$pe vertex`) accept `slew <tr> <min|max>` in every
committed script; try `arrival` the same way. To get a vertex for an
arbitrary pin, try `[sta::find_vertex ...]` / `sta::pin_vertex` / whatever Q0
lists. If no pin-level accessor works, the `report_checks -format full`
level tables are the verbatim arrival evidence.

## Q1. Unconstrained reporting (acceptance item 1, second half)

```
report_checks -unconstrained
```

Record verbatim: does it list `u_ar/D`, `u_ar/RESET_B`, `u_ap/D`,
`u_ap/SET_B`, `u_aq/D`, `u_aq/RESET_B`, `u_ds/D`, `u_dp/D`, `u_dq/D`? Under
which headings? This is the reference for our `skipped_endpoints` list —
record ours beside it.

## Q2. Does the async arc yield an arrival at `Q`/`Q_N`?

Discriminators: `u_ar`/`u_ap`/`u_aq` have no clock on `aclk` — any arrival on
their outputs can only come through the `Clear`/`Preset` arc.

- `find_timing_paths -from [get_ports rst_n]` and `-from [get_ports set_n]` —
  `llength` + per-PathEnd pin names. Empty list = reset never starts a path.
- `find_timing_paths -to [get_pins u_ds/D]` (`u_dp/D`, `u_dq/D`) — reachable
  or "No paths found"?
- Raw arrival at `u_ar/Q`, `u_ap/Q`, `u_aq/Q_N` via whatever Q0 surfaced
  (vertex `arrival` accessor, `sta::pin_*`); fall back to
  `report_checks -from rst_n -through u_ar/Q -format full`.
- Clocked control: `find_timing_paths -from rst_n -through u_ff0/Q` (expected
  empty — reconfirms the 2026-11-17 note on this fixture) and
  `report_checks -to u_ff2/D -format full` (shows `u_ff0/Q`'s clock-derived
  level).
- Command-level discriminator (in-session only, not a committed SDC):
  `set_disable_timing u_ff0 -from CLK -to Q`, then re-run
  `find_timing_paths -to u_ff2/D`. `u_ff2/D` is fed only by `u_ff0/Q`, so it
  stays timed iff `RESET_B -> Q` propagates. Record the result, then note the
  command was transient (not part of the golden SDCs).

The verdict is one line: "arrival present on `u_a*/Q*`" or "no arrival" —
plus the verbatim evidence.

## Q3. Falling recovery/removal on the variants that exist

`u_fbn` (`dfbbn_1`, `CLK_N` on `c1`) is the only uncovered falling-check flop
family (census: `dfrtn` already covered; `sdf*` same shape + scan pins;
`dlrbp`/`dlrtp` covered in `latch_borrow`).

```
report_checks -to [get_pins u_fbn/RESET_B] -format full
report_checks -to [get_pins u_fbn/SET_B]   -format full
```

Record launch/capture clock name and edge times — confirm the check anchors
on the falling `CLK_N` transition (the external rising edge), same as
`dfrtn` in `async_fall`. The endpoint JSON fields are the numeric golden.

## Q4. `non_seq_*` checks on `dfbb*`

`dfbbp`/`dfbbn` carry `non_seq_setup_rising`/`non_seq_hold_rising` (the
`SET_B`/`RESET_B` pair checked against each other). Does OpenSTA:

- emit extra `find_timing_paths` endpoints (e.g. a second row at
  `u_fbp/SET_B`/`u_fbp/RESET_B` or a non-seq pin), or
- report them only via `report_check_types` variants (try
  `report_check_types` with and without flags, inside `catch`), or
- not report them at all?

If they appear as endpoints our `recognized_check_arcs` deferral diverges on
endpoint count: record which pins and which values — that becomes a named R6
residual in `docs/roadmap.md`, and the STRICT compare test is skipped for the
affected mode (the fixture still commits; the divergence is the
measurement).

## Q5. Preset-side check pairing

`report_checks -to [get_pins u_fp/SET_B] -format full` and
`u_fbp/SET_B`: rising recovery/removal anchored like `RESET_B` on `dfrtp`?
Record edge names/times verbatim.

## Q6. Golden + warnings

- `run_sta` emits `ideal`/`propagated` sections of
  `async_arcs.reference.json` (endpoints, wns/tns, violation counts, and
  `pulse_width` rows if the collector is included per fixture.md §3).
- Capture any OpenSTA warnings printed at `read_sdc`/analysis time
  ("unconstrained" notices etc.) verbatim — they describe how OpenSTA
  narrates the async-only endpoints.

## Our-side runs (same fixture, record verbatim)

```
cargo run -p grus-eda -- sta \
  --lib  $PDK/sky130_fd_sc_hd__tt_025C_1v80.lib \
  --tlef $PDK/techlef/sky130_fd_sc_hd__nom.tlef \
  --lef  $PDK/lef/sky130_fd_sc_hd.lef \
  --def  tests/designs/sta_edge/async_arcs/async_arcs.def \
  --sdc  tests/designs/sta_edge/async_arcs/ideal.sdc \
  --json /tmp/ours_ideal.json
```

(and `propagated.sdc`; also run once with `--strict` and record the exit
status + bail message).

Record in `results.md`: endpoint pin list, `recognized_check_arcs` map
(expected `Clear`/`Preset`/`NonSeq*` counts — count them, don't predict),
`skipped_endpoints` rows for `u_a*`/`u_d*` pins with their `reason`/`related`,
`total_arcs`/`arcs_skipped` shares, and the `--strict` verdict. Paste the
CLI's printed summary plus the JSON skip block.

## Recording format

One dated `docs/results.md` entry (title: "async-arc probe — R6
re-measurement"): the committed script name, the census table, Q1–Q6 answers
with the decisive verbatim lines (unconstrained list, `-from rst_n` path
count, `u_fbn`/`u_fp` edge pairs, `non_seq` verdict), our-side counts, the
Q2 verdict, and the resulting R6 disposition. If only part of
`generate_ref.sh` ran, name which parts (AGENTS.md §7.9).
