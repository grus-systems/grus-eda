# xdomain-deferrals — OpenSTA probe plan

One container probe per item, all on the existing `tests/designs/sta_clocks/`
fixtures (`two_clock.def`, `two_clock_latch.def`) via `probe.sh`'s
`run_report` (full-format `report_checks` + `-unconstrained`, stdout+stderr
captured verbatim into `probe_<name>.txt`) and, where the outcome could feed
a golden, `run_sta` (endpoint JSON). SDC contents are in `fixture.md`.

Stdin-Tcl rule (AGENTS.md §7.3): no `!` — write `[...] == 0`. Each scenario
runs in its own `openroad` process (read_sdc accumulates).

## Item 1 — undeclared-launch zero mask

What the deferral is: a reached data extreme whose `dom_*` mask is `0`
(reported as `launch <undeclared>`). Believed unreachable today — the probe
measures the OpenSTA-side corner anyway: a checked endpoint whose data
arrival is launched by something with no declared clock.

- **`t2_u1`** — one-clock SDC (`cka` on `clk1` only; `d_in`/`ya` anchored on
  `cka`, `yb` rules omitted). `u_fb0`/`u_fb1`/`u_fx` are unclocked; `u_fy/D`
  is a cka-captured endpoint whose only launch (`u_fb0/Q`) has no clock.
  Read: does `u_fy/D` (or `u_fx/D`, `u_fb0/D`, `u_fb1/D`, `yb`) appear in
  the timed table at all, in `report_checks -unconstrained`, or produce an
  error? Expected: unconstrained — matching our no-arrival skip (the dead
  launch, not `CrossDomain`).
- **`t2_u2`** — `t2_u1` plus `set_max_delay 1.5 -to [get_pins u_fy/D]` and
  `set_min_delay 0.5 -to [get_pins u_fy/D]`. Read: does the delay-check
  surface time `u_fy/D` despite the unclocked launch? If yes, record the
  reported arrival (raw clk→Q? edge-0 launch?) and required-time anchor —
  that becomes an implement branch.

## Item 2 — latch-D foreign-launch borrow pairing

`two_clock_latch.def` keeps every endpoint same-domain; the deferral is
unexercised today. The probe gives `u_ln/D` (`dlxtn` on `cb` = ckb, opens
low) a foreign launch without touching the DEF:

- **`t2_xlat`** — `t2_latch.sdc` verbatim except `d2`'s
  `set_input_delay` entries anchor on **`cka`** instead of `ckb`. The port
  seed launches on cka while the latch enable sits on ckb → `latch &&
  foreign` deferral on our side.
- **`t2_xlat2`** — same but `-max 1.3` on `d2` (arrival pushed deeper into
  / past the ckb transparent window) — discriminates the borrow cap and the
  launch-edge selection if OpenSTA borrows.

Read in `probe_t2_xlat*.txt`: whether `u_ln/D` reports at all; whether the
report carries `time borrowed from endpoint` / `max time borrow` /
`actual time borrow` blocks (the `t2_latch` evidence lines); which `clock
cka`/`clock ckb` edge times frame the check — that fixes the pairing rule
(launch edge choice vs opening/closing edge choice). If the endpoint is
absent or an error prints, the deferral stays.

Optional discriminator (only if the port-launch result is ambiguous): a
flop-launch variant needs a new DEF (`two_clock_latchx.def`, `u_fa0/Q` also
feeding `u_ln/D`); skip unless the branch decision demands it.

## Item 3 — unpairable hold tying parity

Already measured; no new SDC needed. Committed evidence:

- `probe_t2_waverot.txt` — `Path Type: min` block for `u_fy`:
  `2.00 clock ckb (rise edge)` launch vs `1.90 clock cka (rise edge)`
  capture. cka is `-waveform {1.9 2.1}` on period 2.0 (wraps to
  `[1.9, 0.1]`); ckb period 1.0, LCM 2.0. OpenSTA pairs the **LCM-boundary
  launch edge** (`m·PL == lcm`, which `launch_window`'s
  `tl < lcm·(1-1e-6)` bound excludes).
- `two_clock.reference.json` `t2_waverot.u_fy/D`: `min_arr 2.287085`
  (= `0.287085` clk→Q min + `2.0` launch shift), `min_req 1.862415`
  (= `1.9` capture + `−0.037585` hold).

Equivalent reading (needed for implement): per capture edge in the window,
the smallest launch edge at-or-after it — for `u_fy/D`, capture 1.9 →
launch 2.0, gap 0.1. Also consistent with `t2_wave`'s measured pair
(capture 0.3 → launch 1.0, gap 0.7).

## Item 4 — `grid_lcm` non-convergence

Two near-incommensurate period pairs on `two_clock.def` (all other rules
`= t2_int`):

- **`t2_lcm`** — `cka` 2.0, `ckb` 1.7320508 (≈√3, irrational ratio).
- **`t2_lcm2`** — `cka` 2.0, `ckb` 1.999999 (near-equal → true LCM ~2e6 ns).

Run under a wall-clock `timeout` (probe.sh gains an optional per-call bound)
so an OpenSTA hang is recorded, not fatal. Read: timed table vs warning vs
error vs timeout; if a table appears, the reported launch/capture edge times
reveal the horizon OpenSTA actually searches (e.g. a capped edge count, or a
different pairing window than true LCM).

## Our-side runs (same scenarios, no container)

`grus-eda sta` per new SDC: `t2_u1`/`t2_u2` (expect `u_fy/D` absent with a
no-arrival/`NoPath` skip — the deferral does not fire), `t2_xlat*` (expect
`u_ln/D` `CrossDomain` deferral today), `t2_lcm*` (record whether
`grid_lcm` converges — if it converges to a tiny gcd, note the LCM and the
loop-trip count hazard). `--json` captured; `--strict` verdict noted for the
results entry.

## Artifact discipline

- New scenarios land in a dedicated `XDEFER` list (or the latch-fixture
  block for `t2_xlat*`) in `probe.sh` — `run_report` artifacts only, never
  folded into `*.reference.json` unless the item is implemented.
- `run_report` gains an optional timeout parameter for `t2_lcm*`.
- Rerun `probe.sh` after the change and `git diff` — existing
  `probe_*.txt`/`*.reference.json` must be byte-stable (AGENTS.md §7.1)
  before any commit.
