# load-selectors — OpenSTA probe plan

This is step 0. Do not implement from guesses or from OpenSTA source —
OpenSTA is GPL; use the container's Tcl surface and reports only. Run the
probe inside `ghcr.io/librelane/librelane:3.0.16`, with `${PDK_ROOT}` mounted
as `/pdk`, on `tests/designs/sta_exceptions/exceptions.def`.

## 0. Current failure mode

Before changing code, run our `sta` once on an SDC containing:

```tcl
set_load 0.10 [get_nets d1]
set_load 0.10 [get_pins u_ff1/D]
set_load 0.10 [all_fanouts]
set_fanout_load 4 [get_ports y]
```

Record the exact error and whether the whole SDC aborts (expected:
`value_and_ports` object-type failure or `unsupported command
set_fanout_load`). This is the "before" line in `docs/results.md`.

## 1. Probe harness

Use a throwaway Tcl file or heredoc fed to `openroad -no_init -no_splash`.
The generator's `read_liberty`/`read_lef`/`read_def` preamble already
loads the same sky130 files. Keep stdin Tcl free of `!` history expansion;
write `x == 0` / `x != 0` instead.

For each candidate SDC, run a separate OpenSTA process (or `read_sdc`
inside `catch` and restart between positive cases; SDC state
accumulates). Useful report commands:

```tcl
catch {read_sdc /work/tests/designs/sta_exceptions/$name} err
report_checks -path_delay max -group_path_count 10 \
    -unique_paths_to_endpoint -fields {slew cap fanout input_pins} \
    -digits 6 -format full
report_checks -path_delay min -group_path_count 10 \
    -unique_paths_to_endpoint -fields {slew cap fanout input_pins} \
    -digits 6 -format full
report_check_types -max_fanout -digits 6
report_net d1
```

Endpoint arrivals alone identify whether a load moved timing; the
`Fanout/Cap` stage columns identify which domain OpenSTA charged.

## Q1. `set_load` on `get_nets`

Probe `d1` (`u_mux/X -> u_ff1/D`) because it has one driver and one sink:

```tcl
set_load 0.10 [get_nets d1]
```

Measure against `base.sdc`:

- Does `read_sdc` accept it?
- Does the `u_mux/X` stage `Cap` column become `0.10`, intrinsic+`0.10`,
  or another value?
- Does `u_ff1/D` arrival/slew move by the same amount as a manually
  equivalent capacitance?
- Does `set_load` on `y` (the net also carrying output port `y`) add to
  the existing `set_load 0.05 [get_ports y]`, replace the port load, or
  occupy a separate net-cap domain?

Run a second value (`0.02`) to distinguish *set* from *add*: if the cap
column is `0.02` after `0.10`, the command is last-wins assignment; if it
is `0.12`, annotations accumulate.

## Q2. `set_load` on `get_pins`

Run separately and together:

```tcl
set_load 0.10 [get_pins u_ff1/D]
set_load 0.10 [get_pins u_mux/X]
set_load 0.05 [get_pins {u_mux/X u_ff1/D}]
```

Measure:

- Whether a sink-pin annotation changes the attached net `d1` cap, only
  the sink's downstream path, or is rejected.
- Whether the `0.10` is added to `u_ff1/D`'s Liberty `cap_*` or replaces
  it.
- Whether selecting the driver pin `u_mux/X` does anything OpenSTA
  reports.
- Whether two selected pins on the same net accumulate `0.05 + 0.05`,
  apply once, or produce distinct per-pin effects.
- Whether a top-level port name passed to `get_pins` resolves at all;
  keep `get_ports` as the canonical port form.

## Q3. Missing, duplicate, and mixed objects

```tcl
set_load 0.10 [get_nets {d1 no_such_net}]
set_load 0.10 [get_pins {u_ff1/D u_bad/X}]
set_load 0.10 [get_nets {d1 d1}]
```

Record warning vs error, whether `d1`/`u_ff1/D` still receives the load,
and whether duplicates count once or twice. This decides mixed-collection
and duplicate resolution.

## Q4. `all_fanouts` / `all_fanins`

Probe accepted spellings and options in `catch` first:

```tcl
catch {set fo [all_fanouts]} err
catch {set fo [all_fanouts -from [get_pins u_ff1/Q]]} err
catch {set fo [all_fanout -from [get_pins u_ff1/Q]]} err
catch {set fi [all_fanins]} err
catch {set fi [all_fanins -to [get_pins u_ff1/D]]} err
catch {set fi [all_fanin -to [get_pins u_ff1/D]]} err
```

If a spelling returns a collection, identify whether it contains pins,
ports, nets, or cells (`object_type`/`get_full_name` if available), and
whether `set_load` accepts that collection. Try the option forms OpenSTA
documents for cone queries (`-from`, `-to`, `-levels`, `-pin_levels`,
`-endpoints_only`, `-startpoints_only`, `-trace_arcs`) only enough to
know which ones must be parsed as opaque. Our default remains a counted
`set_load` skip unless the result is a small enumerable set with measured
cap semantics.

## Q5. `set_fanout_load`

```tcl
catch {set_fanout_load 4 [get_ports y]} err
catch {set_fanout_load 4 [get_pins u_ff1/D]} err
catch {set_fanout_load 4 [get_nets d1]} err
report_checks -path_delay max -unique_paths_to_endpoint \
    -fields {slew cap fanout input_pins} -digits 6 -format full
report_check_types -max_fanout -digits 6
```

Measure:

- Whether 3.0.16 accepts the command at all.
- Accepted object types and options.
- Whether endpoint arrival/slew/WNS changes versus `base`.
- Whether only `report_check_types -max_fanout` sees it.

The result decides between "counted non-timing annotation (possibly
`is_clean`-exempt)" and "must model or count as timing-affecting".

## Q6. Qualifier surface

Probe `set_load` forms most likely to appear in generated SDC:

```tcl
set_load -pin_load 0.10 [get_ports y]
set_load -wire_load 0.10 [get_nets d1]
set_load -min 0.10 [get_nets d1]
set_load -max 0.10 [get_nets d1]
set_load -rise 0.10 [get_nets d1]
set_load -fall 0.10 [get_nets d1]
```

Record which are accepted and whether they select a corner/transition or
a pin-vs-wire domain. Unsupported or unmodelled qualifiers should count
the whole `set_load`, not apply to both corners.

## Q7. Parasitic interaction

The acceptance golden is zero-wire, but the API must not silently behave
differently under `NetParasitics`. For one applied net and one applied
pin case, rerun the probe on the same DEF with the closest available
parasitic mode (`read_spef` if a matching SPEF fixture exists, otherwise
placement RC in OpenROAD). Record whether the annotation adds to the
annotated parasitic load, replaces wire capacitance, or changes only the
driver's effective load. If no matching SPEF fixture is practical, state
that parasitic semantics were not measured and choose the conservative
implementation/counting policy in `api.md`.

## Q8. Golden scenarios

After the direct probes, finalize the committed `ld_*` scenarios from
`fixture.md`, add them to `SCENARIOS`, and run
`tests/designs/sta_exceptions/generate_ref.sh` exactly as committed. The
script's `catch {read_sdc}` path already records OpenSTA command
rejections in the golden. Inspect the new entries with a JSON-aware tool:

- at least one applied net/pin scenario moves `u_ff1/D` or `y` arrival
  outside `max(5 ps, 1%)`;
- unsupported selectors either show a reference `error` or baseline
  timing, matching the measured acceptance;
- `set_fanout_load` shows the measured no-op/error outcome;
- every pre-existing scenario key is byte-identical apart from JSON merge
  order (only new `ld_*` keys should appear in `git diff`).

## Results entry format

One dated `docs/results.md` entry must include:

1. the before failure mode and exact command;
2. the container image and whether the probe heredoc or committed script
   was run;
3. Q1–Q7 measured rules, including rejected alternatives;
4. which `ld_*` scenarios are applied vs counted;
5. regenerated golden diff summary and whether OpenSTA rejected any
   scenario;
6. the exact verification commands subsequently run.

## Files

- `tests/designs/sta_exceptions/`
- `docs/results.md`
- `crates/grus-core/src/sta/sdc.rs`
- `crates/grus-core/src/sta/analysis.rs`
