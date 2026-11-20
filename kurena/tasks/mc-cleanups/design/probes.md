# mc-cleanups — Container probe plan

All measurement happens in one container run of
`tests/designs/sta_clocks/probe.sh` inside
`ghcr.io/librelane/librelane:3.0.16`, extended with a `MEASURE_ONLY`
scenario list. MEASURE_ONLY scenarios run `run_report` only — no
`ref_*.json`, so `two_clock.reference.json` is byte-identical by
construction and each new scenario's evidence lands in its own
`probe_<name>.txt`.

```bash
# probe.sh addition, next to PROBE_ONLY
# Measure-only scenarios: pin OpenSTA behaviour for the cleanup items;
# run_report only — never folded into the golden.
MEASURE_ONLY="t2_eclk t2_nsfrom t2_nsto t2_toinv t2_noclk t2_waverot t2_wavez"

for name in $MEASURE_ONLY; do
    run_report "two_clock.def" "$name.sdc" "probe_$name.txt"
done
```

`run_report` already captures stdout *and* stderr (`2>&1`), so OpenSTA
warnings are recorded verbatim in `probe_*.txt`. Every scenario below is
a variant of `t2_int.sdc` — same design (`two_clock.def`), same base
commands, one or two lines changed. The common base is:

```tcl
create_clock -name cka -period 2.0 [get_ports clk1]
create_clock -name ckb -period 1.0 [get_ports clk2]
set_input_delay -min 0.1 -clock cka [get_ports d_in]
set_input_delay -max 0.7 -clock cka [get_ports d_in]
set_output_delay -min 0.1 -clock cka [get_ports ya]
set_output_delay -max 0.7 -clock cka [get_ports ya]
set_output_delay -min 0.1 -clock ckb [get_ports yb]
set_output_delay -max 0.7 -clock ckb [get_ports yb]
set_input_transition 0.1 [get_ports d_in]
set_load 0.05 [get_ports ya]
set_load 0.05 [get_ports yb]
```

(Verify against the committed `t2_int.sdc` before writing the variants —
copy it verbatim and diff, do not reconstruct from memory.)

## P2 — empty clock list (`t2_eclk.sdc`)

Base + appended lines (one `set_clock_*` per command class; a second
block uses `[get_clocks no_such]` for the unknown-name contrast):

```tcl
set_clock_latency 0.5 [get_clocks {}]
set_clock_uncertainty -setup 0.2 [get_clocks {}]
set_clock_transition 0.1 [get_clocks {}]
set_propagated_clock [get_clocks {}]
```

Plus a sibling pass in the same file (or a second scenario
`t2_eclk2` if `read_sdc` aborts mid-file on an error — split the
error-prone lines into separate scenarios if one line's bail hides the
rest):

```tcl
set_clock_latency 0.5 [get_clocks no_such]
```

**Question:** warn (which STA code, per command or per object), silent
no-op, or parse error? Does the command still apply to other rows?

**Discriminator:** `[WARNING …]`/`Error` lines in `probe_t2_eclk.txt`
vs clean output; endpoint table vs `t2_int` baseline (latency would
shift `u_fa*`/`u_fy` arrivals if the empty selection fell through to
*all* clocks — the dangerous silent case to rule out).

If a single error line aborts `read_sdc`, split into `t2_eclk` (empty
list, one command per scenario if needed: `t2_eclk_{lat,unc,tran,prop}`)
and `t2_eclk2` (unknown name) — the split is decided by what
`probe_*.txt` shows, recorded in `docs/results.md`.

## P7 — `-to` resolved-but-invalid warn surface (`t2_toinv.sdc`)

Base + `set_false_path` lines, one per candidate object class:

```tcl
set_false_path -to clk2
set_false_path -to [get_ports clk2]
set_false_path -to d_in
set_false_path -to [get_pins u_fa0/Q]
set_false_path -to [get_pins u_ba/A]
set_false_path -to [get_pins u_fa1/D]
set_false_path -to [get_cells u_buf]
```

(`u_fa0/Q`/`u_ba/A`/`u_buf` — verify names against `two_clock.def`;
`u_fa1/D` is the control: a true endpoint, expected no warning.)

**Questions:** which classes warn `STA-1551`-style (port only? any
non-endpoint pin? cells?), and is the warning once per name or per
command? A second file may add `set_max_delay 1.0 -to clk2` to check
the delay-check path's `-to` surface.

**Discriminator:** count and text of warnings in `probe_t2_toinv.txt`;
endpoint table shows `u_fa1/D` dropped (control works).

## P8 — port-vs-clock namespace priority (`t2_nsfrom.sdc`, `t2_nsto.sdc`)

`t2_nsfrom.sdc`: base with **unnamed** clocks —

```tcl
create_clock -period 2.0 [get_ports clk1]
create_clock -period 1.0 [get_ports clk2]
…same delay/transition/load lines but -clock clk1 / -clock clk2
set_false_path -from clk1
```

`t2_nsto.sdc`: same unnamed clocks + `set_false_path -to clk2`.

**Question:** does a bare name colliding with an unnamed clock's
default name resolve to the port object or the clock object?

**Discriminator:**

- Port wins → `-from clk1` is a dead rule: the full `t2_int`-class
  endpoint table appears (the port launches no data path — `clk1` is a
  clock port).
- Clock wins → `-from clk1` covers the `clk1`-row launches: `u_fa0/D`,
  `u_fa1/D`, `u_fx/D`, `d_in`'s check, `ya` disappear (same set as
  `t2_selfa` with `[get_clocks cka]`).

`t2_nsto` mirrors on `-to clk2` (clock → ckb captures cut; port →
dead + possibly the A7 warning — watch which warning class appears when
the name is *also* a clock name: this tells us whether OpenSTA even
looks at the port object).

## P9 — `-clock`-less input delay under `-waveform` (`t2_noclk.sdc`)

Base with:

```tcl
create_clock -name cka -period 2.0 -waveform {0.3 1.7} [get_ports clk1]
create_clock -name ckb -period 1.0 [get_ports clk2]
set_input_delay -min 0.1 [get_ports d_in]
set_input_delay -max 0.7 [get_ports d_in]
set_output_delay -min 0.1 [get_ports ya]
set_output_delay -max 0.7 [get_ports ya]
set_output_delay -min 0.1 -clock ckb [get_ports yb]
set_output_delay -max 0.7 -clock ckb [get_ports yb]
set_input_transition 0.1 [get_ports d_in]
set_load 0.05 [get_ports ya]
set_load 0.05 [get_ports yb]
```

**Question:** which instant anchors a `-clock`-less
`set_input_delay`/`set_output_delay` when the (first) design clock has a
waveform — the rise instant `0.3`, `0.0`, or the fall `1.7`?

**Discriminator:** `d_in`'s launch arrival into `u_fb0/D` in the
full-format report. `t2_wave` measured the `-clock cka` form shifting
by the `0.3` rise instant; the `-clock`-less row is either
`arr = lat + 0.3 + 0.7 + …` (ours today), `lat + 0.0 + 0.7 + …`, or a
third value. The `ya` required-time anchor discriminates the same way
on the output side (`req = lat + E + 2.0 − 0.7` with `E` the measured
anchor).

## P5 — post-wrap waveforms (`t2_waverot.sdc`, `t2_wavez.sdc`)

`t2_waverot.sdc`: base with `create_clock -name cka -period 2.0
-waveform {1.9 2.1} [get_ports clk1]` (wraps to `[1.9, 0.1]`).
`t2_wavez.sdc`: same with `-waveform {0 2.0}` (wraps to `[0, 0]`).

**Question:** accept (and with what edge instants — reordered? sorted?
collapsed?), warn, or error?

**Discriminator:** `SDC error:` line vs a table; if accepted, the
`cka`-anchored times reveal the effective edges (`u_fa0/D` capture req
= `E_rise + 2.0 − unc − setup`; a clock-line in the full report prints
edge times directly).

## Recording

- Commit the new `t2_*.sdc` + `probe_*.txt` with the implementation —
  they are the measurement evidence (`< 1 MB`, plain text).
- `docs/results.md` dated entry: every probe line verbatim, the warning
  text (with STA codes), the discriminator read-off, and the rule
  implemented (or "no code change — current behaviour matches").
- `docs/multi-clock.md` §1.7 gains the new measured rules in the same
  style as the v0.2.5 revision bullets; its stale "fifteen `t2_*.sdc`
  scenarios" count (~L23) is corrected to the current golden set.
- The `-clock`-less and wrap rules also belong wherever
  `docs/multi-clock.md` describes I/O anchoring and `-waveform`.

## Tcl discipline (repo rules)

- No `!` on stdin → the probe script uses `[…] == 0` / `eq`/`ne`
  comparisons only (existing `run_sta`/`run_report` already comply; new
  code must too).
- `run_report` output goes to `probe_<name>.txt` under
  `tests/designs/sta_clocks/` — same path convention as existing
  probes.
- New scenario names stay `t2_*`-prefixed and are never added to
  `SCENARIOS` (golden) or `PROBE_ONLY` (the bail-pinned test list).

## Files

- `tests/designs/sta_clocks/probe.sh`
- `tests/designs/sta_clocks/t2_eclk.sdc` (new)
- `tests/designs/sta_clocks/t2_nsfrom.sdc` (new)
- `tests/designs/sta_clocks/t2_nsto.sdc` (new)
- `tests/designs/sta_clocks/t2_toinv.sdc` (new)
- `tests/designs/sta_clocks/t2_noclk.sdc` (new)
- `tests/designs/sta_clocks/t2_waverot.sdc` (new)
- `tests/designs/sta_clocks/t2_wavez.sdc` (new)
- `tests/designs/sta_clocks/probe_t2_*.txt` (new outputs)
- `docs/results.md`
- `docs/multi-clock.md`
