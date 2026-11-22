# Fixture — `tests/designs/microbench/mbw.*` (a `when`-carrying cell)

## Design

`mbw` = one `sky130_fd_sc_hd__ebufn_1` (`u_t`): `Z = A` while `TE_B` is
low, high-Z while `TE_B` is high. `Z` carries the library's only
`internal_power` `when` shape: an unconditional `related_pin:"A"` block
plus `related_pin:"TE_B"` blocks gated `when:"A"` and `when:"!A"`. One
cell keeps every Combinational column a direct readout of the semantics
under test, and makes the `when` share large enough that ignoring it
moves the column far past the tolerance (needed for the fail-proof).

`mbw.def` — authored, copies `mb.def`'s skeleton (VERSION/DESIGN/
DIEAREA/ROW/TRACKS):

- `PINS`: `a` INPUT, `teb` INPUT, `z` OUTPUT.
- `COMPONENTS`: `u_t` `sky130_fd_sc_hd__ebufn_1` `+ PLACED`.
- `NETS`: `a` `(PIN a)(u_t A)`; `teb` `(PIN teb)(u_t TE_B)`;
  `z` `(u_t Z)(PIN z)`.

`mbw.sdc` — no clock (slews only): `set_input_transition 0.15
[all_inputs]`, `set_load 0.05 [all_outputs]`. **Do not attach a clock to
`a`/`teb`** — OpenSTA derives clock-net activity from the SDC and the
probe would stop measuring the annotation. Fallback ladder if
`sta_pin_slews` or `report_power` misbehaves without any clock:
`create_clock -name vclk -period 20` with no source; then a dedicated
`clk` input port tied to an unused load. Record whichever lands in
`results.md`.

## `mbw.vcd` (authored; the golden input)

`$timescale 1ns`, scope `tb/dut` mirroring `mb.vcd`'s layout: net-level
`$var`s for `a`/`teb`/`z` under `dut`, pin-level `A`/`TE_B`/`Z` under
`u_t` (separate identifier codes, same waveforms written per-id —
OpenSTA annotates pins; our reader keys leaf names, which stay
case-distinct from the net names). `$dumpvars` initials for all signals
(the mb.vcd lesson: no initials → undefined first states → dropped first
edges).

- `a`: rises at 200/1000/1600, falls at 600/1400/1900 → 3R+3F,
  T1 = 1100/2000 → P(A)=0.55 — asymmetric so both `when` branches
  contribute and neither is degenerate.
- `teb`: two disable pulses [700,900] (while `a`=0) and [1100,1250]
  (while `a`=1) — 4 edges; exercises z-excursions on `z` in both `a`
  states so the z-edge question shows up in the golden too.
- `z`: the cell function verbatim — `a` while `teb`=0, `z` while
  `teb`=1. (1→z→1 during the second pulse; 0→z→0 during the first.)
- `#2000` end marker like `mb.vcd`.

## Probe variants (committed; `probe.md` P1–P4)

Tiny authored files sharing the header/scope skeleton:

- `mbw_duty55.vcd`, `mbw_duty80.vcd` — `teb`≡0; same `a` edge count and
  rise times as `mbw.vcd`, different falls (duty 0.55 vs 0.80).
- `mbw_a0.vcd`, `mbw_a0te16.vcd`, `mbw_a1.vcd`, `mbw_quiet.vcd` —
  constant `a`, TE_B edge-count sweep / z-excursion producer.
- `mbw_s00.vcd`/`s01`/`s10`/`s11` — static `(a,teb)` combos for the
  leakage-state probe.

## `generate_when_ref.sh` (committed; emits the golden)

Self-invoking container script, same skeleton as `generate_saif_ref.sh`:

1. Common reads Tcl: tlef + lef + `tt_025C_1v80` lib + `mbw.def` +
   `mbw.sdc`.
2. Golden session: `read_vcd -scope tb/dut mbw.vcd` +
   `report_power -digits 6`; filtered stdout
   (`grep -v '^\[INFO\|^\[WARNING\|…\|^$'`) committed as
   `mbw.openroad.txt` — labelled filtered, not verbatim.
3. `python3 vcd2saif.py mbw.vcd mbw.saif` (no `--window-end`, no ghost
   scope — plain conversion), then a `read_saif` session; both tables
   parsed (the `ROW` regex from `generate_saif_ref.sh` §4) into
   `mbw.reference.json` = `{"power_vcd": …, "power_saif": …}`.
4. Probe loop over the committed variant files — one session each,
   stdout to the console for `results.md` capture; nothing else
   committed.

## `mbw.saif`

Generated inside the container by the committed `vcd2saif.py` (which
computes `T0`/`T1`/`TC` from the waveform — nothing hand-typed). No
`--ghost-scope`/`--window-end` flags: this fixture's odd-TC coverage is
not the point; `mb.saif` already owns that.

## Test (`power.rs` `#[cfg(test)]`)

`test_when_power_mbw_matches_opensta` — same harness as
`test_saif_power_mb_matches_opensta`:

- `require()` the PDK lib/tlef/lef and `design_dir("microbench")` files;
  panics on missing inputs (no skips).
- `parse_def(mbw.def)` → `load_sdc(mbw.sdc)` → `sta_pin_slews` →
  `load_vcd(mbw.vcd)` → `estimate_power` → compare every group of the
  `power_vcd` block in `mbw.reference.json` at STRICT bounds: start from
  internal 2% / switching 5% / total 1% / leakage 25% with the 1.5e-12 W
  floor; tighten to the measured honest bounds before committing.
- `assert_eq!(report.diagnostics.conditional_power_blocks, 0)` — every
  `when` form in the fixture must be expressible under the measured
  rule; only inexpressible forms may remain counted.
- Sanity-check the new state data on the same run: `a`'s duty
  `t1/(t0+t1)` ≈ 0.55; `teb`'s ≈ its waveform duty — the numbers the
  `when` weights actually consume.
- Fail-proof (AGENTS §7 rule 7): force `weight = 1.0` in the `when`
  path → test red; restore. Corrupt `power_vcd` Combinational internal
  in the golden → red; restore.
