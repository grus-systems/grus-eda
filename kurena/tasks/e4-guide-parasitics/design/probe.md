# e4-guide-parasitics — OpenROAD guide-SPEF probe

Before the builder is coded, record what
`estimate_parasitics -global_routing -spef_file` actually emits. The goal
is the arithmetic rule behind each `*D_NET`, not a vague shape — the same
discipline as the E3.2 placement-RC probe: measure, name the rule in
`docs/`, then implement. Everything runs in the pinned image
(`ghcr.io/librelane/librelane:3.0.16`) with the usual
`docker run -v $REPO:/work -v $PDK_ROOT:/pdk` re-exec; scratch goes to
`tests/designs/gcd/runs/probes/` (gitignored). Tcl on stdin: `!` is
history expansion — write `== 0` (AGENTS.md §7.3).

## Setup

Two SPEFs on `gcd.placed.def` + committed `gcd.guide`, differing only in
the `set_wire_rc` layer:

```tcl
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/gcd/gcd.placed.def
read_sdc /work/tests/designs/gcd/gcd.sdc
set_wire_rc -layer met2          # and a second run with met4
read_guides /work/tests/designs/gcd/gcd.guide
estimate_parasitics -global_routing -spef_file /work/tests/designs/gcd/runs/probes/gcd.guide.spef
exit
```

Plus one tiny case to see structure at print size: a 1-net guide file
holding only `_000_`'s five boxes (the P-GR read-back probe already proved
`read_guides` of a subset file reproduces the same per-net cap, so the
tiny SPEF is trustworthy).

## Questions the dump must answer

For 3–5 nets spanning the size range (`_000_` 2-pin 1-hop, a mid-size
net, the clk tree if present in the guide, one net with detours), dump
the `*D_NET` block and the net's guide boxes side by side:

- **Q1 Node naming/granularity**: are `*CAP` nodes pin names
  (`inst:pin`), synthetic `net:N`, or `net:idx`? How many caps/resistors
  per net vs the net's guide-box count and its covered-GCell count? This
  picks the model's node granularity (per box / per covered GCell / per
  GCell×layer).
- **Q2 Cap structure**: lumped per node, or per-segment values split
  half/half at the ends (the placement SPEF writes the pi split)? Does
  `*D_NET` total equal `Σ*CAP`?
- **Q3 Length rule**: for a straight strip, compute the implied wire
  length `Σcap / c_per_um(layer)` and compare against candidates —
  box span, GCell-center Manhattan distance, span − one pitch, sum of
  GCell pitches. Do the same per resistor if R and C imply the same
  length (`len_r = R / r_per_um`, `len_c = 2·node_cap / c_per_um` for a
  pi split).
- **Q4 Per-layer RC**: does the `set_wire_rc -layer met2` run differ from
  the `met4` run? If yes, each box/segment uses its own layer's LEF RC
  (`res_per_sq`/`cap_per_sq_um`/`edge_cap_per_um` via `WireRc::from_layer`'s
  formula); if no, everything uses the `set_wire_rc` layer — the answer
  decides whether the builder takes one `WireRc` or a per-layer table.
- **Q5 Via model**: a GCell covered on both `met1` and `met2` — is there
  a resistor between the two layer nodes? What value (0, via R from LEF,
  or absent meaning layer-merged nodes)?
- **Q6 Pin attachment**: which node does each `*CONN` pin hang off, with
  what resistance (0, the `1e-6` kohm stub style, or a real segment R)?
  Do stub-layer (`li1`) boxes contribute capacitance?
- **Q7 Uncovered pins**: run the same probe on our emitted
  `gcd.ours.guide` (e4-1 recorded `EST-0026 "Missing route to pin"`
  warnings). Do warned pins still appear in `*CONN`, attached to what,
  or is the net degraded/dropped? Our builder needs the same fallback.
- **Q8 Total-cap decomposition**: solve `*D_NET` cap for `_000_`
  (measured 0.0009510 pF) as Σ over its elements — pins/stubs/segments —
  to get one fully-worked arithmetic anchor, written into
  `docs/results.md`.
- **Q9 Coverage definition**: does a box covering GCells a..b charge
  `b−a` pitches (interior edges only) or `b−a+1` (full span)? The gcd
  boxes are GCell-rect unions, so span = covered × pitch; the difference
  between the two readings is exactly one pitch per strip — visible at
  SPEF precision.

## What gets recorded

`docs/results.md` gains a probe block (before the comparison entry):
date, the exact Tcl run (naming which steps, §7.9), the per-net answers
to Q1–Q9, the `_000_` decomposition, and the resulting model settings.
`docs/e4-guides.md` gains the parasitics section in the same commit —
the rule as measured, not as guessed.

If the SPEF arithmetic stays ambiguous after the dump (e.g. a length
that matches neither candidate), the pinned OpenROAD source may be read
to explain the measured values — `est/` in the pinned revision — and the
explanation recorded as "read from source, arithmetic verified against
the probe". Its code is never copied (AGENTS.md §3).
