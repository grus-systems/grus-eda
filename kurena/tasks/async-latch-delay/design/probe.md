# async-latch-delay — OpenSTA probe record & residual checks

The aborted executor pass ran the probe inside the LibreLane container
(`ghcr.io/librelane/librelane:3.0.16`): scenario SDCs equivalent to
`fixture.md` §1 were fed to OpenSTA and the verdicts below are what its
(lost) `analysis.rs` draft encoded. **The numbers were never committed** —
no scenario SDCs, no `*.reference.json` section, no `docs/results.md` entry;
the SDCs and the draft are gone with the run. What survives is this record,
whose baseline values were spot-checked against the committed goldens and
match. The new run must still reproduce every verdict through the committed
`generate_ref.sh` scripts (AGENTS.md §7.1: reference values come from the
committed script run as written) and record everything in
`docs/results.md`. Where regen disagrees with this record, regen wins —
see "If regen contradicts" below.

## Measured verdicts (to be re-confirmed by golden regen)

### A. async-check pin — `sta_edge/async_reset` on `u_ff0/RESET_B`

Baseline (`ideal`): `max_req 4.900132` (`capture 0.7 + T 4.0 − unc 0.05 −
t_recovery`, t_recovery ≈ −0.250132), `min_req 1.132095`,
`max_arr 1.313763`; launch is the `rst_n` port seed.

| scenario | verdict | measured req rule |
|---|---|---|
| `delay_max` (A1) `set_max_delay 1.0 -to u_ff0/RESET_B` | applied | FF formula verbatim: `capture_min[check.edge] − unc_setup − t_recovery + N` — the recovery arc plays `late_arc` |
| `delay_min` (A2) `set_min_delay 0.5 -to …` | applied | `removal_req_base + N` — same `req += N` shift as hold |
| `delay_from` (A3) `-from [get_ports rst_n]` | applied | same rule; the port seed covers the async launch |
| `delay_cell` (A4) `-to [get_cells u_ff0]` | applied on both rows | `u_ff0/D` and `u_ff0/RESET_B` each take their own check's rule |
| `delay_prop` (A6) on `propagated.sdc` | applied | anchor follows the propagated capture arrival |

### B. latch-D pin — `sta_edge/latch_borrow`

Targets: `u_lp/D` (dlxtp, borrows: baseline `max_req 2.200711`,
`max_arr 3.122415`, slack −0.921703), `u_ln/D` (dlxtn, non-borrowing:
`max_req 2.4`, slack +1.743699), `u_iso/D` (borrow-clamped: `max_req =
max_arr 1.655783`, slack 0).

| scenario | verdict | measured req rule |
|---|---|---|
| `delay_max` (B1) `set_max_delay 1.0 -to u_lp/D` | applied | `req = clamp(arr_dly, base, base + pulse − t_setup)`, `base = N + lat0 − setup_uncertainty` — the delay edge opens a window of the latch's own pulse width; borrowing stops at `pulse − t_setup`, not at the clock closing edge. **Sign corrected at regen**: OpenSTA prints the setup term negated ("library setup time -0.18"), which misled the original `pulse + t_setup` note; `delay_allregs` measured the bound (e.g. `u_ln2/D` `0.9 + 3.4 − 0.1645 = 3.235511`) |
| `delay_min` (B2) `set_min_delay 0.5 -to u_lp/D` | applied | `req = N + lat0 + t_hold + hold_uncertainty` — delay-edge anchored, not closing-edge |
| `delay_ln` (B3) `set_max_delay 1.5 -to u_ln/D` | applied | same clamp rule on a non-borrowing shape (`arr_dly < base` clamps at `base`) |
| `delay_from` (B4) `-from [get_ports p_lp]` | applied | port-seed coverage of the latch launch |
| `delay_allregs` (B5) `-to [all_registers]` | applied | covers FF-D, latch-D, and latch `RESET_B` rows |
| `delay_prop` (B6) on `prop.sdc` | applied | `lat0` is the propagated first-edge latency at the clock pin |

Cross-cutting measured facts the implementation encodes:

1. **Delay-check arrival (`dly`)**: a path launched through a latch's
   open edge re-bases to the delay edge — the launch reports at the
   edge's own latency (`lat0`), not the data's open-edge wait. Flop and
   port launches keep their clock edge, so `dly == arr` for them.
   `slack = req − arr_dly` rigidly (no borrow clamp on slack).
2. **`disabled_borrow`**: a `set_max_delay` whose `-to` covers a latch's
   D pin stops the transparent `D -> Q` forward for *every* downstream
   path of the signature class — uncovered endpoints included; only the
   gate's clock-to-Q launch still arrives. `set_min_delay` coverage does
   not disable. Slew still propagates (slew is physical, not
   path-scoped).
3. **Delay edge**: nominal time 0 at the check pin = the *first* edge
   reaching its clock pin (`edge0_pol` resolves it from clock parity —
   matters on inverted clock nets; `async_fall`'s `dfrtn` is the
   regression case for the fallback path).

## Residual ad-hoc probes (still to run — no golden needed)

Run during the same container session as the golden regen; record outputs
verbatim in `docs/results.md`:

- **A5 negative `-from`**: `set_max_delay 1.0 -from [get_ports d_in] -to
  [get_pins u_ff0/RESET_B]` — the source does not launch the async pin;
  expect the endpoint unchanged. Also `-from [get_pins u_ff0/CLK]`: clock
  pins are launch objects but the async launch is the port — expect no
  coverage. Both discriminate per-source replace semantics.
- **`report_checks -path_delay max`** (or `find_timing_paths` field dump)
  on `delay_max`/`delay_allregs` if any golden field mismatches — the
  fastest way to decompose an unexpected `req` into its terms
  (AGENTS.md §7.6).

## If regen contradicts the record

Per (kind, side): an endpoint that comes back byte-identical to baseline
is **ignored** — keep it in `unsupported_path_delay_endpoints`, keep the
committed scenario as the evidence. An `STA-####` raise is **errored** —
emit a `{"error": "<verbatim>"}` section (`sta_exceptions` precedent:
`md_early`, `md_datapath`), pointed at by `results.md`. Never extrapolate
one side's verdict to the other.

## Recording format

One dated `docs/results.md` entry (style: the `pcr_*`/`ld_*` entries):
the committed scenario SDC names + `generate_ref.sh` as the reference
command, the verdict table per (kind, side), each applied rule with its
numeric decomposition at the target pin, the negative-control outputs,
OpenSTA warnings verbatim, ours-vs-golden STRICT comparison, the
fail-proof result, and what was not run.
