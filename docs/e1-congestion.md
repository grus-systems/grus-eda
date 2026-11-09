# E1: Global-Routing Congestion Estimate — Implementation Spec

This document provides the concrete data structures, equations, and step-by-step algorithms (`E1.1`–`E1.3`) for the implementer to implement E1.

---

## 1. Done Criteria

| Sub-Step | What | Reference Command (`ghcr.io/librelane/librelane:3.0.16`) | Done When |
|---|---|---|---|
| **E1.1** | GCell grid & edge track supply (`congestion/gcell.rs`) | `openroad`: `global_route -verbose` GCell grid log | Grid dimensions $(N_x, N_y)$ and raw track supply per layer match OpenROAD's FastRoute grid |
| **E1.2** | Multi-pin RSMT Steiner tree (`congestion/rsmt.rs`) | Unit tests + wirelength check against OpenROAD `estimate_parasitics` | RSMT wirelength $\ge \text{HPWL}$ on every net, exact on degree $\le 3$, and monotonic with net span |
| **E1.3** | Probabilistic + L/Z Pattern Routing (`congestion/route.rs`) | `openroad`: `global_route` across test variants (varying utilization / floorplan / design) | Total overflow and top-hotspot metrics rank the test variants in the exact same order |

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
    pub h_demand: Vec<f32>,

    /// Vertical boundary edges between (x, y) and (x, y + 1):
    /// indexed by `y * nx + x`, length `(ny - 1) * nx`
    pub v_cap: Vec<f32>,
    pub v_demand: Vec<f32>,
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
}
```

---

## 3. Step-by-Step Algorithms

### Step E1.1: GCell Grid Construction & Track Supply (`gcell.rs`)

1. **Grid Dimensions**:
   - In `sky130_fd_sc_hd` (`1000 DBU = 1 um`), the routing layer pitches in `.tlef` are:
     - `li1` (V/local): `0.46 um` (`460 DBU`)
     - `met1` (H): `0.34 um` (`340 DBU`)
     - `met2` (V): `0.46 um` (`460 DBU`)
     - `met3` (H): `0.68 um` (`680 DBU`)
     - `met4` (V): `0.92 um` (`920 DBU`)
     - `met5` (H): `3.40 um` (`3400 DBU`)
   - If the input DEF already contains `GCELLGRID` statements (`DesignDb::gcell_grids`), adopt its exact `(origin_x_dbu, origin_y_dbu, gcell_w_dbu, gcell_h_dbu, nx, ny)`.
   - Otherwise, follow OpenROAD/FastRoute's default tile size: `gcell_w_dbu = 15 * met2_pitch_dbu` (`15 * 460 = 6900 DBU` when `met2` pitch is `460 DBU`, or `15 * met1_pitch_dbu = 5100 DBU` if uniform 15-track `met1` grid is selected via CLI), with:
     - `origin_x_dbu = die_area.xl`, `origin_y_dbu = die_area.yl`
     - `nx = ((die_area.xh - die_area.xl + gcell_w_dbu - 1) / gcell_w_dbu).max(1) as usize`
     - `ny = ((die_area.yh - die_area.yl + gcell_h_dbu - 1) / gcell_h_dbu).max(1) as usize`
2. **Track Supply Calculation**:
   - For each routing layer $l \in \{\text{met1}, \text{met2}, \text{met3}, \text{met4}, \text{met5}\}$ (skip `li1` for inter-GCell routing, matching OpenROAD `sky130hd` `set_routing_layers -signal met1-met5`):
     - Apply layer adjustment factor $1 - \alpha_l$ (default `sky130hd` OpenROAD adjustments: `met1: 0.30`, `met2: 0.20`, `met3: 0.20`, `met4: 0.20`, `met5: 0.20`, configurable via CLI).
     - If layer $l$ is `Horizontal`:
       - For each GCell row $y \in [0, ny)$, count tracks from `DEF TRACKS Y` (or `gcell_h_dbu / pitch_dbu`) whose $y$-coordinate falls inside $[y_0 + y \cdot H, y_0 + (y + 1) \cdot H)$.
       - Add `tracks_in_row * (1.0 - alpha_l)` to `h_cap[y * (nx - 1) + x]` for all $x \in [0, nx - 1)$.
     - If layer $l$ is `Vertical`:
       - For each GCell column $x \in [0, nx)$, count tracks from `DEF TRACKS X` falling inside $[x_0 + x \cdot W, x_0 + (x + 1) \cdot W)$.
       - Add `tracks_in_col * (1.0 - alpha_l)` to `v_cap[y * nx + x]` for all $y \in [0, ny - 1)$.
3. **Obstruction & `SPECIALNETS` (PDN) Deduction**:
   - **Macro Obstructions (`OBS`)**: For every `Fixed` or `Placed` macro instance with `obs_rects` on routing layers (`met1`..`met5`), transform the obstacle rectangle into DBU coordinates according to the cell's origin and `Orient`.
   - **Power/Ground Wires (`SPECIALNETS`)**: Include every routed wire rectangle in `DesignDb::special_wire_rects` (`VPWR`/`VGND` standard-cell rails on `met1` and PDN straps on `met4`/`met5`).
   - Intersect these obstacle rectangles (bloated by half wire spacing/pitch) with overlapping GCell boundaries and subtract the blocked track count from `h_cap` / `v_cap` (clamped at `0.0`).

---

### Step E1.2: Pin Mapping & RSMT Decomposition (`rsmt.rs`)

1. **Absolute Pin Coordinates**:
   - For a primary I/O port $p$, its coordinate is `(pin_offset_x_dbu[p], pin_offset_y_dbu[p])`.
   - For an instance pin $p$ on cell $c$ with macro size $(W_c, H_c)$ and relative pin offset $(\Delta x, \Delta y)$:
     - `Orient::N`: $(x_c + \Delta x,\; y_c + \Delta y)$
     - `Orient::S`: $(x_c + W_c - \Delta x,\; y_c + H_c - \Delta y)$
     - `Orient::FN`: $(x_c + W_c - \Delta x,\; y_c + \Delta y)$
     - `Orient::FS`: $(x_c + \Delta x,\; y_c + H_c - \Delta y)$
   - Map to GCell index:
     $$gx = \text{clamp}\!\left(\frac{x - x_0}{W_{\text{gcell}}},\; 0,\; nx - 1\right), \quad gy = \text{clamp}\!\left(\frac{y - y_0}{H_{\text{gcell}}},\; 0,\; ny - 1\right)$$
2. **Deduplicate GCell Pins per Net**:
   - Sort and deduplicate `GPoint`s on net $e$. If unique count $k \le 1$, the net is purely intra-GCell and emits zero inter-GCell segments.
3. **RSMT Construction by Degree $k$**:
   - **$k = 2$**: Emit 1 segment `(pts[0], pts[1])`.
   - **$k = 3$**: Compute median Steiner point:
     $$x_s = \text{median}(x_0, x_1, x_2), \quad y_s = \text{median}(y_0, y_1, y_2)$$
     Emit up to 3 non-zero-length segments connecting each $(x_i, y_i)$ to $(x_s, y_s)$.
   - **$k \ge 4$ (Prim's Rectilinear MST + L-Flip Steiner Refinement)**:
     1. Run Prim's algorithm in $O(k^2)$ on the $k$ points using Manhattan distance $d(u, v) = |x_u - x_v| + |y_u - y_v|$ (using stack-allocated arrays for $k \le 64$, no heap allocation).
     2. Refine adjacent MST edges sharing a common vertex $u$: if two edges $(u, v)$ and $(u, w)$ overlap in horizontal or vertical projection, insert the Steiner point $(x_s, y_s) = (\text{median}(x_u, x_v, x_w), \text{median}(y_u, y_v, y_w))$ to eliminate double-counted trunk wirelength (matching FLUTE within ~1–2% on degree 4–12 nets).

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
Using the initial demand from Pass 1, define edge cost:
$$\text{cost}(d, c) = 1.0 + \frac{10.0}{1.0 + \exp\bigl(-2.0 \cdot (d - c)\bigr)}$$

Iterate over all non-degenerate 2D `SteinerSegment`s `(p1, p2)`:
1. Subtract the segment's Pass 1 `0.5` contribution from the two bounding-box L-paths (rip-up).
2. Evaluate the two **L-shape** routes:
   - $L_{\text{HV}}$: horizontal along $y = p_1.y$ from $p_1.x$ to $p_2.x$, then vertical along $x = p_2.x$ from $p_1.y$ to $p_2.y$.
   - $L_{\text{VH}}$: vertical along $x = p_1.x$ from $p_1.y$ to $p_2.y$, then horizontal along $y = p_2.y$ from $p_1.x$ to $p_2.x$.
3. If both $L_{\text{HV}}$ and $L_{\text{VH}}$ traverse at least one edge with $d + 1.0 > c$, also evaluate **Z-shape** routes (horizontal-vertical-horizontal via intermediate column $x_{\text{mid}} \in (x_{\min}, x_{\max})$, and vertical-horizontal-vertical via intermediate row $y_{\text{mid}} \in (y_{\min}, y_{\max})$) in $O(\Delta x + \Delta y)$ time using prefix sums along the bounding box.
4. Commit `+1.0` demand along the minimum-cost route.

#### Verification Script (`OpenROAD global_route`)
Run inside the LibreLane container:
```tcl
read_lef $tlef
read_lef $lef
read_def $placed_def
set_routing_layers -signal met1-met5
set_global_routing_layer_adjustment met1 0.30
set_global_routing_layer_adjustment met2-met5 0.20
global_route -verbose
```
Compare total overflow and variant ranking across test cases (`gcd` at 50%, 65%, 80%, 90% core utilization, plus `picorv32`) and record in `docs/results.md`.
