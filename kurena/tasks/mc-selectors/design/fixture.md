# mc-selectors — Fixture changes (revision)

The first pass committed all scenario SDCs; this revision adds two new
ones and reshuffles which are golden. Everything runs on
`two_clock.def` (cka 2.0 ns on `clk1` → `u_fa0`/`u_fa1`/`u_fy`, ckb
1.0 ns on `clk2` → `u_fb0`/`u_fb1`/`u_fx`, cross paths both directions,
`d_in` clocked by `cka`, `ya`/`yb` output-delayed by `cka`/`ckb`).

## New `t2_*.sdc`

Copy `t2_selbare.sdc`'s shape — the committed boilerplate plus one line:

- `t2_selport.sdc` — `set_false_path -from clk1` (`probe.md` §1: a
  clock's *source-port* name in exception position).
- `t2_selpto.sdc` — `set_false_path -to clk2` (input port on the `-to`
  side).

## `probe.sh`

- `SCENARIOS` (29 → 39): append `t2_selport t2_selpto` and promote
  `t2_selbare t2_selac t2_selunion t2_selunk t2_grp1 t2_grpe t2_grpb
  t2_wavebig` from `PROBE_ONLY`.
- `PROBE_ONLY` (12 → 4): `t2_grpn t2_wave3 t2_wave4 t2_waverev` — the
  error/divergence forms (`probe.md` §2 has the per-name rationale).
- One logic change in `run_sta`: an empty `$endpoints` string emits
  `null` for `wns_max/tns_max/wns_min/tns_min` and `0` violations —
  `t2_selac`/`t2_selunion` cut every launch (`No paths found.`), and an
  unguarded `format "%.6f"` writes a non-numeric value into
  `two_clock.probe.json`. Every scenario with a non-empty table is
  byte-identical before and after.

## Golden expectations the scenarios pin

- `t2_selport`/`t2_selpto`: whatever the probe measures — the full
  8-endpoint table under the expected port-object reading (`probe.md`
  §1 decision table). The golden, not the design, carries the answer.
- `t2_selbare`: the `t2_selfa` drop set (cka launches out: `u_fa0/D`,
  `u_fa1/D`, `u_fb0/D`, `u_fx/D`, `ya` gone; `u_fy/D`, `u_fb1/D`,
  `yb` stay).
- `t2_selac`/`t2_selunion`: union coverage — both domains' launches
  cut; every check whose launch is any clock leaves the table.
- `t2_selunk`: dead rule — the full 8-endpoint `t2_int` table
  (OpenSTA warns, applies nothing).
- `t2_grp1`/`t2_grpe`/`t2_grpb`: the `t2_grp` 5-endpoint table —
  `u_fb0/D`, `u_fx/D`, `u_fy/D` absent (`grpe` additionally pins the
  empty-group arm).
- `t2_wavebig`: the wrapped `{0 0.5}` waveform — expected identical to
  `t2_wave05`/`t2_int` on this all-rise fixture; the golden pins it.

## `tests/designs/README.md`

Update the `sta_clocks` paragraph: scenario counts (40 golden:
39 `two_clock` + `t2_latch`), the `PROBE_ONLY` list shrinks to the
four bail-pinned forms, and the port-name selector rule gets a clause.

## Regen discipline

- Run `probe.sh` as committed inside `ghcr.io/librelane/librelane:3.0.16`
  — the whole script, not a subset (AGENTS.md §7 rules 1, 9).
- `git diff` after regen: the 29 pre-existing `two_clock.probe.json`
  blocks, `two_clock_latch.probe.json`, and all existing
  `probe_t2_*.txt` byte-identical; only new `ref` blocks and the two
  new `probe_t2_selp*.txt` files appear.
