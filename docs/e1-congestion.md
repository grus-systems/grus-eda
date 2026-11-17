# E1: Global-Routing Congestion Estimate — Implementation Spec

This document provides the concrete data structures, equations, and step-by-step algorithms (`E1.1`–`E1.3`) for the implementer to implement E1.

---

## 1. Done Criteria

| Sub-Step | What | Reference Command (`ghcr.io/librelane/librelane:3.0.16`) | Done When |
|---|---|---|---|
| **E1.1** | GCell grid & edge track supply (`congestion/gcell.rs`) | `openroad`: `global_route -verbose` GCell grid log | Grid dimensions $(N_x, N_y)$ and raw track supply per layer match OpenROAD's FastRoute grid |
| **E1.2** | Multi-pin RSMT Steiner tree (`congestion/rsmt.rs`) | Unit tests + wirelength check against OpenROAD `estimate_parasitics` | RSMT wirelength $\ge \text{HPWL}$ on every net, exact on degree $\le 3$, and monotonic with net span |
| **E1.3** | Probabilistic + L/Z Pattern Routing (`congestion/route.rs`) | `openroad`: `global_route` across test variants (varying utilization / floorplan / design) | Per-edge usage-ratio map vs OpenROAD's GCell usage map: Spearman ρ ≥ 0.60 and ≥ negative-control null + 0.15 (null = max of the one-cell shift control and the 95th percentile of 20 fixed-seed permutations), hot-set (ratio ≥ 0.9) overlap ≥ 0.45. The gcd rank-order test stays as a smoke test. (F2 closure, 2026-11-14: the map replaces the old total-overflow ranking, which could not fail informatively — see `docs/history/review-2026-11-11.md` §F2 and `docs/history/step4-designs.md`.) |

---

## 2. Concrete Data Structures (`crates/grus-core/src/congestion/`)

```rust
#[derive(Clone, Debug)]
pub struct GCellGrid {
    pub origin_x_dbu: i32,
    pub origin_y_dbu: i32,
    pub gcell_w_dbu: i32,
    pub gcell_h_dbu: i32,
    pub nx: usize, // number of GCell columns
    pub ny: usize, // number of GCell rows

    /// Horizontal boundary edges between (x, y) and (x + 1, y):
    /// indexed by `y * (nx - 1) + x`, length `ny * (nx - 1)`
    pub h_cap: Vec<f32>,
    pub h_raw_cap: Vec<f32>,
    pub h_demand: Vec<f32>,

    /// Vertical boundary edges between (x, y) and (x, y + 1):
    /// indexed by `y * nx + x`, length `(ny - 1) * nx`
    pub v_cap: Vec<f32>,
    pub v_raw_cap: Vec<f32>,
    pub v_demand: Vec<f32>,

    /// Per-layer totals (indexed by `LayerId(0..tech_db.layers.len())`) for direct
    /// verification against OpenROAD `dbGCellGrid` and `[INFO GRT-0053]`:
    pub layer_raw_edge_tracks: Vec<u32>,
    pub layer_orig_edge_tracks: Vec<u32>,
    pub layer_derated_edge_tracks: Vec<u32>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct GPoint {
    pub x: u16,
    pub y: u16,
}

#[derive(Copy, Clone, Debug)]
pub struct SteinerSegment {
    pub p1: GPoint,
    pub p2: GPoint,
}

#[derive(Clone, Debug)]
pub struct CongestionReport {
    pub nx: usize,
    pub ny: usize,
    pub total_overflow: f32,
    pub max_edge_overflow: f32,
    pub top_05pct_overflow: f32,
    pub overflow_gcells: usize,
    pub gcell_usage_ratio: Vec<f32>, // length nx * ny, max(demand/cap) over adjacent edges
    pub h_usage_ratio: Vec<f32>,     // length ny * (nx - 1)
    pub v_usage_ratio: Vec<f32>,     // length (ny - 1) * nx
}
```

---

## 3. Step-by-Step Algorithms

### Step E1.1: GCell Grid Construction & Track Supply (`gcell.rs`)

1. **Grid Dimensions (matching OpenDB `dbBlock::getGCellTileSize` & `FastRoute` grid)**:
   - In `sky130_fd_sc_hd` (`1000 DBU = 1 um`), the routing layer pitches in `.tlef` are:
     - `li1` (V/local): `0.46 um` (`460 DBU`)
     - `met1` (H): `0.34 um` (`340 DBU`)
     - `met2` (V): `0.46 um` (`460 DBU`)
     - `met3` (H): `0.68 um` (`680 DBU`)
     - `met4` (V): `0.92 um` (`920 DBU`)
     - `met5` (H): `3.40 um` (`3400 DBU`)
   - If the input DEF already contains `GCELLGRID` statements (`DesignDb::gcell_grids`), adopt its `(origin_x_dbu, origin_y_dbu, gcell_w_dbu, gcell_h_dbu, nx, ny)`.
   - Otherwise, follow OpenROAD's `dbBlock::getGCellTileSize()` default: `gcell_w_dbu = gcell_h_dbu = 15 * pitch_dbu(met2)` (`15 * 460 = 6900 DBU`, i.e., $15\times$ the pitch of the 3rd routing layer `met2`, overridable via CLI `--gcell-tracks`), with integer **floor** division:
     - `origin_x_dbu = die_area.xl`, `origin_y_dbu = die_area.yl`
     - `nx = ((die_area.xh - die_area.xl) / gcell_w_dbu).max(1) as usize`
     - `ny = ((die_area.yh - die_area.yl) / gcell_h_dbu).max(1) as usize`
     - Column $x \in [0, nx)$ spans $[X_x, X_{x+1})$, where $X_x = x_0 + (x as i32) \cdot W_{\text{gcell}}$ for $x < nx$ and $X_{nx} = \text{die\_area.xh}$ (the last column extends to the upper die boundary, matching `dbGCellGrid`).
     - Row $y \in [0, ny)$ spans $[Y_y, Y_{y+1})$, where $Y_y = y_0 + (y as i32) \cdot H_{\text{gcell}}$ for $y < ny$ and $Y_{ny} = \text{die\_area.yh}$ (the last row extends to the upper die boundary).
     - Any DBU coordinate $(x, y)$ maps to GCell indices:
       $$gx = \text{clamp}\!\left(\frac{x - x_0}{W_{\text{gcell}}},\; 0,\; nx - 1\right), \quad gy = \text{clamp}\!\left(\frac{y - y_0}{H_{\text{gcell}}},\; 0,\; ny - 1\right)$$

2. **Raw Track Supply Calculation (matching `dbGCellGrid::getCapacity`)**:
   - Skip `li1` (`LayerId(0)`) for inter-GCell routing, matching OpenROAD `sky130hd` `set_routing_layers -signal met1-met5`.
   - For each active routing layer $l \in \{\text{met1}, \text{met2}, \text{met3}, \text{met4}, \text{met5}\}$:
     - If layer $l$ is `Horizontal`:
       - For each GCell row $y \in [0, ny)$, count tracks from `DesignDb::tracks` (`TrackGrid` with `layer == l` and `is_x_grid == false`) whose coordinate $t_y = \text{start\_dbu} + k \cdot \text{step\_dbu}$ ($0 \le k < \text{num\_tracks}$) satisfies $Y_y \le t_y < Y_{y+1}$ (or $Y_y \le t_y \le Y_{ny}$ when $y == ny - 1$).
       - If `DesignDb::tracks` has no matching `TrackGrid` for layer $l$, fall back to `(Y_{y+1} - Y_y) / pitch_dbu`.
       - Across the $nx - 1$ horizontal edges in row $y$, the raw edge track count is `raw_tracks_y[y]`, contributing `(nx - 1) * raw_tracks_y[y]` to `layer_raw_edge_tracks[l]`.
     - If layer $l$ is `Vertical`:
       - For each GCell column $x \in [0, nx)$, count tracks from `DesignDb::tracks` (`TrackGrid` with `layer == l` and `is_x_grid == true`) whose coordinate $t_x$ satisfies $X_x \le t_x < X_{x+1}$ (or $X_x \le t_x \le X_{nx}$ when $x == nx - 1$).
       - Across the $ny - 1$ vertical edges in column $x$, the raw edge track count is `raw_tracks_x[x]`, contributing `(ny - 1) * raw_tracks_x[x]` to `layer_raw_edge_tracks[l]`.

3. **Obstruction (`OBS`) Merging, `SPECIALNETS` Deduction & Layer Derating**:
   - **Connected-Component Merging of Macro `OBS` and Macro Pin Geometry (`readers/lef.rs`)**:
     In `sky130_fd_sc_hd.lef`, a single internal `met1` jumper in cells like `dfxtp_2`, `xnor2_2`, and `xor2_2` is decomposed into 5–7 touching `RECT` statements (a main bar plus small via-enclosure pads). At the end of `parse_macro` in `readers/lef.rs`, merge any two `obs_rects` on the same `LayerId` that touch or overlap (`a.xl <= b.xh && b.xl <= a.xh && a.yl <= b.yh && b.yl <= a.yh`) into their bounding box union until fixed point, and record signal/PG LEF `PIN` routing-layer geometry separately in `MacroDef::pin_obs_rects` so pin shapes also block GCell tracks.
   - **Instance `OBS` + Pin 8-Orientation Transform**:
     For every `Fixed` or `Placed` cell instance $c$ at `(cx, cy)` with macro size `(w, h)` and orientation `orient`, transform each macro `(layer, rect)` in `obs_rects` and `pin_obs_rects` into DBU coordinates `(xl, yl, xh, yh)`:
     - `N`: `(cx + r.xl, cy + r.yl, cx + r.xh, cy + r.yh)`
     - `S`: `(cx + w - r.xh, cy + h - r.yh, cx + w - r.xl, cy + h - r.yl)`
     - `W`: `(cx + h - r.yh, cy + r.xl, cx + h - r.yl, cy + r.xh)`
     - `E`: `(cx + r.yl, cy + w - r.xh, cx + r.yh, cy + w - r.xl)`
     - `FN`: `(cx + w - r.xh, cy + r.yl, cx + w - r.xl, cy + r.yh)`
     - `FS`: `(cx + r.xl, cy + h - r.yh, cx + r.xh, cy + h - r.yl)`
     - `FW`: `(cx + r.yl, cy + r.xl, cx + r.yh, cy + r.xh)`
     - `FE`: `(cx + h - r.yh, cy + w - r.xh, cx + h - r.yl, cy + w - r.xl)`
   - **Power/Ground Wires (`SPECIALNETS`)**:
     Include every routed non-via wire rectangle `(layer, rect)` in `DesignDb::special_wire_rects` (`VPWR`/`VGND` `FOLLOWPIN` rails on `met1`, `STRIPE` straps on `met4`/`met5`, and `DRCFILL` patches on `met2`/`met3`).
   - **Per-Edge 1D Cross-Track Interval Union & Track Deduction**:
     For each routing layer $l$ with pitch $P_l = \text{pitch\_dbu}(l)$ and layer adjustment $\alpha_l \in [0.0, 1.0]$ (default `sky130hd` adjustments: `met1: 0.30`, `met2..met5: 0.20`, overridable via uniform or per-layer adjustment):
     - If layer $l$ is `Horizontal`:
       - Each obstacle `(xl, yl, xh, yh)` on layer $l$ maps to $gx_1 = \text{gcell\_x}(xl)$, $gx_2 = \text{gcell\_x}((xh - 1).\max(xl))$, $gy_1 = \text{gcell\_y}(yl)$, $gy_2 = \text{gcell\_y}((yh - 1).\max(yl))$ (treating exclusive upper bounds cleanly at GCell boundaries).
       - For each row $y \in [gy_1, gy_2]$ and horizontal edge column $x \in [gx_1, \min(\max(gx_1, gx_2 - 1), nx - 2)]$, clip the obstacle's Y-span to row $y$: $[s, e] = [\max(yl, Y_y), \min(yh, Y_{y+1})]$. If $e > s$, record interval $(s, e)$ on edge $(x, y)$.
       - On each horizontal edge $(x, y)$, sort and merge overlapping/touching 1D intervals $(s_i, e_i)$, and compute blocked tracks:
         $$\text{blocked}(l, x, y) = \sum_{i} \frac{e_i - s_i + P_l - 1}{P_l}$$
       - Compute original (unadjusted) and derated edge track capacity (matching OpenROAD's `FastRoute` integer truncation and minimum-1 clamp when $\text{orig} > 0$ and $\alpha_l < 1.0$, adding `1e-6` before `.floor()` to avoid IEEE 754 `f32 -> f64` representation loss such as `15.0 * (1.0 - 0.20f32 as f64) = 11.999999955`):
         $$\text{orig}(l, x, y) = \max\bigl(0,\; \text{raw\_tracks\_y}[y] - \text{blocked}(l, x, y)\bigr)$$
         $$\text{derated}(l, x, y) = \begin{cases} 0 & \text{if } \text{orig}(l, x, y) == 0 \text{ or } \alpha_l \ge 1.0, \\ \max\bigl(1,\; \lfloor \text{orig}(l, x, y) \cdot (1.0 - \alpha_l) + 10^{-6} \rfloor\bigr) & \text{otherwise.} \end{cases}$$
       - Accumulate `orig` into `layer_orig_edge_tracks[l]`, `derated` into `layer_derated_edge_tracks[l]`, and add `derated as f32` to `h_cap[y * (nx - 1) + x]`.
     - If layer $l$ is `Vertical`:
       - Symmetric: for each column $x \in [gx_1, gx_2]$ and vertical edge row $y \in [gy_1, \min(\max(gy_1, gy_2 - 1), ny - 2)]$, clip the obstacle's X-span to $[X_x, X_{x+1})$, sort and merge 1D intervals on edge $(x, y)$, sum $\lceil (e_i - s_i) / P_l \rceil$, compute `orig` and `derated`, and add `derated as f32` to `v_cap[y * nx + x]`.
     - Also record raw per-edge track totals in `h_raw_cap` and `v_raw_cap` so per-edge usage ratios match `dbGCellGrid`.
   - **Note on `met1` Internal Jumpers vs. Boost.Polygon Slicing**:
     On `gcd.placed.def`, this formula matches OpenROAD's `dbGCellGrid` raw edge capacities on all 5 layers (`3624, 2600, 1812, 1300, 360`) and matches OpenROAD's `[INFO GRT-0053]` `Original` and `Derated` resources **to the exact integer** on `met2` (`2504 / 1996`), `met3` (`1781 / 1399`), `met4` (`1252 / 926`), and `met5` (`324 / 180`). On `met1`, including `pin_obs_rects` and exclusive upper bounds gives `2748 / 1843` vs. OpenROAD's `2705 / 1815` (total derated capacity across `met1..met5` is `6344` vs. OpenROAD's `6316`, within `0.44%`).

---

### Step E1.2: Pin Mapping & RSMT Decomposition (`rsmt.rs`)

1. **Absolute Pin Coordinates**:
   - Call `DesignDb::pin_pos_dbu(pin, tech_db)` (which handles primary I/O ports and all 8 DEF cell orientations matching OpenROAD's `dbITerm::getAvgXY`).
   - Map $(x, y)$ to GCell index:
     $$gx = \text{clamp}\!\left(\frac{x - x_0}{W_{\text{gcell}}},\; 0,\; nx - 1\right), \quad gy = \text{clamp}\!\left(\frac{y - y_0}{H_{\text{gcell}}},\; 0,\; ny - 1\right)$$
2. **Deduplicate GCell Pins per Net**:
   - Sort and deduplicate `GPoint`s on net $e$. If unique count $k \le 1$, the net is purely intra-GCell and emits zero inter-GCell segments.
3. **RSMT Construction by Degree $k$**:
   - **$k = 2$**: Emit 1 segment `(pts[0], pts[1])`.
   - **$k = 3$**: Compute median Steiner point:
     $$x_s = \text{median}(x_0, x_1, x_2), \quad y_s = \text{median}(y_0, y_1, y_2)$$
     Emit up to 3 non-zero-length segments connecting each $(x_i, y_i)$ to $(x_s, y_s)$.
   - **$k \ge 4$ (Prim's Rectilinear MST + L-Flip Steiner Refinement)**:
     1. Run Prim's algorithm in $O(k^2)$ on the $k$ points using Manhattan distance $d(u, v) = |x_u - x_v| + |y_u - y_v|$ (using stack-allocated arrays for $k \le 64$, no heap allocation), breaking equal-distance ties toward smaller $|y_v - y_u|$ (preferring same-row horizontal standard-cell connections) and then smaller vertex index.
     2. Refine adjacent MST edges sharing a common vertex $u$: if two edges $(u, v)$ and $(u, w)$ overlap in horizontal or vertical projection, insert the Steiner point $(x_s, y_s) = (\text{median}(x_u, x_v, x_w), \text{median}(y_u, y_v, y_w))$ to eliminate double-counted trunk wirelength (788 GCell units on `gcd` vs OpenROAD's 812 routed edge demand).

---

### Step E1.3: Pass 1 Probabilistic & Pass 2 Pattern Routing (`route.rs`)

#### Pass 1: Probabilistic Demand (RISA / Uniform L-Split)
Initialize `h_demand` and `v_demand` to `0.0`. For each `SteinerSegment { p1, p2 }`:
- Let $x_{\min} = \min(p_1.x, p_2.x)$, $x_{\max} = \max(p_1.x, p_2.x)$, $y_{\min} = \min(p_1.y, p_2.y)$, $y_{\max} = \max(p_1.y, p_2.y)$.
- **Flat horizontal ($y_{\min} == y_{\max}$)**:
  - For $x \in [x_{\min}, x_{\max})$: `h_demand[y_min * (nx - 1) + x] += 1.0`.
- **Flat vertical ($x_{\min} == x_{\max}$)**:
  - For $y \in [y_{\min}, y_{\max})$: `v_demand[y * nx + x_min] += 1.0`.
- **2D bend ($x_{\min} < x_{\max}$ and $y_{\min} < y_{\max}$)**:
  - Add `0.5` along both L-shape bounding box paths:
    - Horizontal legs at $y = y_{\min}$ and $y = y_{\max}$ for $x \in [x_{\min}, x_{\max})$ get `+0.5`.
    - Vertical legs at $x = x_{\min}$ and $x = x_{\max}$ for $y \in [y_{\min}, y_{\max})$ get `+0.5`.

#### Pass 2: Congestion-Aware L/Z Pattern Routing
For Pass 2 pattern selection, score adding `+1.0` demand over current demand $d$ and capacity $c$ with **pure overflow cost**:
$$\text{overflow\_cost}(d, c) = \max(0.0,\; d + 1.0 - c)$$
Using pure overflow cost rather than a sub-capacity sigmoid avoids over-smoothing near-capacity peaks (usage ratios in $[0.90, 1.00]$) while routing around actual channel overflow.

Sort 2D `SteinerSegment`s short-first by `(dx + dy, seg_idx)` for deterministic rip-up-and-reroute across up to 3 iterations:
1. **Rip-up**:
   - On iteration `0`, subtract every 2D segment's Pass 1 `0.5` contribution from the two bounding-box L-paths so all 2D segments commit to concrete `0/1` paths instead of leaving `0.5/0.5` fractional averaging on non-overflowing edges.
   - On iterations `1..2`, rip up (`-1.0`) and re-route only segments whose committed path traverses at least one overflowing edge (`d > c`), stopping early if none overflow.
2. Evaluate the two **L-shape** routes under `overflow_cost(d, c)`:
   - $L_{\text{HV}}$: horizontal along $y = p_1.y$ from $p_1.x$ to $p_2.x$, then vertical along $x = p_2.x$ from $p_1.y$ to $p_2.y$.
   - $L_{\text{VH}}$: vertical along $x = p_1.x$ from $p_1.y$ to $p_2.y$, then horizontal along $y = p_2.y$ from $p_1.x$ to $p_2.x$.
3. If `best_cost > 0.0` (both L-routes overflow), also evaluate **Z-shape** routes (horizontal-vertical-horizontal via intermediate column $x_{\text{mid}} \in (x_{\min}, x_{\max})$, and vertical-horizontal-vertical via intermediate row $y_{\text{mid}} \in (y_{\min}, y_{\max})$) in $O(\Delta x + \Delta y)$ time using prefix sums along the bounding box, adding a `+0.01` bend tie-breaker so a 2-bend Z-route only replaces an L-route when it strictly reduces overflow.
4. Commit `+1.0` demand along the minimum-cost route and record the segment's committed edge list. (Flat horizontal or vertical segments from Pass 1 already have their straight-line edge list recorded.)

Pass 3 (bounded-box Dijkstra) retains the smooth congestion-avoidance edge cost:
$$\text{cost}(d, c) = 1.0 + \frac{10.0}{1.0 + \exp\bigl(-2.0 \cdot (d + 1.0 - c)\bigr)} + 50.0 \cdot \max(0.0,\; d + 1.0 - c)$$

#### Pass 3: Bounded-Box Maze Rip-Up and Reroute (FastRoute Expanding Box)
Because RSMT Steiner point insertion decomposes many multi-pin nets into flat horizontal or vertical segments (`x1 == x2` or `y1 == y2`) and L/Z pattern routing is strictly confined to a segment's bounding box $[x_{\min}, x_{\max}] \times [y_{\min}, y_{\max}]$, congested segments cannot utilize free tracks in adjacent GCell rows or columns without box expansion.

Run 3 rip-up-and-reroute iterations with expanding bounding-box margins $M \in [2, 3, 4]$:
- In each iteration, visit every segment $s_i = (p_1, p_2)$ whose currently committed path contains at least one overflowing edge (`demand > cap`):
  1. **Rip-up**: subtract `1.0` demand from each edge in $s_i$'s current path.
  2. **Bounded-Box Dijkstra**: define the search window:
     $$X_{\text{lo}} = \max(0, x_{\min} - M), \quad X_{\text{hi}} = \min(nx - 1, x_{\max} + M)$$
     $$Y_{\text{lo}} = \max(0, y_{\min} - M), \quad Y_{\text{hi}} = \min(ny - 1, y_{\max} + M)$$
     Run Dijkstra's shortest-path algorithm from `p1` to `p2` on the grid subgraph $[X_{\text{lo}}, X_{\text{hi}}] \times [Y_{\text{lo}}, Y_{\text{hi}}]$ using edge weight $\text{cost}(d, c)$.
  3. **Commit**: add `1.0` demand along the shortest path and replace $s_i$'s committed path.

#### Congestion Report Aggregation
After routing all nets:
- For each horizontal edge $e_h \in [0, ny \cdot (nx - 1))$ and vertical edge $e_v \in [0, (ny - 1) \cdot nx)$:
  $$\text{ovf}(d, c) = \max(0.0,\; d - c), \quad \text{usage\_ratio}(d, c, \text{raw}) = \begin{cases} (\max(0,\; \text{raw} - c) + d) / \text{raw} & \text{if } \text{raw} > 0.0, \\ d / c & \text{else if } c > 0.0, \\ \infty & \text{else if } d > 0.0, \\ 0.0 & \text{otherwise.} \end{cases}$$
- `h_usage_ratio`, `v_usage_ratio`: per-edge `usage_ratio(d, c, raw)` matching OpenROAD's `dbGCellGrid` blockage+signal usage fraction over raw track supply (`*.gcell_usage.txt`).
- `total_overflow`: sum of $\text{ovf}(d, c)$ across all horizontal and vertical edges (accumulated in `f64`, cast to `f32`).
- `max_edge_overflow`: maximum $\text{ovf}(d, c)$ across all horizontal and vertical edges.
- `top_05pct_overflow`: mean of the largest $k = \max(1, \lceil 0.005 \cdot N_{\text{edges}} \rceil)$ edge overflows sorted descending.
- `gcell_usage_ratio[y * nx + x]`: maximum of $\text{ratio}(d, c)$ over the up to 4 boundary edges incident to GCell $(x, y)$ (left $(x-1, y)$, right $(x, y)$, bottom $(x, y-1)$, top $(x, y)$).
- `overflow_gcells`: number of GCells $(x, y)$ with `gcell_usage_ratio[y * nx + x] > 1.0`.

#### Verification Script (`OpenROAD global_route`)
In `tests/designs/gcd/generate_ref.sh`, run inside the LibreLane container (`ghcr.io/librelane/librelane:3.0.16`) and emit all reference metrics directly into `tests/designs/gcd/gcd.reference.json` via live `dbGCellGrid` queries (`sum(max(0, usage - capacity))` across `met1..met5`):
1. **Baseline `gcd.placed.def` (`met1: 0.30, met2..met5: 0.20`)**:
   - Grid dimensions `nx = 13, ny = 14`, `tile_size_dbu = 6900`.
   - Per-layer raw edge track capacities (`3624, 2600, 1812, 1300, 360`), original capacities (`2705, 2504, 1781, 1252, 324`), and derated capacities (`1815, 1996, 1399, 926, 180`).
   - Total routed inter-GCell edge demand (`grt_total_edge_demand = 812`).
2. **Variant Set A (Layer-Adjustment Sweep on `gcd.placed.def`)**:
   - Uniform layer adjustment $\alpha \in \{0.50, 0.70, 0.80, 0.85, 0.90\}$ (`set_global_routing_layer_adjustment met1-met5 $alpha`, `global_route -verbose -allow_congestion`).
   - Emit `grt_layer_adj_sweep_alpha: [0.50, 0.70, 0.80, 0.85, 0.90]` and `grt_layer_adj_sweep_overflow` (`[0, 0, 31, 69, 204]`) into `gcd.reference.json`.
3. **Variant Set B (Core-Utilization Placement Sweep on `gcd.synth.v`)**:
   - Four placed DEF fixtures (`gcd.util25.def`, `gcd.util45.def`, `gcd.util65.def`, `gcd.util78.def`) generated at `25%, 45%, 65%, 78%` core utilization (`initialize_floorplan -utilization $util -aspect_ratio 1.0 -core_space 5.52 -site unithd`, `make_tracks`, `place_pins -hor_layers met3 -ver_layers met2`, `global_placement -density $dens` with `$dens` in `{0.33, 0.53, 0.73, 0.86}`, `detailed_placement`) and routed with `set_global_routing_layer_adjustment met1-met5 0.75` (and `0.80`).
   - Emit `grt_util_sweep_utils: [25, 45, 65, 78]`, `grt_util_sweep_overflow_075` (`[6, 77, 116, 168]`), and `grt_util_sweep_overflow_080` (`[154, 314, 312, 350]`) into `gcd.reference.json`.
   - Compare total overflow and variant ranking against OpenROAD's `global_route` and record in `docs/results.md`.
