# Fixture — `tests/designs/microbench/` SAIF artifacts

## `vcd2saif.py` (new, committed, runs in the container)

python3 exists in the LibreLane image; no other SAIF emitter does
(`overview.md` §measured-4). The script converts `mb.vcd` → `mb.saif`:

- Parse the VCD (`$scope`/`$var`/`$enddefinitions`, `#t` timestamps, scalar
  `0/1/x/z` changes — mb.vcd is all scalars; vectors may bail loudly).
- Per declared signal compute, over `[first_ts, last_ts]` = 1990 ns:
  `T0`, `T1`, `TX` (x/z time), `TZ=0`, `TB=0`, `TC` (clean 0↔1 toggle
  count), `IG=0`.
- Emit the OpenSTA-accepted grammar (`saif-format.md`): `(VERSION "vcd2saif")`,
  `(DIVIDER / )`, `(TIMESCALE 1 ns)`, `(DURATION 1990)`, scope tree
  mirroring the VCD (`tb` → `dut` → `_d_`/`_i_`), `dut` signals under one
  `NET` group, `_d_`/`_i_` pin signals under `NET` groups (the only form
  OpenSTA annotates).
- Optionally include one `(INSTANCE ghost ...)` with a ghost signal: OpenSTA
  drops it silently (measured), our reader counts it — exercises the
  unmatched diagnostics end-to-end on the committed fixture. Decide at
  implementation; if included, assert `saif_unmatched_scopes == 1` in the
  test and record the OpenSTA silent-drop in results.md.

## `generate_saif_ref.sh` (new, committed)

Self-invokes inside `ghcr.io/librelane/librelane:3.0.16` (copy the
`sta_edge/*/generate_ref.sh` preamble verbatim: `REPO_ROOT`, `PDK_HOST_ROOT`,
`docker_cmd` fallback, `-v /work -v /pdk`).

Inside the container:

1. `python3 /work/tests/designs/microbench/vcd2saif.py mb.vcd mb.saif`
   (working dir `/work/tests/designs/microbench`).
2. `openroad` session A: read lef/lib/def/sdc → `read_vcd -scope tb/dut
   mb.vcd` → `report_power` → Tcl parses the group rows into the
   `power_vcd` JSON block. The raw report text role is already served by
   the committed `mb.openroad.txt` — verify the session-A table agrees
   with it, do not commit a second VCD raw file.
3. `openroad` session B: same reads → `read_saif -scope tb/dut mb.saif` →
   `report_power > mb.saif.openroad.txt` → `power_saif` block.
4. Emit `mb.saif.reference.json` (schema in `probe.md` P3) plus the raw
   `mb.saif.openroad.txt` (mirrors the `mb.openroad.txt` precedent).

Report-table parsing: match `^(Sequential|Combinational|Clock|Macro|Pad|Total)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)` on the `> file` output; emit the five columns per group.
**Precision contingency**: `report_power` may not accept a `-digits`
flag in this build — try it first; if unsupported, emit at native
precision and record in `results.md` that the golden's resolution is
print-rounded (the `POWER_SAIF` bounds must then exceed that granularity).

## `mb.power.json` (regenerated)

`PowerDiagnostics` field rename (`driven_nets_without_vcd` →
`driven_nets_without_activity`) + new `saif_unmatched_*` fields change the
schema; regenerate with `grus-eda power --vcd mb.vcd --json mb.power.json`
in the same commit so the committed artifact matches the code.

## Test (`power.rs` `#[cfg(test)]`)

`test_saif_power_mb_matches_opensta`:

- `require()` PDK lib/tlef/lef + `design_dir("microbench")` files; panics
  loudly if absent (no skips).
- `load_saif(mb.saif)` → `resolve(&design, None)` (exercises auto-scope:
  must land on `dut`) → `estimate_power` → compare every `power_saif` group
  field at `POWER_SAIF`.
- Assert `saif_unmatched_*` counts (0, or the ghost counts if the ghost
  scope is included).
- Assert our SAIF report equals our VCD report on `mb.vcd` (same counts →
  identical totals, exact). This holds because `clk`'s odd TC=199 splits
  to (100,99) under the `t0 >= t1` rule — the VCD truth; if the split
  rule ever changes, re-derive this expectation before asserting.
- Fail-proof: corrupt `clk` TC in a temp copy of `mb.saif` → red; restore.
