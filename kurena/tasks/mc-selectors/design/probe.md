# mc-selectors — Probes required before coding

Every rule below is a *question*; the implementer measures it on OpenSTA
in `ghcr.io/librelane/librelane:3.0.16` by adding scenarios to
`tests/designs/sta_clocks/probe.sh` (fixture list in `fixture.md`) and
records the answers in `docs/results.md` and `docs/multi-clock.md`
§1.6 **before** writing eval code. Nothing in this file may be assumed.

## 1. Clock selectors in `-from`/`-to`

For each of `set_false_path`, `set_max_delay`/`set_min_delay`, and
`set_multicycle_path`, on `two_clock.def` (cka 2.0 ns on `clk1`,
ckb 1.0 ns on `clk2`):

- `set_false_path -from [get_clocks cka]` — which endpoints leave the
  table? Hypothesis (to falsify): every launch tagged `cka` — `u_fa0`,
  `u_fa1`, `u_fy` launches plus the `d_in` port seed (its
  `set_input_delay -clock` is `cka`) — so `u_fa0/D`, `u_fa1/D`,
  `u_fx/D`, `u_fy/D`, `ya` lose their checks. Does OpenSTA also drop
  the *input* check (`d_in`-launched), or only clocked launches?
- `set_false_path -to [get_clocks ckb]` — hypothesis: every endpoint
  whose capture is `ckb` (`u_fx/D`, `u_fb0/D`, `u_fb1/D`, `yb`).
  Confirm whether the output port `yb` (its `set_output_delay -clock`
  is `ckb`) is covered.
- `set_false_path -from cka` (bare name, no `get_clocks`) — does
  OpenSTA resolve a clock name in exception position at all, and does
  it anchor launch or capture? If it parses, compare its drop set
  against the `[get_clocks cka]` form.
- `set_false_path -from [all_clocks]` — drop set.
- `set_max_delay N -from [get_clocks cka]` and `-to [get_clocks ckb]` —
  does a clock `-from` keep the ordinary (clocked) launch with the
  measured edge-0 capture anchor, or become a startpoint form like
  `get_pins inst/Q`? Does `-to [get_clocks ckb]` cover `yb`?
- `set_multicycle_path -setup 2 -from [get_clocks cka]`
  (`-to [get_clocks ckb]` mirror) — does the multiplier scale the
  capture row's period as measured for pin selectors (`t2_mcp`)?
- Multi-name `[get_clocks {cka ckb}]` on `-from` — union coverage.
- Unknown clock `[get_clocks nope]` inside `-from` — OpenSTA error, or
  warning + dead rule? (Current model bails `unknown clock`; match the
  measured severity.)

## 2. `set_clock_groups`

- `t2_grp` (`-asynchronous`, committed): re-confirm the 8→5 table on
  regen — `u_fb0/D`, `u_fx/D`, `u_fy/D` gone, I/O-anchored cross checks
  included, same-domain untouched.
- Modes: repeat the same command with `-logically_exclusive` and
  `-physically_exclusive` — same endpoint drop? If yes they share the
  eval path; if OpenSTA rejects or treats them differently, name the
  bail.
- `-asynchronous -allow_paths` — accepted? Semantics?
- Precedence vs explicit constraints: `-asynchronous` groups **plus**
  `set_max_delay 1.0 -from [get_clocks cka] -to [get_pins u_fx/D]`
  (or a covering `-to`): does OpenSTA still report `u_fx/D` under the
  delay check (explicit wins / groups lose), or is it still dropped
  (groups win)? And swapped file order if the answer is order-
  dependent. Also `-asynchronous` **plus** `set_false_path -from
  [get_clocks cka]` — sanity: same drop.
- Degenerate forms: one `-group` only, empty `-group`, `-group` with a
  bare name instead of `[get_clocks ...]`, missing mode flag,
  `-name` — what does OpenSTA accept?

## 3. `create_clock -waveform`

- `create_clock -name cka -period 2.0 -waveform {0 0.5}` and
  `{0.3 1.7}` on `two_clock.def` (other clock unchanged): record
  per-endpoint arr/req. Hypothesis: same-domain check shifts both
  launch and capture by the rise instant (arr/req +0.3, slack
  unchanged); cross-domain pairs re-enumerate on the shifted launch
  grid. Verify the *fall* instant matters somewhere the fixture can see
  — the committed checks are all rise-triggered, so the fall edge only
  shows via pairing if a fall launch/capture exists; otherwise the
  golden pins the rise instant and the fall instant is covered by the
  offset machinery's symmetry (state this honestly in `results.md`).
- Waveform on the input-delay clock (`-clock cka` with `-waveform`):
  does the `d_in` arrival shift by the waveform rise instant (port
  seeds currently anchor at `latency + in_delay`)?
- Error/edge forms: odd instant count, `>2` instants, instants outside
  `[0, period)`, `{1.7 0.3}` (reversed order) — which does OpenSTA
  accept, and what do the accepted ones compute?

## Method

- New `t2_*.sdc` files per `fixture.md`; `probe.sh` `SCENARIOS`
  extended; `run_sta` + `run_report` produce `ref_*.json` (folded into
  `two_clock.probe.json`) and `probe_*.txt` (the launch/capture clock
  names and edge times — the anchoring evidence).
- Tcl on stdin to `openroad`/`sta`: `!` is history expansion — write
  `[...] == 0` (AGENTS.md §7 rule 3).
- `git diff` after regen: only the new scenario blocks and new
  `probe_*.txt` files may appear.
