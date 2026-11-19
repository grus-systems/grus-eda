# Fixture — `tests/designs/microbench/` SAIF artifacts

## `vcd2saif.py` (exists; add `--window-end`)

python3 exists in the LibreLane image; no other SAIF emitter does
(`overview.md` §measured-4). The script converts `mb.vcd` → `mb.saif`:

- Parse the VCD (`$scope`/`$var`/`$enddefinitions`, `#t` timestamps, scalar
  `0/1/x/z` changes — mb.vcd is all scalars; vectors bail loudly).
- Per declared signal compute over the window `[first_ts, last_ts]`:
  `T0`, `T1`, `TX` (x/z time), `TZ=0`, `TB=0`, `TC` (clean 0↔1 toggle
  count; edges at the window bounds count), `IG=0`.
- **New option `--window-end <ts>`** (VCD timestamp units) overriding the
  aggregation window's `last_ts` and therefore `DURATION`. Events after it
  are ignored by the existing `aggregate()` bound check. Rationale: the
  review requires the committed fixture to exercise the odd-TC rise/fall
  split. `--window-end 1990` truncates the window to `[10, 1990]`
  (`DURATION 1980`), dropping clk's final `#2000` fall → `clk`/`CLK` get
  `TC 199` (odd), `T0 = T1 = 990` (a duty tie — also exercises the tie
  rule). Expected emitted lines for `clk` and `_d_`/`CLK`:
  `(TC 199) (T0 990) (T1 990)`; `a`/`q`/`D`/`Q`/`A` keep `TC 4` with
  `T0 1180`, `y`/`Y` `T1 1180` (each tail-state duration loses the last
  10 ns). Verify these values against the regenerated file before
  committing — do not transcribe them from this doc.
- Emit the OpenSTA-accepted grammar (`saif-format.md`): `(VERSION "vcd2saif")`,
  `(DIVIDER / )`, `(TIMESCALE 1 ns)`, `(DURATION 1980)`, scope tree
  mirroring the VCD (`tb` → `dut` → `_d_`/`_i_`), `dut` signals under one
  `NET` group, `_d_`/`_i_` pin signals under `NET` groups (the only form
  OpenSTA annotates).
- `--ghost-scope tb/dut` stays: emits `(INSTANCE ghost …)` under `dut` —
  OpenSTA drops it silently (measured), our reader counts it.

## `generate_saif_ref.sh` (exists; three fixes)

Self-invokes inside `ghcr.io/librelane/librelane:3.0.16`. Fix list:

1. **Step-1 invocation gains the window:**
   `python3 vcd2saif.py --ghost-scope tb/dut --window-end 1990 mb.vcd mb.saif`.
   Update the header comment's window wording (`[10, 1990]`, `DURATION 1980`).

2. **Fix the "whole stdout" comment** on session B — the committed
   `mb.saif.openroad.txt` is *filtered* output:
   `openroad … | grep -v '^\[INFO\|^\[WARNING\|^The NOWIREEXTENSIONATPIN\|^$'`.
   The comment must say filtered stdout (INFO/WARNING/banner/blank lines
   dropped), and `docs/results.md` must note the same so the "raw file"
   claim is honest. The filter itself stays — the committed file should
   contain the annotation count, report table, and `saif`/`unannotated`
   counts, not tool-version banner noise.

3. **Probe variants mutate by signal name, never positionally** (the
   rejected-review bug). Rewrite the §5 variant table:

   ```python
   import re

   def sig_fields(text, name):
       """Return (span, fields-dict-text) of the `(<name> (T0 ..) ..)` line."""
       m = re.search(rf"\({re.escape(name)} \(T0 (\d+)\) \(T1 (\d+)\) "
                     rf"\(TX (\d+)\) \(TZ (\d+)\) \(TB (\d+)\) \(TC (\d+)\) "
                     rf"\(IG (\d+)\)\)", text)
       assert m, f"signal {name} not found"
       return m

   # duty swap anchored on the signal name — `a` and `y`, whose duty is
   # asymmetric; clk's is degenerate (T0 == T1) under the truncated window,
   # so the old `duty_clk` variant would have been a no-op.
   def duty_swap(text, name):
       m = sig_fields(text, name)
       new = m.group(0).replace(f"(T0 {m.group(1)}) (T1 {m.group(2)})",
                                f"(T0 {m.group(2)}) (T1 {m.group(1)})")
       return text[:m.start()] + new + text[m.end():]

   def igtx(text, name):
       m = sig_fields(text, name)
       new = m.group(0).replace("(TX 0)", "(TX 40)").replace("(IG 0)", "(IG 40)")
       return text[:m.start()] + new + text[m.end():]

   variants = {
       "tc10":    re.sub(r"\(TC (\d+)\)", lambda m: f"(TC {int(m.group(1)) * 10})", text),
       "duty_a":  duty_swap(text, "a"),
       "duty_y":  duty_swap(text, "y"),
       "igtx":    igtx(text, "y"),   # mutates `y`'s fields — the record says `y`
       "port":    …as today (NET→PORT on the two INSTANCE blocks)…,
       "noghost": …as today…,
   }
   ```

   Rules: each variant names the signal it mutates; matches are anchored on
   `(<name> (T0 …) …)` so identical field values on other lines (`y` vs
   `_i_`/`Y`, `clk` vs `_d_`/`CLK`) cannot be hit instead. The probe loop's
   variant list becomes `tc10 duty_a duty_y igtx port noghost`.

Everything else stands: session A (`read_vcd` + `report_power -digits 6`,
cross-checked against committed `mb.openroad.txt` at its 3-digit print
resolution), session B (`read_saif -scope tb/dut` + `report_power -digits 6`
+ `report_activity_annotation`), the Tcl-side table parse into
`mb.saif.reference.json` (`power_vcd` + `power_saif` blocks).

## `mb.power.json`

Unchanged — it is the VCD-path report and no diagnostics field changed in
this revision.

## Test (`power.rs` `#[cfg(test)]`)

`test_saif_power_mb_matches_opensta` — updates for the truncated window:

- `require()` PDK lib/tlef/lef + `design_dir("microbench")` files; panics
  loudly if absent (no skips). Unchanged.
- `load_saif(mb.saif)` → `resolve(&design, None)` → `estimate_power` →
  compare every `power_saif` group field at the current bounds. Unchanged.
- `assert_eq!(diag.unmatched_scopes, 1)` / `unmatched_signals == 0`
  (ghost scope, no ghost signals at top level). Unchanged.
- **Replace the bit-for-bit report equality** with a per-net comparison
  (AGENTS §7 rule 5): `load_vcd(mb.vcd)` and compare `activity.signals`
  per net — every net's `(rise, fall)` must equal the VCD counts *except*
  `clk`, which must be exactly `(vcd.rise, vcd.fall - 1)` = `(100, 99)`
  (the `#2000` fall is outside the `[10,1990]` SAIF window). Then assert the
  SAIF report's total is within a named bound of the VCD report's
  (measured, ~sub-% — the two windows differ in both count and duration);
  record the actual delta in `results.md`. Update the stale comment citing
  `clk TC=200 -> (100,100)`.
- Fail-proof: corrupt `clk` `TC 199`→`398` in a temp copy of `mb.saif` →
  test goes red; restore (values shift with the new fixture).

Also update `saif.rs` tests: `test_split_tc` gains the duty-tie case
(`tc=199, t0 == t1` → `(100,99)`) to lock the tie→rise rule, and
`test_resolve_nets_ports_pins_and_drops`'s inline fixture should mirror the
committed file (`clk`/`CLK` `TC 199`, `T0 990`, `T1 990` → `(100,99)`).
