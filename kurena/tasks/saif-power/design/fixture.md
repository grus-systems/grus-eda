# Fixture — `tests/designs/microbench/` SAIF artifacts

## `vcd2saif.py` (exists; two latent-bug fixes)

python3 exists in the LibreLane image; no other SAIF emitter does
(`overview.md` §measured-4). The script converts `mb.vcd` → `mb.saif`:

- Parse the VCD (`$scope`/`$var`/`$enddefinitions`, `#t` timestamps, scalar
  `0/1/x/z` changes — mb.vcd is all scalars; vectors bail loudly).
- Per declared signal compute over the window `[first_ts, last_ts]`:
  `T0`, `T1`, `TX` (x/z time), `TZ=0`, `TB=0`, `TC` (clean 0↔1 toggle
  count; edges at the window bounds count), `IG=0`.
- `--window-end <ts>` (VCD timestamp units) overrides the aggregation
  window's `last_ts` and therefore `DURATION`; events after it are
  ignored by `aggregate()`'s bound check. `--window-end 1990` truncates
  the window to `[10, 1990]` (`DURATION 1980`), dropping clk's final
  `#2000` fall → `clk`/`CLK` get `TC 199` (odd) with `T0 = T1 = 990` (a
  duty tie — the golden exercises both the odd split and the tie→rise
  rule).
- Emit the OpenSTA-accepted grammar (`saif-format.md`): `(VERSION "vcd2saif")`,
  `(DIVIDER / )`, `(TIMESCALE <num> <unit>)` **parsed from `$timescale`**
  (fix 2 below), `(DURATION 1980)`, scope tree mirroring the VCD
  (`tb` → `dut` → `_d_`/`_i_`), `dut` signals under one `NET` group,
  `_d_`/`_i_` pin signals under `NET` groups (the only form OpenSTA
  annotates).
- `--ghost-scope tb/dut` stays: emits `(INSTANCE ghost …)` under `dut` —
  OpenSTA drops it silently (measured), our reader counts it.

### Fix 1 — strip the literal `$end` suffix, not a character set

`handle_item`'s `$var` branch has
`decl = {"name": name.rstrip("$end").strip(), …}`. `str.rstrip` takes a
character set: any signal ending in `e`, `n`, `d`, or `$` loses its tail
(`mode` → `mo`, `en` → ``, `end` → ``). The fused-`name$end` token it
defends against needs a literal suffix strip:

```python
name = words[4].removesuffix("$end")
```

`str.removesuffix` needs Python ≥ 3.9 — check `python3 --version` in the
image during the run; the `name[:-4] if name.endswith("$end") else name`
form is the fallback. The trailing `.strip()` is a no-op on `split()`
output and goes with it. Latent on the committed fixture: mb's names end
in none of those characters, and `$end` already arrives as its own
`words` token — no output change.

### Fix 2 — emit the parsed `$timescale`, not a hardcoded `1 ns`

`parse_vcd` splits `$timescale` into `timescale_s`, then `main` binds it
to `_timescale_s` and emits the literal `(TIMESCALE 1 ns)`. Correct by
accident (mb.vcd is `1ns`), wrong for any other input.

- `parse_vcd`'s `$timescale` handler keeps the emission pair: `num` =
  `spec[:i].strip()`, `unit` = `spec[i:].strip().lower()` (`"1ns"` →
  `("1", "ns")`, `"100ps"` → `("100", "ps")`); validate `unit` against
  `UNITS` via `die()` (the current `UNITS[…]` lookup would surface a
  `KeyError` traceback at the boundary). `timescale_s` has no remaining
  consumer — do not return it (the `float(spec[:i])` conversion stays as
  num validation only). Signature becomes
  `(root, ts_num, ts_unit, first_ts, last_ts)`.
- `main` emits `f"  (TIMESCALE {ts_num} {ts_unit})"`. `DURATION` needs no
  rescaling: `first_ts`, `last_ts`, `--window-end`, and every emitted
  field duration are already in VCD-timestamp units = TIMESCALE units.
  State that invariant in the docstring.
- On mb.vcd (`$timescale 1ns`) the emitted line is byte-identical to the
  hardcoded one — the committed `mb.saif` does not change.

## `generate_saif_ref.sh` (committed; unchanged this pass)

Self-invokes inside `ghcr.io/librelane/librelane:3.0.16`. The script's
first-revision fixes are committed — this pass changes nothing in it,
only the converter it invokes:

1. Step-1 invocation carries the window:
   `python3 vcd2saif.py --ghost-scope tb/dut --window-end 1990 mb.vcd mb.saif`.

2. The session-B comment records that committed `mb.saif.openroad.txt` is
   *filtered* output:
   `openroad … | grep -v '^\[INFO\|^\[WARNING\|^The NOWIREEXTENSIONATPIN\|^$'`
   (INFO/WARNING/banner/blank lines dropped; the file keeps the annotation
   count, report table, and `saif`/`unannotated` split, not banner noise).

3. **Probe variants mutate by signal name, never positionally** (the
   rejected-review bug). The §5 variant table:

   ```python
   import re

   def sig_fields(text, name):
       """Return the re.Match of the `(<name> (T0 ..) ..)` line."""
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

## Test (`power.rs` `#[cfg(test)]` — committed; unchanged this pass)

`test_saif_power_mb_matches_opensta` covers the truncated window:

- `require()` PDK lib/tlef/lef + `design_dir("microbench")` files; panics
  loudly if absent (no skips).
- `load_saif(mb.saif)` → `resolve(&design, None)` → `estimate_power` →
  compare every `power_saif` group field at the current bounds.
- `assert_eq!(diag.unmatched_scopes, 1)` / `unmatched_signals == 0`
  (ghost scope, no ghost signals at top level).
- Per-net comparison (AGENTS §7 rule 5): `load_vcd(mb.vcd)` against
  `activity.signals` — every net's `(rise, fall)` equals the VCD counts
  *except* `clk`, which is exactly `(vcd.rise, vcd.fall - 1)` = `(100, 99)`
  (the `#2000` fall is outside the `[10,1990]` SAIF window); the report
  delta is recorded in `results.md`.
- Fail-proof: corrupting `clk` `TC 199`→`398` turns the test red.

The `saif.rs` tests mirror the committed fixture (`clk`/`CLK` `TC 199`,
`T0 990`, `T1 990` → `(100,99)`, including the `t0 == t1` tie); this pass
adds only the two cases in `steps.md` §1 — fractional `DURATION` rejection
and quoted-string escapes.
