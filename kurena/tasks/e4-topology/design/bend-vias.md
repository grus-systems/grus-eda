# Bend via-stacks — bridging `met1`/`met2` at H∩V cells

## Root cause (verified)

A GCell `C` covered by both an H run (`met1` strip) and a V run (`met2`
strip) of the same net produces wire endpoints at `(C,met1)` and
`(C,met2)` — two nodes with **no resistor between them**.
`updateVias` only creates a via from a zero-length segment adjacent to a
segment one layer up (`docs/e4-guides.md` §10); two wire segments can
never pair. So every direction change that does not coincide with a pin
cell (pin stub chains bridge `met1↔met2` already: bound_to_v cells emit
a `met1` rung that pairs into the `met2` wire) splits the net into
disjoint per-layer components.

Verified instance: `_012_` in `gcd.guide.ours.spef` — a 4-node dead
component `{_012_:4,_012_:5,_012_:6,_388_:D}` containing a paired
`li1→met1` stub chain and a `met1` wire: the whole `met1` side branch is
unreachable because its junction to the `met2` route has no via.

OpenROAD's own guide emits the bend cell box on both layers — `_132_` in
`gcd.guide` carries the same `20700 55200 27600 62100` rect on
`met1`,`met2`,`met3`,`met4` — each box a zero-length segment, consecutive
pairs pairing into a via chain. That is the mechanism this change
reproduces under our fixed `met1`/`met2` assignment.

## Emission rule

In `build_guides`, per net:

1. Compute the set `bends` = GCells `(x,y)` covered by **both** an H-run
   strip and a V-run strip of the net — i.e., incident to ≥1 committed H
   edge and ≥1 committed V edge — **excluding** cells in `pin_cells`
   (bound pin cells already bridge the layers through their stub chain;
   unbound pin cells are never strip-covered).
2. For each `B ∈ bends` (deterministic order — sorted `(y,x)` matches
   the existing BTree iteration style), emit two consecutive boxes:
   `met1@B` then `met2@B`, each the 1×1 GCell rect.

Segment-level result (`guide_net_tree` sees): a `met1` zero-length
segment (zls) at B followed by a `met2` zls at B → `updateVias` rule 1
fires → `met1→met2` via
connecting `(B,met1)` and `(B,met2)` — both nodes already interned by
the strips' wire endpoints → the two layer subtrees join.

Residual: the `met2` zls pairs with nothing above it → one self-loop on
its node, R = `via_kohm[met2]` = the via2 cut `0.00341` — on a node that
is now connected, the same pattern the reference shows (`resp_val:2
resp_val:2` on a live node). Self-loop count therefore rises ~1 per
bend while dead *components* collapse — the census (`probe.md`) tracks
both numbers separately.

## Placement in the box stream

The pair only needs the two zls **consecutive** — append all bend pairs
after the `met2` strip section, before the unbound stub groups. Position
is otherwise unconstrained because:

- a following strip wire on `met2` at B does not re-pair the consumed
  zls (each segment pairs once, forward only; `met2z + met2-wire` fails
  both rules — `met2+1 = met3 ≠ met2`);
- a stub group at a different cell is a different point;
- a stub chain top at the **same** cell B is impossible (pin cells are
  excluded from `bends`; a pin cell's stubs are emitted inside the strip
  interleave, not adjacent to the appended pairs).

## Expected effect (hypothesis to confirm by census)

Every junction in the 2D route tree is an H∩V cell, so with bend vias
all strip components chain back to the driver: multi-node islands → ~0
on our emission, at/below the reference residual (gcd 54 multi-node
comps / picorv32 604 nets — their dead multi-node islands are real
route-path features: detour branches on layers the guide covers without
a link back, which we then record as the route-path divergence class).

## Constraints

- `*D_NET` unchanged (zls carry no cap; the added via adds no cap) →
  e4-2 gate stays 269/269 + 6744/6744.
- Box count: +2 per bend cell (dedup no-op — 1×1 stubs differ from the
  covering strip rects). Gate watch: picorv32 baseline `-13.0%` has
  ~2 points of headroom under ±15%; expected +5–10k boxes on 55k →
  lands ~`-4..-8%`. Record the new delta.
- e4-1 weighted share: extra 1×1 `met1`/`met2` boxes may intersect
  reference boxes — share moves, must stay ≥ 0.70 (baseline 0.975/0.960).
- `read_guides` must still accept the file (`check_guide_readback.sh`).
- If a cell is covered by both an H and a V strip but the route uses it
  only as a crossing (no turn), the via is still semantically right:
  the committed edges at the cell belong to the same net.

## Alternative considered (rejected, recorded)

Emitting a single `met1` zls mid-run by splitting the V strip at B
(`[V-prefix, met1z@B, V-suffix]` → the zls pairs *up into* the suffix
wire, zero self-loop residue) needs one box instead of two but couples
the fix to strip splitting and only works at run endpoints — an
interior covered cell has no seg endpoint adjacent to insert at. The
append-pair is position-free and mirrors the reference's same-rect
multi-layer boxes literally.
