# e6-drt-e2e — instance-hull occupancy model (shipped v1)

The core fix, **landed at `d8d0c6c`**: make the maze's occupancy
model conservative w.r.t. subcell-internal geometry it cannot see.
Inside any instance bbox (including decap/filler/tap cells) block all
maze nodes on the masked layers for every net, except inside the
net's own-pin cover. This file records what actually shipped; the
remaining connectivity work is `pin-access.md`.

## Why the pre-mask model was loose

`premark_obs` rasterizes `mac.obs_rects_raw` → `Obs(cell)` over each
placed instance. The old `hit_verdict` made `Obs(cell)` invisible to
any net owning a member pin on `cell` — the recorded looseness that
let host nets route *through* their own cells' internals (the red
gate's error bboxes inside `_328_`-class instances) and let two
subcell geometry sources go unmodelled (`mac.pin_obs_rects`, and
everything Magic sees inside the master that LEF OBS does not
declare). The delegated reference proves the real rule is finer than
a bbox ban — 234 li1/met1 segments in `gcd.routed.def` cross foreign
instance bboxes through genuine whitespace, Magic-clean — so the
hull is the conservative substitute and the cost is connectivity,
not correctness.

## Shipped model (what `d8d0c6c` implements)

**Hull.** `HullMask` indexes every `{Placed, Fixed, Cover}` cell's
placed outline (`orient_rect` + translate, the `drc::geom` transform)
into y-banded x-interval lists (`HullBand`, `premiy`/`premix` prefix
maxima for skip scans). Point query `hull_cells_at`, legality
`point_allowed` / `seg_allowed`.

**Masked layers = {li1, met1}** — the L1 variant: sky130_fd_sc_hd
masters paint internals only on li1/met1, so met2+ transit over a
cell stays legal detailed routing (the strict all-layer variant
starves every net to graph islands — measured pre-landing). On a
masked layer a node survives inside a hull iff the cell hosts a
member pin of the net **and** the point is inside the net's
`PinCover` rect for that cell — except on *host-open* layers.

**Host-open = {met1}** — `hull_host_open_layer`: inside a foreign
hull met1 stays fully masked, but inside a host hull the whole
interior is open at graph level and `Obs(cell)` marks carry legality
(the delegated routes run met1 access wires through exactly those
OBS-free bands). `mask.carve(l)` = `masked[l] && !open[l]` — the
host-cell `Obs` carve applies on **li1 only** (li1 internals are too
dense to navigate); on met1 the marks block the host too.

**`PinCover`** — `cell → [RectDbu]`, the XY union of the net's member
pin shapes per host cell (not layer-keyed: the delegated routes drop
the whole mcon→met1→M1M2 access stack inside a pin rect's XY on
layers the pin does not declare), grown by `grow_to_track` to hold a
track line of every masked layer and its escape layer — without the
growth a narrow pin rect holds no via-capable crossing (measured:
~half of gcd's sinks `NoPath` on the strict rect).

**Mechanics** — the mask filters `region` pair lists at graph build
(`point_allowed` per node, `hull_masked` census); `transit_extend`
then inserts each host hull's track-grid + index-adjacent
L-corridor on the lowest unmasked layer (met2) so escape vias have
landing nodes. `pin_nodes`, `via_edges`, `relaxed_vias` see only
surviving nodes.

**Occupancy additions** — `mac.pin_obs_rects` chained into
`premark_obs` (source the maze was missing; empty for sky130_hd but
correct); `ObsPin` occupant for instance-pin metal inside hulls —
own net free, foreign **hard** block (a soft price there is a Magic
subcell-overlap finding the maze cannot see); io-port rects keep the
soft `Pin` mark.

**Confinement** — relaxed-via positions/stubs must pass
`point_allowed`/`seg_allowed` on masked layers (candidate list
bounded, nearest-first confined wins); `project_attach` admits only
stubs staying inside own cover; `route_one_net` re-checks committed
projected stubs (`stub_outside_cover` — expected empty, nonzero is
a bug).

## Connectivity cost (measured at d8d0c6c)

`hull_masked=227033` nodes removed on gcd; `unrouted` 0 → 67;
`collisions=0`, `self_drc=0`, `stub_outside_cover=0`. All sampled
failures are `NoPath` pockets — graph-reachable attach sets whose
escape arcs die in `edge_cost`. The pin-access work to bring unrouted
back toward 0 is `pin-access.md` — its guardrail: **the mask itself
(masked layer set, foreign-hull ban, cover semantics) is not the
knob.**

## Residual classes (named, counted, printed)

- `hull_masked` — nodes removed per layer, census print (landed).
- `unrouted` + `unrouted_kinds` — the connectivity cost, per net and
  per `FailKind` (`pin-access.md` step 0 adds the kinds line).
- `stub_outside_cover` — committed stubs violating confinement
  (expected 0 — a nonzero count is a bug, not a residual).
- `boundary_overlap` / `boundary_abut` — the two allowed unclassified
  Magic names, per-input counts in the golden; `ours` target 0.
- Any nonzero `lef_expressible` count post-fix is a gate failure to
  chase, **not** a residual.

## Falsification arms

- Occupancy mask disabled (`--occ` off path) must reproduce the red
  gate's nonzero counts — the deck's nonzero-failure path (E0 lesson
  7; the first-contact RED measurement is itself the demonstrated
  arm, recorded in `docs/results.md`).
- A gate arm asserting `hull_masked > 0` on gcd — the mask fires,
  not vacuously green.

## Files

- `crates/grus-core/src/drt/graph.rs` (landed: `HullMask`,
  `pin_cover`, `transit_extend`, `hull_masked`)
- `crates/grus-core/src/drt/maze.rs` (landed: `ObsPin`, carve gate,
  confinement)
