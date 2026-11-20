# port-launch-anchor — Scenario files and predicted goldens

Every new file is the shared `base_prop.sdc` preamble plus one command
(same shape as `pmd_*`/`pm2_*`/`pmo_*`/`pml_*`). `SCENARIOS` in
`generate_ref.sh` gains `pmp_port pmp_minport pmp_port2 pmp_alls`
(append at the end, after `pml_min`).

## Derived constants (from the committed golden, for prediction only)

- `t_hold(dfxtp_1) = -0.035445` — `base` `u_ff0/D.min_req` 0.764555
  `= 0.7 + 0.1 + t_hold`.
- Propagated capture-early arrivals (`base_prop` `min_req - 0.1 - t_hold`):
  `u_ff0/CLK` 0.169918, `u_ff1/CLK` 0.183785, `u_ff2/CLK` 0.240998.
- `t_setup` at a `d_in`-driven D pin ≈ 0.12 — `md_port` ideal req
  1.528237 `= 0.7 - 0.05 - t_setup + 1.0` gives 0.121763; `pmd_basic`
  gives 0.120452 at `u_ff1/D`. Exact per-transition values come from the
  regen (propagated slews shift `t_setup` by a few ps).
- Delay-check required (FF endpoint):
  `req = cap_early(endpoint/CLK) - 0.05 - t_setup + N`
  (`pmd_basic`: 1.013333 reproduces exactly at `u_ff1/D`).
- Delay-check required (output port): `req = N - out_delay_max - unc_setup`
  = `N - 0.75` (`pmo_*` reproduces the ideal required times under
  propagation exactly).

## `pmp_port.sdc`

`base_prop.sdc` + `set_max_delay 1.0 -from [get_ports d_in] -to [get_cells u_ff0]`

Predicted under H-src/H-prop (arr = ordinary propagated port arrival):

| pin | max_arr | max_req | max_slack | min side |
|---|---|---|---|---|
| `u_ff0/D` | 0.8 | ~0.998 (`0.169918 - 0.05 - t_setup + 1.0`) | ~+0.198 | `base_prop` row (0.1 / 0.234473 / -0.134473) |
| `u_ff1/D`, `u_ff2/D`, `y`, `y2` | `base_prop` rows verbatim | | | |

Predicted `wns_max` ≈ +0.198 (the delay check undercuts `y2`'s 0.335);
`setup_violations` 0; min side identical to `base_prop`.

Wrong anchors at `u_ff0/D`: H-ideal → arr 1.4, slack ≈ −0.40; H-cap →
arr ~0.87, slack ≈ +0.13. All ≥ 60 ps apart — far above tolerance.

## `pmp_minport.sdc`

`base_prop.sdc` + `set_min_delay 0.5 -from [get_ports d_in] -to [get_cells u_ff0]`

| pin | min_arr | min_req | min_slack | max side |
|---|---|---|---|---|
| `u_ff0/D` | 0.1 | 0.734473 (`0.234473 + 0.5`) | −0.634473 | `base_prop` row |
| others | `base_prop` rows verbatim | | | |

Predicted `wns_min` −0.634473, `tns_min` −0.840026, `hold_violations` 2
(`u_ff0/D` delay check + `u_ff2/D` ordinary hold — the delay check
replaces `u_ff0/D`'s ordinary −0.134). Wrong anchors: H-ideal → arr 0.7,
slack −0.034; H-cap → arr ~0.17, slack −0.564.

## `pmp_port2.sdc`

`pmp_port.sdc` with `set_clock_latency 1.1` instead of `0.6` — the only
line changed. `-source 0.1` unchanged.

Predicted under H-src/H-prop: **every row byte-identical to `pmp_port`**
(the ideal network latency is inert under `set_propagated_clock`; the
fixture's ordinary checks already prove this via `base_prop`, this
scenario re-proves it for delay checks). Under any anchor that carries
the network term, `u_ff0/D.max_arr = 1.9` and slack ≈ −0.90; under H-cap
≈ 0.87 as in `pmp_port`. If the regen shows `pmp_port2` rows differ from
`pmp_port` at all, the anchor is not propagated-clean — decompose before
touching code.

Fallback variant if a reviewer wants the source-latency term perturbed
instead: `set_clock_latency -source 0.35` (network stays 0.6) →
predicted arr 1.05 (propagated) vs 1.65 (ideal) vs ~1.1 (capture-pin);
either variant discriminates, but only one is needed.

## `pmp_alls.sdc`

`base_prop.sdc` + `set_max_delay 1.0 -from [all_inputs]`

`[all_inputs]` covers `d_in` and `sel` (`clk` matches the selector but is
not a launch source — inert). Predicted coverage and numbers under
H-src/H-prop:

| pin | covered by | max_arr | max_req | max_slack |
|---|---|---|---|---|
| `u_ff0/D` | `d_in` | 0.8 | ~0.998 | ~+0.198 |
| `u_ff2/D` | `d_in` | 0.8 | ~1.07 (`0.240998 - 0.05 - t_setup + 1.0`) | ~+0.27 |
| `u_ff1/D` | `sel` (sole covered candidate) | sel's propagated arrival (~0.4 + S->X, < 1.0) | ~1.0 (`0.183785 - 0.05 - t_setup' + 1.0`) | delay check reported — predicted slack ~+0.0..+0.5 < ordinary +1.217 |
| `y2` | `sel` | sel's arrival at `y2` (< `base_prop`'s 1.015388, which is a FF launch — `pml_both` shows the winner is `u_ff2`) | 0.25 | predicted ~−0.6..−0.8 — a real setup violation |
| `y` | uncovered (`u_ff1/Q` only) | `base_prop` row | | |

Predicted `wns_max` ≈ `y2`'s delay slack (~−0.7), `setup_violations` 1;
min side identical to `base_prop`. If `u_ff1/D` or `y2` show non-covered
numbers, the `-from [all_inputs]` resolution diverged — that is itself a
finding; check whether OpenSTA handled the `clk` member before changing
anything (fallback selector `[all_inputs -no_clocks]` is inert for `clk`
on our side too but narrows the experiment, prefer diagnosing first).

## Regen notes

- Run `bash tests/designs/sta_exceptions/generate_ref.sh` from the repo
  root; it re-invokes itself inside `ghcr.io/librelane/librelane:3.0.16`
  (Colima for docker; PDK from `~/.ciel`, `PDK_ROOT` overridable).
- The diff should be **additive only**: four new `pmp_*` keys, no value
  changes in the 102 existing scenarios (preambles are untouched — only
  `pmp_port2` differs and it is a new file). Any drift in existing
  values means the script or fixture broke — stop and investigate.
- `ref_pmp_*.json` temporaries are deleted by the script itself.
