# mc-xdomain — Fixture & reference regeneration

One new scenario file plus regeneration of the committed probe
artifacts. No changes to `two_clock.def`, the Liberty, or the existing
SDCs — the reference runs OpenSTA on the same inputs.

## 1. `tests/designs/sta_clocks/t2_mdx.sdc`

Pin-startpoint `-from` on a cross-domain endpoint: `set_max_delay` and
`set_min_delay` both `-from [get_pins u_fb0/Q] -to [get_pins u_fy/D]`
(ckb→cka). The measured semantic (`probe_t2_mdx.txt`) is *not* the
§1.5 capture-edge-0 anchor — a `get_pins inst/Q` `-from` makes the pin
itself the path startpoint:

- OpenSTA warns `'u_fb0/Q' is not a valid start point` yet reports the
  check with the pin as startpoint at arrival 0 (`Startpoint: u_fb0/Q
  (internal pin)`); `req = N ∓ library constraint` — `max 0.885446 =
  1.0 − 0.114554`, `min 0.962415 = 1.0 − 0.037585` — with no clock
  anchor, latency, or uncertainty. Our report carries the same numbers
  as `arr = ep.dly − pin.dly = 0.0` (the segment is a direct net).
- The pin's clocked launch is *replaced* by pin-segment paths:
  `u_fb1/D` — also launched by `u_fb0/Q` but not named by the `-to` —
  loses both checks and is absent from OpenSTA's endpoint table (the
  golden asserts ours drops it too, via the "reported but absent from
  OpenSTA" check). This is the `pin_from_sig_*` kill (`api.md` §5.2).
- Unrelated endpoints (`u_fx/D`, `u_fb0/D`, …) keep their ordinary
  paired/anchor semantics — `u_fx/D` still shows the A→B min-gap pair
  (launch 4.0/capture 4.2).

```tcl
create_clock -period 2.0 -name cka [get_ports clk1]
create_clock -period 1.4 -name ckb [get_ports clk2]
set_input_delay -clock cka -max 0.5 [get_ports d_in]
set_input_delay -clock cka -min 0.1 [get_ports d_in]
set_output_delay -clock cka -max 0.3 [get_ports ya]
set_output_delay -clock cka -min 0.0 [get_ports ya]
set_output_delay -clock ckb -max 0.3 [get_ports yb]
set_output_delay -clock ckb -min 0.0 [get_ports yb]
set_max_delay 1.0 -from [get_pins u_fb0/Q] -to [get_pins u_fy/D]
set_min_delay 1.0 -from [get_pins u_fb0/Q] -to [get_pins u_fy/D]
```

Same base as `t2_md` so the uncovered endpoints stay byte-identical in
expectation. Cross-domain coverage summary after this ticket:

| scenario | cross endpoints | what they pin |
|---|---|---|
| `t2_int` | `u_fx/D` (cka→ckb), `u_fy/D` (ckb→cka), `d_in`→`u_fb0/D` | pairing on equal grids — B→A launch 1.0→capture 2.0, not edge 0 |
| `t2_frac` | same set | fractional-period pairing — launch 4.0/capture 4.2 (A→B), 9.8→10.0 (B→A) |
| `t2_prop`, `t2_prop1`, `t2_lat` | same set | pairing with propagated / declared network latency |
| `t2_unc` | same set | capture-row uncertainty on foreign checks |
| `t2_uncx` | same set | `-from/-to` directional replacement, one direction only |
| `t2_mcp`, `t2_mcps` | same set | `d_max` on the paired req; `d_min = −nh·Pc` (`−1.037585`) |
| `t2_md`, `t2_mdl` | `u_fx/D` (`-to`), `u_fy/D` (`-from` & `-to` in `t2_mdl`) | delay-check anchor at capture edge 0, no pairing needed |
| `t2_mdo` | none new (output port, same-domain) | unchanged — output delay check stays on the anchor row |
| `t2_mds` | none new (same-domain cell selectors) | unchanged — `get_cells` `-from` stays a launch selector |
| `t2_mdx` (new) | `u_fy/D` both sides | pin-startpoint `-from`: segment `arr 0.0`, `req 0.885446/0.962415`; `u_fb1/D` clocked-launch kill |

## 2. `tests/designs/sta_clocks/probe.sh`

Append `t2_mdx` to the scenario loop — one-word change, no other edits
(Tcl history-expansion and stdin discipline unchanged).

## 3. Regeneration

```sh
# as documented in tests/designs/README.md and docs/results.md —
# LibreLane image, run probe.sh as committed:
docker run --rm -v "$PWD/tests/designs/sta_clocks:/work" -w /work \
  ghcr.io/librelane/librelane:3.0.16 sta -no_init /work/probe.sh
```

(whichever container invocation the committed script assumes — use the
exact recorded command from `docs/results.md` / the `sta_clocks`
fixture doc.)

Discipline (`AGENTS.md` §7 rules 1–2):

- Only `two_clock.probe.json` and `probe_t2_mdx.txt` (new) may change;
  `git diff` on the other `probe_*.txt` and the existing scenario
  blocks must be empty.
- Never hand-enter numbers — `probe.sh` emits them.

## 4. `tests/designs/README.md`

Update the `sta_clocks` entry: cross-domain endpoints are now evaluated
(edge pairing + coincident hold + inter-clock uncertainty), `t2_mdx`
listed, and the remaining deferrals described (unpairable hold parity /
latch / undeclared). List `t2_mdx.sdc` in the file table.

## Files

- `tests/designs/sta_clocks/t2_mdx.sdc`
- `tests/designs/sta_clocks/probe.sh`
- `tests/designs/sta_clocks/two_clock.probe.json`
- `tests/designs/sta_clocks/probe_t2_mdx.txt`
- `tests/designs/README.md`
