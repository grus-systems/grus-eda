# e4-guide-parasitics — Guide → NetRcTree semantics

The rule `estimate_guide_parasitics` implements. Written ahead of the
probe (`probe.md`) as a default model plus explicit knobs; the probe
answers fix each knob and the resolved rule lands in
`docs/e4-guides.md` in the same commit as the code (AGENTS.md §7.6 —
the rule in `docs/` first, then the code; §7.8 — keep docs in step).

## Inputs

- `DesignDb` (placed DEF): net→pin list (`net_pins`, driver at index 0),
  pin positions (`pin_pos_dbu`), per-pin access layer
  (`MacroPin.layer` / `pin_io_layer` — the same rule `build_guides`
  uses for stub layers).
- `TechDb`: `dbu_per_micron`, per-`LayerId` unit RC through
  `WireRc::from_layer`'s formula — `R = res_per_sq / width_um` (kΩ/µm),
  `C = cap_per_sq_um * width_um + 2 * edge_cap_per_um` (pF/µm). Precompute
  one `WireRc` per `routing_layers` entry once, not per box.
- Per net: `&[GuideBox]` — from `build_guides` (`NetGuide.boxes`) or
  `parse_guides` resolved through the name map.

## Per-net graph (default model)

1. **Coverage**: union each net's boxes into a covered-cell set keyed by
   `(gcell_x, gcell_y, layer)`. A box `[xl,yl,xh,yh]` covers cells
   `gcell_xy(xl, yl)` ..= `gcell_xy(xh - 1, yh - 1)` — index the interior,
   never the `xh`/`yh` edge, which sits on the next cell's boundary
   (AGENTS.md §7.5: boundary arithmetic is where the off-by-ones live;
   the die-edge cell where `x_bounds` clamps to `die_xh` is the case to
   check in the unit test).
2. **Nodes**: one node per covered `(gcell, layer)` cell plus one node
   per pin of the net (pin nodes carry no wire cap, matching `pin_node`
   in `NetRcTree`).
   *Knob N1*: the probe may show the reference merges layers into one
   node per GCell — then key coverage by `(x, y)` only and take the
   "wire" layer of the cell for segment RC.
3. **Wire segments**: within one guide box on layer `L`, connect the
   covered GCells along the box's long axis into a chain over
   consecutive cells; each link's length is
   *knob N2* — GCell-center distance (pitch) or box-span share —
   and carries `WireRc` of `L`. Resistance goes on the tree edge;
   capacitance splits half/half onto the two endpoint nodes (the same pi
   convention `build_net_tree` and OpenROAD's SPEF use).
   Overlapping boxes on the same layer dedup: a `(u, v)` cell link is
   charged once.
4. **Vias**: when `(gcell, L1)` and `(gcell, L2)` are both covered with
   `L1`,`L2` adjacent routing layers, add a via link — *knob N3*: 0 Ω or
   the LEF via resistance (the probe reads the value off the SPEF).
5. **Pin attachment**: pin `p` at GCell `g` connects to `(g, access)`
   when covered, else to `(g, met1)` when covered, else *knob N4* —
   nearest covered node of the net (Manhattan, on that layer's chain) or
   a zero-R attach at the driver's cell, matching whatever the probe sees
   for `EST-0026`-warned pins. Attach resistance *knob N5*: 0 Ω, the
   `1e-6` stub style, or a fraction of pitch.
   Stub-layer capacitance *knob N6*: charged or not (probe Q6).
6. **Tree**: BFS from the driver pin's node — the same shape
   `spef_net_tree` produces: `parent`, `edge_r_kohm`, `wire_cap_pf`,
   `pin_node` in `net_pins` order, `total_wire_cap_pf = Σ node caps`.
   Unreached pins are a hard error on the reference guides (they cover
   every pin GCell) and the knob-N4 fallback on ours.

## Skips

`None` for nets with no driver, `< 2` pins, or no boxes — the same
usable-net filter `spef_parasitics` applies (`net_driver` +
`net_pins >= 2`), so `NetParasitics` consumers see the same coverage on
both guide sources and on a SPEF.

## Units

DBU → µm by `dbu_per_micron`; `R * C` lands in ns like the existing
paths (`r_kohm_per_um`/`c_pf_per_um`). `f32`, matching the file's
numeric model.

## Complexity

Per net, work is `O(covered cells)`. picorv32's ~63 k boxes / 6.7 k nets
is small enough to stay serial; the `>= 512`-net `into_par_iter` split
from the sibling builders may be copied if profiling says otherwise —
deterministic either way (collect per net into the `nets` vec).
