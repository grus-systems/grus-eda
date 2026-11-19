# async-latch-delay — OpenSTA probe

Step 0, run before any implementation (AGENTS.md §2: reference values come
from the committed script run inside `ghcr.io/librelane/librelane:3.0.16`;
stdin Tcl must not use `!` — write `[…] == 0`). Every answer lands in the
dated `docs/results.md` entry; nothing is implemented from guesswork.

## Probe mechanics

Reuse the fixture `generate_ref.sh` collection procs verbatim — the
`find_timing_paths -path_delay max|min` + `data_arrival_time` /
`data_required_time` / `slack` / `slew` accessors already capture every
field a delay check moves. For the first pass, feed one-off heredocs to
`openroad -no_init -no_splash` loading lib/lef/def + `ideal.sdc` (or
`latch.sdc`) plus inline `set_max_delay`/`set_min_delay` commands, and
dump the target endpoint's arr/req/slack next to the baseline values in
`*.reference.json`. Once the verdict is known, the committed scenario
SDCs (`fixture.md`) regenerate the same numbers through the committed
script — the probe pass and the golden pass must agree.

Verdict per (kind, side): **applied** = reported `req`/`slack` differ
from baseline at the target pin; **ignored** = byte-identical endpoint
set; **errored** = `read_sdc` or eval raises (record the `STA-####`
message verbatim, and whether the run still produced the rest of the
report). Also record any `WARNING`/`ERROR` lines OpenSTA prints for the
commands.

## A. async-check pin — `sta_edge/async_reset` on `ideal.sdc`

Target: `u_ff0/RESET_B` (dfrtp recovery/removal endpoint). Baseline:
`max_req 4.900132` (`capture 0.7 + T 4.0 − unc 0.05 − t_recovery`,
t_recovery ≈ −0.250132), `min_req 1.132095`, `max_arr 1.313763` —
launch is the `rst_n` port seed (`set_input_delay 0.5`).

- **A1** `set_max_delay 1.0 -to [get_pins u_ff0/RESET_B]` —
  applied/ignored/errored? If applied, discriminate the req rule:
  - (a) recovery-anchored (the FF formula verbatim):
    `req = capture_min[edge] − unc_setup − t_recovery + N`
    ≈ `0.7 − 0.05 + 0.250132 + 1.0 = 1.900132`;
  - (b) edge-referenced (the output-port shape): `req = N − unc_setup`
    = `0.95`;
  - (c) clock-check req shifted: `req = baseline + N` = `5.900132`;
  - (d) other — solve for the term and record.
  Also note whether `arr` is unchanged (expected: replace semantics —
  candidate keeps its ordinary arrival).
- **A2** `set_min_delay 0.5 -to [get_pins u_ff0/RESET_B]` — candidates:
  (a) `req_min_delay = removal_req_base + N` ≈ `1.132095 + 0.5`;
  (b) `req = N + unc_hold`; (c) unchanged.
- **A3** `set_max_delay 1.0 -from [get_ports rst_n] -to [get_pins
  u_ff0/RESET_B]` — does the `-from` port selector cover the async
  check's launch? Verdict + same req decomposition.
- **A4** `set_max_delay 1.0 -to [get_cells u_ff0]` — `get_cells`
  resolves to every endpoint pin of the instance, i.e. `u_ff0/D` (FF-D,
  known supported) *and* `u_ff0/RESET_B`. If OpenSTA applies to both,
  two rows change; if only to `D`, the async kind is selectively
  skipped — a clean discriminator independent of pin selectors.
- **A5** negative `-from`: `set_max_delay 1.0 -from [get_ports d_in] -to
  [get_pins u_ff0/RESET_B]` — the delay rule's source does not launch
  the async pin; expect the endpoint unchanged (replace semantics is
  per-source). Also `-from [get_pins u_ff0/CLK]`: OpenSTA launch objects
  are clock pins but the async check's launch is the port — expect no
  coverage.
- **A6** repeat A1 on `propagated.sdc` — does the anchor move to the
  propagated capture arrival (`u_ff0/CLK` ≈ 0.713763-ish early arrival
  vs the ideal 0.7)?

## B. latch-D pin — `sta_edge/latch_borrow` on `latch.sdc`

Targets: `u_lp/D` (dlxtp, borrows: baseline `max_req 2.200711`
opening-edge, `max_arr 3.122415`, slack −0.921703), `u_ln/D` (dlxtn,
non-borrowing: `max_req 2.400000`, slack +1.743699), `u_iso/D`
(isolation latch: `max_req = max_arr = 1.655783`, slack exactly 0 — the
stark borrow-clamp case). Launch per pin: `p_lp`/`p_ln`/`p_iso` port
seeds.

- **B1** `set_max_delay 1.0 -to [get_pins u_lp/D]` — verdict. If
  applied, discriminate:
  - (a) closing-edge FF formula verbatim (`check.edge` = FALL for
    dlxtp): `req = capture_min[FALL] − unc − t_setup + N`
    ≈ `2.5 − 0.1 − t_setup + 1.0`;
  - (b) opening-edge anchored: `req = opening_edge_min − unc −
    t_setup + N` ≈ `0 − 0.1 − t_setup + 1.0`;
  - (c) edge-referenced: `req = N − unc` = `0.9`;
  - (d) baseline shifted: `req = 2.200711 + 1.0`.
  And critically: does `slack = req − arr` hold rigidly, or does the
  borrow clamp survive (`req = arr` when inside the window)? The slack
  sign discriminates all four candidates.
- **B2** `set_min_delay 0.5 -to [get_pins u_lp/D]` — candidates:
  (a) `hold_base + N` (`2.566444 + 0.5`); (b) edge-referenced
  `N + unc_hold`; (c) unchanged.
- **B3** `set_max_delay 1.5 -to [get_pins u_ln/D]` — non-borrowing
  latch; separates "delay check anchored like the ordinary check" from
  "delay check is a new check family" and uses a different N for
  attribution clarity.
- **B4** `set_max_delay 1.0 -from [get_ports p_lp] -to [get_pins
  u_lp/D]` — `-from` port coverage on the latch launch.
- **B5** `set_max_delay 1.0 -to [all_registers]` — universal `-to`:
  covers every FF-D (`u_f*`/`u_fn*`), every latch-D (`u_*n`/`u_*p`/
  `u_iso`), and the latch-async pins (`u_rt*/u_rb*/RESET_B`). Per-kind
  verdict at scale; the RESET_B rows double-cover the async verdict on
  latch cells. Record which rows move.
- **B6** B1 on `prop.sdc` — anchor under `set_propagated_clock`.

## What the verdict decides

| outcome | consequence |
|---|---|
| applied (any side) | implement the measured req rule per `api.md`; committed scenario goldens at STRICT |
| ignored / errored | kind stays in `unsupported_path_delay_endpoints`; scenario SDCs + verdict committed as evidence |

If OpenSTA applies on one side only (e.g. max yes, min no), implement the
applied side and keep the other counted — record the asymmetry
explicitly; do not extrapolate one side's verdict to the other.

## Recording format

One dated `docs/results.md` entry: exactly which Tcl ran (the committed
scenario SDCs once they exist; name the ad-hoc commands verbatim
otherwise), the baseline vs reported `arr`/`req`/`slack` at each target
pin, the verdict per (kind, side), the winning req rule with its numeric
decomposition, any OpenSTA warnings, and what was not run.
