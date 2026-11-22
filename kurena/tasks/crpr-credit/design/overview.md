# crpr-credit — Overview

## Goal

Close `docs/path-delays.md` section 5 item 2 ("CRPR credit on delay checks
needs a fixture with nonzero clock-net late/early spread"): measure whether
OpenSTA applies a CRPR credit to `set_max_delay`/`set_min_delay` checks, and
reconcile our delay-check composition (`analysis.rs:3239-3243` comment: the
credit the clock check adds is not reproduced).

## What the ordinary check does today

`collect_endpoints`' `credit_for(trans)` (`analysis.rs:3711-3735`) returns the
early/late spread `arrival_max[trans] - arrival_min[trans]` at the **shared
clock-net driver**, but only when `crpr_sink_net` resolves *every* launch in
the endpoint's combinational cone to flip-flops on that one net
(`CRPR_BAD` on any port, latch, undriven, or different-net source). That is an
all-source consensus, not OpenSTA's per-path credit: OpenSTA credits each path
the spread at the last shared vertex of *that path's* launch and capture clock
paths. On single-net cones (`sta_edge/timing_derate`, `sta_edge/propagated_*`,
all five real designs) the two coincide and are golden-verified (`R7`,
v0.1.62). On `u_ff1/D` — whose cone mixes `u_ff0` on `c1`, `u_ff2` on `c3`,
and port `sel` — consensus is `CRPR_BAD`, so **our ordinary check can never
credit at `u_ff1/D` regardless of spread.** The goal's assumption "if derate
reaches the clock net on both sides the ordinary check already discriminates"
is therefore wrong on this fixture as it stands; a discriminating scenario
must also control *which path wins the report*.

## Spread knob (chosen)

Two lines appended to the `base_prop` preamble in every `pcr_*` file:

```
set_timing_derate -early 0.80 -clock -cell_delay
set_timing_derate -late 1.20 -clock -cell_delay
```

- Only the clock-network cell delays are derated: `u_clkb` (`c1` driver),
  `u_clkb2`, `u_clkb3`. Data-path cells (`clk->q`, `mux2`), cell checks
  (`t_setup`/`t_hold` respond to `-data`/`-cell_check` only) and `-net_delay`
  (no parasitics in this flow) are untouched — every number moves through the
  clock pins alone, so any slack residual is attributable to the credit, not
  to a modelled derate gap.
- Selector semantics already validated both engines on
  `sta_edge/timing_derate` (`set_timing_derate -early 0.85 -clock
  -cell_delay` is a committed, passing scenario line).
- Estimated spread `S` at `u_clkb/X`: `c1`-pin early arrival `0.169918`
  (`pmp_*` entry, `docs/results.md` 2026-11-19) - source latency `0.1` puts
  `d(u_clkb)` ~ `0.070`; `S = (1.20-0.80)*0.070` ~ **28 ps**, ~5x the 5 ps
  tolerance. Verify post-regen (below); if margin < ~10 ps, widen to
  `0.70`/`1.30` before proceeding.

Rejected knobs: `set_clock_latency -early/-late` (our parser refuses it and
root-seeded spread would credit *every* path incl. the `u_ff2` control —
muddier attribution), `-net_delay` (zero parasitics here),
`set_clock_transition` (slew only, no arrival spread).

## Scenarios (5 new, `SCENARIOS` 106 -> 111)

All files = `base_prop.sdc` contents + `set_propagated_clock` (already in the
preamble) + the two derate lines + the per-file extra:

| file | extra lines | what it discriminates |
|---|---|---|
| `pcr_clk` | (derates only) | baseline: spread exists — `u_ff1/D`'s req_max (capture on `c1` *early*) and req_min (*late*) shift in **opposite** directions vs `base_prop`; a uniform derate would shift both the same way. `u_ff1/D` max winner stays the `u_ff2` path (launch `c3`, common prefix with capture `c1` = the `clk` root port only, spread 0) so its slack carries ~no credit either side. |
| `pcr_clkfp` | `set_false_path -from [get_cells u_ff2] -to [get_cells u_ff1]` | drops `u_ff2`'s candidate at `u_ff1/D`; `sel` loses to `u_ff0` (~0.55 vs ~0.75), so the reported max check is the **`u_ff0` path whose launch shares `c1` with the capture**. OpenSTA's slack = un-credited + `S`; ours (consensus `CRPR_BAD`) = un-credited -> expected **red by ~S**, probing per-path-vs-consensus on the ordinary check. |
| `pcr_max` | `set_max_delay 1.0 -from [get_cells u_ff0] -to [get_cells u_ff1]` | the `pmd_basic` shape under spread: the covered `u_ff0` delay check is the reported max candidate at `u_ff1/D` (un-credited slack ~0.24, far below the ~1.2 clock checks). Golden slack = prediction `+ S` **iff** OpenSTA credits delay checks. |
| `pcr_min` | `set_min_delay 0.5 -from [get_cells u_ff0] -to [get_cells u_ff1]` | hold-side twin (`pmd_min` shape: covered check wins the min report, ~-0.16). Golden slack `+ S` iff the hold delay check is credited. |
| `pcr_off` | `set_max_delay 1.0 -from [get_cells u_ff2] -to [get_cells u_ff1]` | control: covered launch on `c3` shares only the `clk` root (spread 0) with capture `c1` — credit must be ~0 even if delay checks are credited. Separates "shared-path credit" from a blanket slack shift. |

(`pcr_off`'s covered source is `u_ff2` on `c3`, not `c2` — the goal text
predates the `u_clkb3` stage added by the diverging-winner fixture.)

## Reading the golden

- Spread exists iff `pcr_clk` `u_ff1/D`/`u_ff0/D`/`u_ff2/D` req_max shifts
  down and req_min up vs `base_prop` (opposite-direction proof).
- Ordinary-check credit: `pcr_clkfp` `u_ff1/D` slack minus a no-credit
  prediction ~ `+S`. If it is `+0`, either the knob made no spread (see the
  opposite-shift check) or OpenSTA does not credit even ordinary checks —
  contradicted by the passing `sta_edge/timing_derate`, so treat as a
  fixture fault first.
- Delay-check credit: `pcr_max`/`pcr_min` slack vs the un-credited
  composition: `+S` = credited, `0` = not. `pcr_off` must stay `0` either
  way.

## Reconcile branches

**A. OpenSTA credits delay checks (expected) — implement per-candidate
credit.** `pcr_clkfp` is also red (ordinary check), and both need the same
machinery: the merged `PinTimingState` does not remember which launch
produced each extreme. Design: per-extreme **launch tags** mirroring the
`par_max`/`par_min` parity masks —

- Intern per run in `StaPrepared`: bit 0 = `CRPR_BAD` (port/latch/unresolved
  seed); bits 1.. = `(clock_net, launch_edge)` per `graph.launches` entry
  (`design.pin_nets[l.clk]`, `l.edge`). `u8` masks cap at 8 tags incl. BAD —
  single-clock designs use <= 3; on overflow seed every launch with BAD
  (conservative = today's answer) and record the bound.
- `PinTimingState` gains `crpr_max`/`crpr_min: [u8; 2]`; `merge()` takes the
  two masks with identical winner-replace/tie-union semantics as `par_*`;
  `merge_all` ORs them (merged state feeds reached-analysis only).
- `credit_for(trans, pol)` reads the data pin's `crpr_max[pol]` (closing req)
  / `crpr_min[pol]` (hold req) instead of `crpr_sink_net(data)`: single-bit,
  non-BAD tag whose `(net, edge)` matches `(capture net, trans)` -> driver
  spread, else 0. `crpr_sink_net`/`crpr_driver_net`/`ff_clk`/`latch_q`/
  `crpr_memo` are then dead — delete them.
- Delay checks: max side gets `+ credit_for(check.edge, pol)` on
  `req` (`analysis.rs:3250`); the tag comes from
  `class_states[ci][ep]` (class-isolated sources give the covered winner's
  tag directly). Min side inherits the credit automatically — `w.req`
  already carries `-credit` from the class's hold req (below).

**B. OpenSTA does not credit delay checks** — `pcr_max`/`pcr_min` green,
`pcr_clkfp` still red. Then tags land for the ordinary check only; the delay
composition must *exclude* the credit: max side unchanged, **min side must
add `credit` back** (`w.req` = `base - credit`, so `req_min_delay` needs
`w.req + credit + n`). Record the measured "no credit" in the docs.

**C. Credit value differs from driver spread** (e.g. partial credit, or a
different common point): record the measured rule in `docs/results.md` first
(AGENTS.md lesson 6: reproduce the reference's arithmetic), then implement.

## Non-goals / notes

- No DEF changes: the spread comes from SDC derates; `exceptions.def` stays.
- Existing scenarios must regenerate byte-identical (each is an independent
  OpenSTA process): the `git diff` on `exceptions.reference.json` must be
  additive-only (new `pcr_*` keys); anything else = nondeterminism, say so.
- `docs/timing-exceptions.md` fixture line says "102 SDC scenarios" (already
  stale at 106; becomes 111) and its "CRPR safety" section describes the
  netlist-consensus rule — update both if tags land.
- Latch/latch-borrow CRPR, multi-clock (>8 tags), and per-edge-parity credit
  stay out of scope (conservative = no credit there, as today).

## Files

- `tests/designs/sta_exceptions/pcr_clk.sdc`
- `tests/designs/sta_exceptions/pcr_clkfp.sdc`
- `tests/designs/sta_exceptions/pcr_max.sdc`
- `tests/designs/sta_exceptions/pcr_min.sdc`
- `tests/designs/sta_exceptions/pcr_off.sdc`
- `tests/designs/sta_exceptions/generate_ref.sh`
- `tests/designs/sta_exceptions/exceptions.reference.json`
- `crates/grus-core/src/sta/analysis.rs`
- `docs/path-delays.md`
- `docs/results.md`
- `docs/timing-exceptions.md`
- `Cargo.toml`
- `Cargo.lock`
