# mc-cleanups — Container probe plan (revision 3)

One container run of `tests/designs/sta_clocks/probe.sh` inside
`ghcr.io/librelane/librelane:3.0.16`. Revisions 1–2 landed P0–P4; the
only new probe is **P5 — `t2_clkdup`**, the same-anchor same-sense
duplicate-rule case the second review flagged as inferred-not-measured.

## P5 — `t2_clkdup`: same-clock same-sense duplicate rules (M1)

New file `tests/designs/sta_clocks/t2_clkdup.sdc` — copy `t2_int.sdc`
verbatim and replace the `d_in`/`ya`/`yb` delay lines:

```tcl
create_clock -name cka -period 2.0 [get_ports clk1]
create_clock -name ckb -period 1.0 [get_ports clk2]
set_input_delay -min 0.1 -clock cka [get_ports d_in]
set_input_delay -min 0.3 -clock cka [get_ports d_in]
set_input_delay -max 0.7 -clock cka [get_ports d_in]
set_input_delay -max 0.4 -clock cka [get_ports d_in]
set_output_delay -min 0.1 -clock cka [get_ports ya]
set_output_delay -min 0.3 -clock cka [get_ports ya]
set_output_delay -max 0.7 -clock cka [get_ports ya]
set_output_delay -max 0.4 -clock cka [get_ports ya]
set_output_delay 0.5 -clock ckb [get_ports yb]
set_output_delay -max 0.3 -clock ckb [get_ports yb]
set_input_transition 0.1 [get_ports d_in]
set_load 0.05 [get_ports ya]
set_load 0.05 [get_ports yb]
```

Three live hypotheses for a plain rule's effect on same-clock entries:

- **H_acc — accumulate** (current code): a plain rule prunes only
  different-clock entries; same-clock duplicates coexist and each is
  its own check, so the corner reports the worst.
- **H_slot — per-corner-slot overwrite**: a port keeps one delay per
  (clock, corner) slot; a plain rule overwrites the corner(s) it
  covers on the same clock. A `-max` rule after a `Both` entry
  overwrites only the max half — the min value survives.
- **H_overlap — entry-overlap removal**: a plain rule removes same-clock
  entries whose covered corners overlap its own, whole-entry. A `Both`
  entry under a `-max` rule is removed entirely — the min corner loses
  its anchor too (produces `null` min fields).

Dead hypotheses, already refuted by committed evidence: replace-all
and replace-per-clock would both have dropped `t2_int`'s `-min @cka`
entry when `-max @cka` followed — its min corner is timed, so neither
is possible.

**Discriminators** (`ref_t2_clkdup` endpoint block +
`probe_t2_clkdup.txt`; values are the *delay the row reflects* —
exact numbers come out of the probe):

| outcome | `d_in`-launched `max_arr` (`u_fa0/D`, `u_fb0/D`) | `d_in`-launched `min_arr` | `ya` `max_req` | `ya` `min_req` | `yb` `min_req` | `yb` `max_req` |
|---|---|---|---|---|---|---|
| H_acc | `0.7`-class (worst of 0.7/0.4) | `0.1`-class (worst = earliest) | `2.0 − 0.7`-class | `0 − 0.1`-class | `0 − 0.5`-class | `1.0 − 0.5`-class |
| H_slot | `0.4`-class (last wins) | `0.3`-class | `2.0 − 0.4`-class | `0 − 0.3`-class | `0 − 0.5`-class (Both's min half survives) | `1.0 − 0.3`-class |
| H_overlap | `0.4`-class | `0.3`-class | `2.0 − 0.4`-class | `0 − 0.3`-class | `null` (no surviving min entry) | `1.0 − 0.3`-class |

The orderings were chosen so last-wins never coincides with worst:
`-min` duplicates run small→large and `-max` duplicates large→small.
The `yb` `Both`-then-`-max` pair separates H_slot from H_overlap on
the min corner (`−0.5` vs `null`) and both from H_acc on the max
corner (`0.7`-req vs `0.5`-req). `wns_max` corroborates: under H_acc
the `yb`/`ya` `0.5`- and `0.7`-delay checks both exist and the worse
slack enters wns/tns.

Notes for the read-off:

- Under H_acc OpenSTA merges same-clock arrivals into one endpoint row
  (the anchors are identical, so unlike `t2_clkcor2`'s `yb` there is
  only one `Endpoint: yb` row — the worst check's). If the report
  instead shows two rows or the *last* entry's check rather than the
  worst, that is still evidence — record what it shows and follow it.
- `d_in` min `0.1→0.3` and max `0.7→0.4` use the *same* order on the
  input side because the input worst direction is inverted on min
  (earliest arrival is worst for hold).
- If none of the three shapes fits the observed table, implement what
  the data shows and record the rule verbatim — do not force a model.

Joins `SCENARIOS` (all three outcomes are expressible in the existing
entry-list model — `api.md` R6 gives each implementation).

## SCENARIOS / MEASURE_ONLY after this change

```bash
SCENARIOS="…existing 44… t2_clkdup"   # 45 on two_clock.def
MEASURE_ONLY="t2_eclk t2_nsfrom t2_nsto t2_toinv t2_noclk"   # unchanged
PROBE_ONLY="t2_grpn t2_wave3 t2_wave4 t2_waverev t2_eclk3 t2_eclk4"  # unchanged
```

- `two_clock.reference.json` grows 44 → 45 blocks; `evaluated` in
  `test_sta_clocks_match_opensta` 45 → 46.
- All pre-existing blocks must regenerate byte-identical — the fold
  change must not move any committed scenario (none carries a
  same-clock same-sense duplicate; verify via `git diff` anyway).
- `probe_t2_clkdup.txt` (new) and the golden block are this ticket's
  committed evidence (< 1 MB).

## Contingency (container unavailable)

Only if the container genuinely cannot run: skip P5 entirely — **no**
`t2_clkdup.sdc`, no `SCENARIOS` change, `evaluated` stays 45. Keep the
fold and mark it inferred/unmeasured per `api.md` R6's marking path.
Choosing the contingency must be stated in the `docs/results.md`
entry; the `.sdc` spec above stays committed in this design doc for a
later run.

## Files

- `tests/designs/sta_clocks/probe.sh`
- `tests/designs/sta_clocks/t2_clkdup.sdc` (new)
- `tests/designs/sta_clocks/probe_t2_clkdup.txt` (new)
- `tests/designs/sta_clocks/two_clock.reference.json`
- `docs/results.md`
