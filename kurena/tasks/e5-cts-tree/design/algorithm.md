# e5-cts-tree — Tree Constructor Algorithm

All construction lives in `crates/grus-core/src/cts/`. `mod.rs` holds the
public surface (`api.md`) and tests; `tree.rs` holds the constructor.

## 1. Clock-root identification — reusing the SDC clock model

```rust
struct ClockRoot {
    clock_name: String,   // `Clock::name` ("core_clk")
    root_net: NetId,      // net driven by that port
    driver_pin: PinId,    // `net_driver(root_net)`
}
// `port_pin` dropped at implementation — the driver lookup supersedes
// it (a port-driven root carries the port pin as its driver).
```

For each `clock` in `sdc.clocks` with `clock.port.is_some()` (design
clocks; virtual I/O clocks have no tree to build):

1. Find the io pin: `db.io_pins.iter().find(|p| resolve(pin_names[p]) == port)`
   — the same resolution `analysis.rs:1396-1406` performs when seeding
   the propagated-clock walk. `bail!("cts: clock `{name}` source
   `{port}` is not an io port")` on miss — loud, not skipped.
2. `db.pin_nets[port]` → root net; `bail!` on `None` (unconnected clock
   port).
3. `db.net_driver(net)` → driver pin; `bail!` when `None` or when a
   second `pin_is_driver` member exists (multi-driver, same refusal the
   mutation API makes).
4. Deduplicate on `NetId`: two `create_clock` rows naming the same port
   share one tree; the report records the first clock's name.

`bail!("cts: no design clock resolved to a port-driven net")` when the
loop produces no root. A root net driven by a *cell* pin
(`pin_cells[driver].is_some()` — an internal/generated clock) is
accepted: the tree hangs off that cell's output exactly as off a port;
`propagated`-flag state is irrelevant to construction.

## 2. Sink collection

`sinks = db.net_pins(root_net)` minus `driver_pin`, **snapshotted** into
a `Vec<PinId>` before any mutation (insert-api lesson: never iterate a
CSR segment while mutating). Positions come from
`db.pin_pos_dbu(pin, tech_db)` — existing cells never move under
pin-and-place legalization, so pre-computed positions stay valid through
construction.

Documented scope: *every* non-driver pin is a sink. On the committed
designs that set is exactly the register clock pins (35 / 1,597). A
clock-as-data tap or a pre-existing buffer input would also be a sink —
the tree still builds legally (the tap's signal is delayed by buffer
levels); §Limitations names this.

## 3. Partition — count-equalized median bisection

`partition(loads: &[(PinId, (i32, i32))], max_loads: usize) -> Vec<Vec<(PinId, (i32, i32))>>`

- `g = ceil(n / max_loads)`. If `g <= 1`, return the single group.
- Else recursively split into `floor(g/2)` + `ceil(g/2)` groups: sort the
  loads by the coordinate of the bbox's **longer axis** (tie: other axis,
  then `PinId`), split at the median so the two sub-problems take
  `floor(g/2)`/`ceil(g/2)` groups respectively; recurse.
- Result: `g` groups whose sizes differ by ≤ 1, each spatially coherent
  (bisection always splits the longer axis). Depth balance is exact —
  every sink descends the same number of levels; cap balance is ±1 pin.

`max_loads = 8` (config). Rationale: TritonCTS's fanout sits at ~7–9
loads/buffer on both designs (35/5 ≈ 7, 1597/197 ≈ 8.1). With `g =
ceil(n/8)`: gcd → 5 leaf groups of 7; picorv32 → 200 leaf groups of ~8,
then 25, then 4, then root — depth 4, ~230 buffers. Recorded choice;
`docs/results.md` shows the measured census next to the reference.

## 4. Level construction (bottom-up)

```
loads := sinks with positions            # (PinId, (x,y))
level := 0
loop:
    groups := partition(loads, max_loads)
    bufs := []
    for (j, g) in groups:
        b   := insert_cell(name(level, j, root), pick_buffer(g), tech, lib)
        m   := create_net(net_name(level, j, root))
        connect_pin(b.out, m)              # driver → index 0
        for p in g: connect_pin(p, m)      # sink → segment end
        seed b at centroid(g)
        bufs.push(b)
    if bufs.len() == 1:
        connect_pin(bufs[0].in, root_net)  # the tree root buffer
        break
    loads := bufs.map(|b| (b.in_pin, b.pos))
    level += 1
```

- `b.in` / `b.out` are the buffer macro's single non-power
  `Input`/`Output` pins — located by the same `is_power`-guarded scan
  `insert_buffer_on_net` performs (`clkbuf_*` power pins are
  `DIRECTION INOUT` in LEF; a direction-only filter misfires). The scan
  is `fn buffer_signal_pins(mac: &MacroDef, tech) -> (MacroPinId, MacroPinId)`
  or simply `cell_pins(b)` filtered by `pin_dirs` + `!is_power`.
- Level-0 members are moved off `root_net` via `connect_pin` (it
  disconnects-then-inserts); level ≥ 1 members are freshly created,
  still-unconnected buffer input pins — `connect_pin` handles `pin_nets
  == None` directly.
- Termination is guaranteed: each level's load count =
  `ceil(prev / max_loads)` strictly decreases for `prev > max_loads`.
- The single top buffer's input lands on `root_net`: the net ends as
  `{driver, root_buf.in}` — the TritonCTS shape (`clk` = `( PIN clk )
  ( clkbuf_0_clk A )`).
- `tree_depth = level + 1` (buffer count on the longest root→sink path;
  matches the reference's `CTS-0102` "Path depth" convention).
- Why not `insert_buffer_on_net` for k-way splits: it moves **all**
  sinks of a net to one new net — repeated calls produce a serial
  buffer chain, not a fanout. The constructor uses the primitive triple
  uniformly for every group (it *is* `insert_buffer_on_net`'s
  `insert_cell`+`create_net`+`connect_pin` internals, applied
  group-wise). Recorded deviation from the goal's literal wording.

## 5. Buffer sizing — recorded rule

`pick_buffer(group, cfg)`:

- `C_est = Σ lib pin cap_nom of group sinks + wire_cap`, with
  `wire_cap = WireRc::from_layer(tech, cfg.wire_layer).c_pf_per_um ×
  group_bbox_half_perimeter_um`. Sink `cap_nom` comes via
  `db.pin_lib_pins[p]` → `lib.pins[lp].cap_nom`; an unmapped sink
  contributes 0 (still counted in `sinks`; name the assumption in
  `e5-cts.md`).
- Choose the **first** buffer in `cfg.buf_list` (ordered strongest →
  weakest: `clkbuf_8`, `clkbuf_4`, `clkbuf_2`) whose output-pin Liberty
  `max_cap` (`LibPin::max_cap`) ≥ `C_est`; fall back to the list's first
  entry when no entry covers the load or `max_cap` is absent.
- The **top** buffer of each tree is `cfg.root_buf` (`clkbuf_16`),
  mirroring `-root_buf`.
- Recorded expectation: our census will skew smaller than TritonCTS's
  (it picked `clkbuf_16`/`clkbuf_8` almost exclusively); the report's
  per-type census is the honesty record, and ticket 4's e2e STA says
  whether it matters. Not a pass/fail gate.

## 6. Naming and determinism

- Buffer: `clkbuf_{ctr}_{root_net_name}`; its net:
  `clknet_{ctr}_{root_net_name}`; `ctr` is a per-tree counter assigned
  level-major (level 0 groups in partition order, then upward; the top
  buffer takes the last index). TritonCTS's own root names
  (`clkbuf_0_clk`, `clknet_0_clk`) are the model.
- Names pass through the interner and `escape_def_name` at emission;
  unusual root-net characters (`[`, `]`) round-trip. A name collision
  makes `insert_cell`/`create_net` bail loudly — the honest failure for
  re-running CTS on an already-buffered design; name it in the commit.
- Determinism: partition sorts on `(x, y, PinId)` — fully ordered;
  group → name → net order is sequential; Abacus is deterministic.
  `docs/results.md` records a rerun `git diff` state on the emitted DEF
  (expect byte-identical).

## 7. Position seeding + pin-and-place legalization

- Each buffer's `cell_x_dbu`/`cell_y_dbu` is seeded at the centroid of
  the loads it drives (level 0: mean of sink pin positions; upper
  levels: mean of child-buffer seed positions). Status stays `Unplaced`
  until legalization — `pin_pos_dbu` resolves positions regardless.
- Legalize: snapshot every non-inserted `Placed`/`Unplaced` cell → set
  `Fixed` (regardless of macro class — `Fixed`/`Cover` are the only
  statuses Abacus buckets as row obstacles, so a `Placed` welltap or
  spacer left unpinned would be neither movable nor obstacle); build
  `gp_x_um/gp_y_um = cell_*_dbu / dbu_per_um` for all cells;
  `legalize_placement(&mut db, tech, &gp_x, &gp_y)` (it self-verifies —
  under the pinned statuses, so `run_cts` re-runs `verify_legality`
  after the restore); restore each cell's original status. Only the
  inserted buffers are movable — Abacus drops them into free sub-row
  gaps near the seeds.
- Why pin-and-place (the acceptance needs it literally): with all cells
  movable, Abacus may shift pre-existing cells → component lines would
  differ and control-net HPWL would move. Pinned, the emitted DEF's
  non-clock component lines and non-clock net lines are byte-identical
  and every untouched pin keeps its position — so "control net HPWL
  unchanged" holds in the emitted DEF itself, not just pre-legalization.
- Divergence recorded: OpenROAD displaces neighbors freely (its
  `gcd.cts.def` moves pre-existing cells). At ~60-65% utilization free
  gaps exist near any seed; a dense design could push buffers off their
  cluster — stated in `e5-cts.md`, not claimed as quality-equivalent.

## 8. Metrics (computed post-legalization, inside `run_cts`)

- `tree_buffers`: census of inserted cells by macro name.
- `clock_subnets` / `inserted_instances`: created nets / inserted cells
  (equal — each buffer creates exactly one net; matches the reference's
  `clock_subnets == tree_buffers` relation).
- `sinks`, `tree_depth` as above.
- `avg_sink_wirelength_um`: mean over sinks of Manhattan distance from
  the sink pin to its driving leaf buffer's output-pin position
  (`pin_pos_dbu`, post-legalize). Recorded definition; the reference's
  `CTS-0101` "Average sink wire length" is the same quantity class.
- `clock_net_wirelength_um`: Σ per-net HPWL (`pin_pos_dbu` bbox, same
  arithmetic as `compute_hpwl_dbu` restricted to the tree's nets: root
  net + all created nets). The reference census HPWL'd "every net
  attached to an inserted instance" — the same set. Basis: `"hpwl"`.
- `def_components`, `def_signal_nets`: `num_cells()`, `num_nets()` post-
  mutation.

## Edge cases & limitations (all recorded, none silent)

- **0 sinks** on a root net: skip that clock (the tree is empty);
  reported `sinks` reflects it. **1..=max_loads sinks**: single buffer —
  the `bufs.len() == 1` exit — depth 1.
- **Multi-clock**: each design clock gets an independent tree; report
  fields aggregate (`clock` = comma-joined names, per-tree breakdown in
  the internal run data). Committed designs are single-clock.
- **Cell-driven root net**: accepted (a generated/internal clock's
  subtree gets the same treatment); `+ USE` semantics of the driver are
  untouched.
- **Clock-as-data taps / pre-existing buffer fragments**: every
  non-driver pin is a sink — a tap's arrival is delayed by tree depth
  (semantically safe, latency-shifting); a fragment's input pin becomes
  a leaf sink and its downstream subtree survives (the fragment adds
  real depth we don't count in `tree_depth` — stated, not hidden).
- **`io_pins` untouched** (I5); buffer power pins stay `pin_nets ==
  None` like every existing cell.
- **`net_def_spellings`** maintained by `create_net` (I6) — emitted
  guide tokens stay consistent.
- **Deferred**: dummy loads (reference 3/56 — ours emits none; census
  shows it), `repair_clock_nets` (slew/fanout repair), `+ USE CLOCK` /
  `+ NONDEFAULTRULE` / `+ SOURCE TIMING` emission, TRACKS/GCELLGRID/
  VIAS/SPECIALNETS in the output DEF (pre-existing `write_def_full`
  scope), skew-driven re-balancing (ticket 4+).
