# load-selectors — `sta_exceptions` fixture scenarios

Extend `tests/designs/sta_exceptions/` rather than creating a new
fixture: `exceptions.def` already has the useful single-driver internal
nets (`d1`, `d2`, `y2`) and the committed generator already isolates one
SDC scenario per OpenSTA process.

Every new file is `base.sdc` plus the lines shown below. Keep the
existing `set_load 0.05 [get_ports {y,y2}]` preamble so overlap with
port-load rules is measured, not hidden.

## 1. Scenario table

| file | extra lines | question / expected coverage |
|---|---|---|
| `ld_net.sdc` | `set_load 0.10 [get_nets d1]` | Applied net load on `u_mux/X -> u_ff1/D`; must move `u_ff1/D` arrival/slew vs `base` at STRICT tolerance. |
| `ld_pin_sink.sdc` | `set_load 0.10 [get_pins u_ff1/D]` | Applied sink-pin load; identifies whether OpenSTA charges the pin or attached net. |
| `ld_pin_drv.sdc` | `set_load 0.10 [get_pins u_mux/X]` | Driver-pin domain; distinguishes "pin cap at driver", "attached-net cap", and "no effect". |
| `ld_pins.sdc` | `set_load 0.05 [get_pins {u_mux/X u_ff1/D}]` | Two pins on one net: accumulation, deduplication, and driver+sink overlap. |
| `ld_nets.sdc` | `set_load 0.05 [get_nets {d1 y}]` | Multi-net collection; `y` also has the preamble's port load, exposing domain overlap. |
| `ld_portnet.sdc` | `set_load 0.03 [get_nets y]` | Port load plus net annotation on the same net: additive, replacement, or separate domains. |
| `ld_last.sdc` | `set_load 0.10 [get_nets d1]`<br>`set_load 0.02 [get_nets d1]` | File-order rule for one target: last-wins vs accumulation. |
| `ld_badnet.sdc` | `set_load 0.10 [get_nets {d1 no_such_net}]` | Mixed resolvable/missing collection; diagnostics and measured partial-apply rule. |
| `ld_badpin.sdc` | `set_load 0.10 [get_pins {u_ff1/D u_bad/X}]` | Same for pin specs. |
| `ld_afo.sdc` | `set_load 0.10 [all_fanouts -from [get_pins u_ff1/Q]]` | Fanout-cone selector. If the probe finds another canonical spelling/options, use that; expected our side is a counted whole-command skip. |
| `ld_afi.sdc` | `set_load 0.10 [all_fanins -to [get_pins u_ff1/D]]` | Fanin-cone selector, same counted-skip policy. |
| `ld_fo.sdc` | `set_fanout_load 4 [get_ports y]` | Command-table coverage and measured timing/design-rule effect. |
| `ld_cells.sdc` | `set_load 0.10 [get_cells u_mux]` | Typed but unsupported object domain; must parse and count `set_load`, not abort. |

If the probe shows OpenSTA rejects `all_fanouts`/`all_fanins` or
`set_fanout_load`, keep the scenario and let `generate_ref.sh` record an
`{"error": ...}` entry. If it accepts a different spelling, update the
scenario to the accepted spelling and record the rejected spelling in the
probe notes.

## 2. Why `d1` is the discriminator

`d1` is driven by `u_mux/X` and has one data endpoint, `u_ff1/D`. Its
intrinsic load is only the D pin's Liberty capacitance, so a `0.10 pF`
annotation is large enough to move `u_ff1/D` max/min arrivals and slews
well beyond `max(5 ps, 1%)` while staying inside ordinary sky130 NLDM
ranges. `y` is the overlap case: `u_ff1/Q -> PIN y`, with the port's
existing `0.05 pF` load already present.

If the regenerated golden shows `ld_pin_drv` does not move timing, that
is still a useful measured result; `ld_pin_sink` remains the required
applied pin-load scenario.

## 3. `generate_ref.sh`

Append the scenario names to `SCENARIOS` at the end, in this order:

```text
ld_net ld_pin_sink ld_pin_drv ld_pins ld_nets ld_portnet ld_last
ld_badnet ld_badpin ld_afo ld_afi ld_fo ld_cells
```

No other generator change is needed for the endpoint JSON. If the probe
requires a stage-cap field for a committed record, prefer recording that
in `docs/results.md` from the probe output rather than widening the
golden schema; the existing endpoint comparison is the regression
surface.

Run the script exactly as committed from the repository root. It
self-reexecutes inside the LibreLane image and regenerates
`exceptions.reference.json`. Review with `git diff`: all existing
scenario entries must be unchanged; only the `ld_*` keys and the new SDC
files/script list may be new.

## 4. Diagnostics coverage

The golden cannot express our skip counters. Add diagnostics assertions
in `test_sta_exception_diagnostics`:

- `ld_afo`/`ld_afi`: `sdc_ignored_commands["set_load"] == 1` and
  `is_clean()` is false, unless the probes produce a representable
  selection (then apply it and update this expectation).
- `ld_badnet`/`ld_badpin`: `unresolved_load_selectors` counts the
  measured missing-object granularity; `is_clean()` is false.
- `ld_cells`: `sdc_ignored_commands["set_load"] == 1`.
- `ld_fo`: `sdc_ignored_commands["set_fanout_load"] == 1`; `is_clean()`
  follows the measured timing-effect rule from `api.md`.
- If `ld_badnet`/`ld_badpin` apply the resolvable half, compare their
  moved endpoints to OpenSTA; if they skip the whole rule, compare to
  `base` and still assert the unresolved count.

## 5. Fail-proof requirement

After the implementation is green, deliberately break one load path —
for example, stop adding the resolved net annotation into `net_load` —
and confirm `ld_net` fails the golden comparison. Restore the code and
rerun the focused test. Record this in `docs/results.md`.

## Files

- `tests/designs/sta_exceptions/`
- `crates/grus-core/src/sta/analysis.rs`
- `docs/results.md`
