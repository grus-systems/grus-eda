# SAIF grammar (accepted) and OpenSTA quirks (measured)

## Grammar our reader accepts

S-expressions. Header items we consume; everything else is ignored.

```
(SAIFILE
 (SAIFVERSION "2.0")
 (DIRECTION "backward")
 (DESIGN)
 (DATE "...")
 (VENDOR "...")
 (PROGRAM_NAME "...")
 (VERSION "...")            ; NOT (PROGRAM_VERSION ...) — OpenSTA rejects it
 (DIVIDER / )
 (TIMESCALE <int> <unit>)   ; unit: s ms us ns ps fs (same set as vcd.rs)
 (DURATION <int>)           ; window length in TIMESCALE units — required
 (INSTANCE <scope>
   [(NET  (<sig> <fields>) ...)]
   [(PORT (<sig> <fields>) ...)]
   (INSTANCE <sub-scope> ...) ...)
 [more INSTANCE roots])
<sig>    = (<name> (T0 i) (T1 i) (TX i) (TZ i) (TB i) (TC i) (IG i))
<fields> = any subset/order of T0 T1 TX TZ TB TC IG
```

Reader rules:

- `TIMESCALE` + `DURATION` are required; `duration_s = DURATION * timescale_s`.
- Durations (`T0`,`T1`,`TX`,`TZ`,`TB`) parse as `f64`; `TC`,`IG` as `u64` —
  a fractional `TC` is a hard error (OpenSTA rejects all floats; we allow
  float durations but not a fractional toggle count).
- `VIRTUAL_INSTANCE` and unknown `(KEYWORD ...)` items inside an `INSTANCE`
  body are skipped (counted nowhere — parser-level, like `readers/vcd.rs`).
  OpenSTA hard-errors on `VIRTUAL_INSTANCE`; we are lenient and document it.
- Signal names may be `\`-escaped tokens; strip the escaping backslashes so
  `name\[0\]` → `name[0]` (matches DEF bus-bit form, and Verilator output).
- `PORT` and `NET` groups are resolved identically by our reader (see
  `api.md`); the divergence vs OpenSTA's PORT-ignoring is documented, since a
  real emitter may put pin data under `PORT`.

## OpenSTA 3.0.16 quirks (probed — do not copy, record)

| Behaviour | Measured result |
|---|---|
| `(PROGRAM_VERSION "x")` header | STA-0169 line-8 syntax error; `(VERSION "x")` accepted |
| `(DURATION 1990.0)` float | syntax error; integer accepted |
| Float in signal fields | syntax error at that line |
| `/` inside an unescaped signal name | syntax error |
| `NET` group, name = top port | annotates the port's pin activity |
| `NET` group, name = internal net (`q`) | silently dropped (no warning/count) |
| `NET` group inside `(INSTANCE _d_)` | name resolves to `_d_`'s **pins** (D/CLK/Q → 3 annotated) |
| `PORT` group anywhere | annotates **nothing** (0 pins, both at top scope and inside instances) |
| `INSTANCE ghost` / unknown signal names | silently dropped, no diagnostic |
| `VIRTUAL_INSTANCE` | STA-0169 parse error; annotations before it survive |
| `read_saif` without `-scope` | "cannot read file" (scope effectively required) |
| `TC` scale | linear in switching power (4 → 40 = 10x) |
| `T0`/`T1` swap | early probe: changes **leakage** (state-dependent) only; committed run: identical columns — record the committed run's result |
| `IG=40`, `TX=40` vs 0 | no change to `report_power` |
| `estimate_parasitics -placement` before `report_power` | no change on mb |

## Consequence for `mb.saif`

The converter must emit, under the selected `dut` scope, a `NET` group with
the four top-level signals (`clk`, `a`, `q`, `y`) — `q` is dropped by
OpenSTA but consumed by us — plus `(INSTANCE _d_)`/`(INSTANCE _i_)` scopes
with `NET` groups named by **pin names** (`D`,`CLK`,`Q` / `A`,`Y`), which is
the only form OpenSTA 3.0.16 annotates. All fields integer.

With `--window-end 1990` (see `fixture.md`) the committed file carries
`(DURATION 1980)`; `clk`/`CLK` get `(T0 990) (T1 990) (TC 199)` — odd `TC`
with a duty tie, so the golden exercises both the odd-split and the
tie→rise rule. `a`/`q`/`D`/`Q`/`A` keep `TC 4` (`T0 1180`, `T1 800`);
`y`/`Y` get `T1 1180` — every tail-state duration loses the last 10 ns of
the truncated window. Confirm against the regenerated file; these numbers
are expected values, not commitments.
