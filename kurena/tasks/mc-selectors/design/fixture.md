# mc-selectors — Fixture changes

All on `two_clock.def` (cka 2.0 ns on `clk1` → `u_fa0`/`u_fa1`/`u_fy`,
ckb 1.0 ns on `clk2` → `u_fb0`/`u_fb1`/`u_fx`, cross paths both
directions, `d_in` clocked by `cka`, `ya`/`yb` output-delayed by
`cka`/`ckb`). New `t2_*.sdc` files share the committed boilerplate
(create_clocks, input/output delays, `set_input_transition`,
`set_load`) with only the measured lines appended — copy `t2_grp.sdc`'s
shape.

## New scenarios

Exact names are the implementer's choice; keep the `t2_*` convention.

- `t2_selfa` — `set_false_path -from [get_clocks cka]` (launch-domain
  coverage, incl. the `d_in` port seed question).
- `t2_selfb` — `set_false_path -to [get_clocks ckb]` (capture-domain
  coverage, incl. the `yb` output-port question).
- `t2_selbare` — `set_false_path -from cka` (bare-name clock probe;
  expected either a parse error in OpenSTA or clock anchoring).
- `t2_selac` — `set_false_path -from [all_clocks]` if cheap to add.
- `t2_selmd` — `set_max_delay`/`set_min_delay` with `-from
  [get_clocks ...]` and/or `-to [get_clocks ...]` (split into two
  files if coverage needs isolation).
- `t2_selmc` — `set_multicycle_path -setup 2 -from [get_clocks cka]`
  (+ a `-to` form).
- `t2_grpl`, `t2_grpp` — `t2_grp` with the mode swapped to
  `-logically_exclusive` / `-physically_exclusive`.
- `t2_grpa` — `t2_grp` + `-allow_paths` (accept-or-reject probe).
- `t2_grpd` — `t2_grp` + a `set_max_delay` covering a cross pair
  (precedence probe; add the swapped file-order twin if the first
  answer is order-dependent).
- `t2_wave` — `create_clock -name cka -period 2.0 -waveform {0.3 1.7}`
  (+ unchanged `ckb`): non-trivial duty on a design clock, both
  same-domain and cross-domain endpoints affected.
- `t2_wave05` — `-waveform {0 0.5}` on `cka` (25% duty): rise stays at
  0, fall at 0.5 — pins the two instants independently of each other.
- `t2_waveio` (only if §3 of `probe.md` needs it) — the input delay's
  `-clock` carries a waveform: does `d_in`'s arrival move by the rise
  instant.
- `t2_waveerr` is **not** a fixture — malformed `-waveform` forms are
  parse-side unit tests, not OpenSTA scenarios (unless the probe needs
  OpenSTA's acceptance of `>2`/reversed instants — then make one).

## `probe.sh`

- Extend `SCENARIOS` with the new names; no other logic changes.
- Regen inside `ghcr.io/librelane/librelane:3.0.16` per the committed
  script as written (AGENTS.md §7 rule 1).
- `git diff` audit before commit: the 16 existing scenario blocks in
  `two_clock.probe.json`, `two_clock_latch.probe.json`, and every
  existing `probe_t2_*.txt` **byte-identical**; only new blocks/files
  appear. Name the steps actually run in `docs/results.md` (rule 9).
- `tests/designs/README.md` `sta_clocks` entry: update the scenario
  count and the parse-gate sentence (`t2_grp` now parses).

## Golden expectations the scenarios pin

- `t2_grp`: endpoint table `8→5` — `u_fb0/D`, `u_fx/D`, `u_fy/D`
  absent, wns/tns/violations recomputed on the remaining five.
- Selector scenarios: exact drop/keep sets from the probe; delay-check
  forms pin `arr`/`req` values (anchor at capture edge 0, §1.5 of
  `docs/multi-clock.md`, if the probe confirms clock `-from` keeps the
  clocked launch).
- Waveform scenarios: full per-endpoint field comparison at STRICT —
  `arr`/`req` shift by the waveform instants; the two scenarios
  distinguish rise-instant effects (`{0.3 1.7}` moves both) from
  fall-instant effects (`{0 0.5}` moves only the fall).
