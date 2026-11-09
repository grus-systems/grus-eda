# E3: Static Timing Analysis (STA) — Implementation Spec

This document breaks E3 into three progressive implementation steps (`E3.1`–`E3.3`) with exact data structures, NLDM extrapolation code, and Elmore RC equations for the implementer.

---

## 1. Done Criteria & Scope

| Sub-Step | What | Reference Command (`ghcr.io/librelane/librelane:3.0.16`) | Done When |
|---|---|---|---|
| **E3.1** | Zero-Wire STA: levelized DAG, dual Rise/Fall propagation, NLDM extrapolation, Setup/Hold slack | `sta`: `read_liberty`, `read_verilog`, `create_clock`, `report_checks -path_delay min_max` | Worst slack (Setup & Hold) and every endpoint's arrival/slew agree within **1% or 5 ps** on `gcd` and `picorv32` |
| **E3.2** | Placement-Based Virtual RC STA (`parasitics.rs`) | `openroad`: `estimate_parasitics -placement`, `report_tns`, `report_wns` | Agrees with OpenROAD placement-based parasitics when given identical per-micron $r_0, c_0$ |
| **E3.3** | SPEF-Based STA (`readers/spef.rs` + Elmore tree) | `sta`: `read_spef`, `report_checks -path_delay min_max` | Worst slack and every endpoint's arrival agree within **1% or 5 ps** on extracted `.spef` |

> **Scope Rule**: E3 targets edge-triggered flip-flop designs (`dfxtp`, `dfrtp`, `dfxbp`, etc.). Transparent level-sensitive latches (`dlxtp`, time borrowing across cycle boundaries) are out of initial scope; if a design instantiates a latch, emit a clear diagnostic warning.

---

## 2. Concrete Data Structures (`crates/grus-core/src/sta/`)

```rust
use crate::interner::{CellId, LibCellId, NetId, PinId};
use crate::lib_db::{NldmTable, TimingSense, TimingType};

pub const RISE: usize = 0;
pub const FALL: usize = 1;

#[derive(Clone, Debug)]
pub struct TimingArcEdge {
    pub src_pin: PinId,
    pub dst_pin: PinId,
    pub cell_id: CellId,
    pub sense: TimingSense,
    pub timing_type: TimingType,
    pub lib_arc_idx: u32, // index into LibDb::arcs
}

#[derive(Clone, Debug)]
pub struct TimingCheck {
    pub data_pin: PinId,
    pub clk_pin: PinId,
    pub cell_id: CellId,
    pub is_setup: bool,
    pub is_rising_clk: bool,
    pub lib_arc_idx: u32,
}

#[derive(Clone, Debug)]
pub struct TimingGraph {
    /// Topological levels of nets/cells: level `l` contains nets `level_net_ids[level_offsets[l]..level_offsets[l+1]]`
    pub level_offsets: Vec<u32>,
    pub level_net_ids: Vec<NetId>,

    /// Outgoing cell timing arcs from each input PinId (CSR indexed by PinId)
    pub pin_arc_offsets: Vec<u32>,
    pub pin_arcs: Vec<TimingArcEdge>,

    /// Sequential setup/hold checks at endpoints
    pub checks: Vec<TimingCheck>,
    pub primary_outputs: Vec<PinId>,
}

#[derive(Clone, Debug)]
pub struct PinTimingState {
    pub arrival_max: [f32; 2],  // [RISE, FALL] in ns (initialized to f32::NEG_INFINITY)
    pub arrival_min: [f32; 2],  // [RISE, FALL] in ns (initialized to f32::INFINITY)
    pub slew_max: [f32; 2],     // [RISE, FALL] in ns
    pub slew_min: [f32; 2],     // [RISE, FALL] in ns
    pub required_max: [f32; 2], // [RISE, FALL] in ns (initialized to f32::INFINITY)
    pub required_min: [f32; 2], // [RISE, FALL] in ns (initialized to f32::NEG_INFINITY)
}
```

---

## 3. Exact Algorithms & Formulas

### Step E3.1: NLDM Lookup with Linear Extrapolation (`nldm.rs`)

OpenSTA **never clamps** out-of-bounds slews or capacitances; it linearly extrapolates using the first interval $[0, 1]$ or last interval $[M-2, M-1]$. Use this exact implementation:

```rust
use crate::lib_db::{NldmTable, TableAxisVar};

#[inline]
fn find_interval_unclamped(axis: &[f32], val: f32) -> (usize, f32) {
    let n = axis.len();
    if n <= 1 {
        return (0, 0.0);
    }
    // Linear extrapolation below axis[0] uses interval [0, 1]
    if val <= axis[0] {
        let denom = axis[1] - axis[0];
        let u = if denom.abs() > 1e-12 { (val - axis[0]) / denom } else { 0.0 };
        return (0, u);
    }
    // Linear extrapolation above axis[n - 1] uses interval [n - 2, n - 1]
    if val >= axis[n - 1] {
        let i = n - 2;
        let denom = axis[i + 1] - axis[i];
        let u = if denom.abs() > 1e-12 { (val - axis[i]) / denom } else { 0.0 };
        return (i, u);
    }
    // Binary search for interior interval axis[i] <= val <= axis[i + 1]
    let mut lo = 0usize;
    let mut hi = n - 1;
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if axis[mid] <= val {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let denom = axis[lo + 1] - axis[lo];
    let u = if denom.abs() > 1e-12 { (val - axis[lo]) / denom } else { 0.0 };
    (lo, u)
}

pub fn eval_nldm_delay_or_slew(
    table: &NldmTable,
    in_slew_ns: f32,
    out_cap_pf: f32,
) -> f32 {
    let (x, y) = match (table.var1, table.var2) {
        (TableAxisVar::InputNetTransition, TableAxisVar::TotalOutputNetCapacitance) => {
            (in_slew_ns, out_cap_pf)
        }
        (TableAxisVar::TotalOutputNetCapacitance, TableAxisVar::InputNetTransition) => {
            (out_cap_pf, in_slew_ns)
        }
        (TableAxisVar::InputNetTransition, _) => (in_slew_ns, 0.0),
        (TableAxisVar::TotalOutputNetCapacitance, _) => (out_cap_pf, 0.0),
        _ => (in_slew_ns, out_cap_pf),
    };
    eval_2d(table, x, y)
}

pub fn eval_nldm_constraint(
    table: &NldmTable,
    constrained_slew_ns: f32,
    related_slew_ns: f32,
) -> f32 {
    let (x, y) = match (table.var1, table.var2) {
        (TableAxisVar::ConstrainedPinTransition, TableAxisVar::RelatedPinTransition) => {
            (constrained_slew_ns, related_slew_ns)
        }
        (TableAxisVar::RelatedPinTransition, TableAxisVar::ConstrainedPinTransition) => {
            (related_slew_ns, constrained_slew_ns)
        }
        _ => (constrained_slew_ns, related_slew_ns),
    };
    eval_2d(table, x, y)
}

#[inline]
pub fn eval_2d(table: &NldmTable, x: f32, y: f32) -> f32 {
    let n2 = table.index_2.len();
    if table.index_1.len() == 1 && n2 == 1 {
        return table.values[0];
    }
    let (i, u) = find_interval_unclamped(&table.index_1, x);
    if n2 <= 1 {
        let v0 = table.values[i];
        let v1 = table.values[(i + 1).min(table.index_1.len() - 1)];
        return (1.0 - u) * v0 + u * v1;
    }
    let (j, v) = find_interval_unclamped(&table.index_2, y);
    let t00 = table.values[i * n2 + j];
    let t10 = table.values[(i + 1) * n2 + j];
    let t01 = table.values[i * n2 + (j + 1)];
    let t11 = table.values[(i + 1) * n2 + (j + 1)];
    (1.0 - u) * (1.0 - v) * t00
        + u * (1.0 - v) * t10
        + (1.0 - u) * v * t01
        + u * v * t11
}
```

#### Dual Polarity Propagation Rules (`propagate.rs`)
1. **Startpoint Initialization**:
   - Primary inputs (`PinDir::Input` on `pin_cells[p] == None`): set `arrival_max = [input_delay, input_delay]`, `arrival_min = [input_delay, input_delay]`, `slew_max = [in_slew_rise, in_slew_fall]`, `slew_min = [in_slew_rise, in_slew_fall]`.
   - Clock network pins: if running ideal clock (`propagate_clock = false`), set clock pin `CLK` of every flip-flop to `arrival = [0.0, period / 2.0]`, `slew = [clk_slew, clk_slew]`. Immediately evaluate every `RisingEdge` (`CLK -> Q` / `Q_N`) arc using `in_slew = slew[RISE]` on `CLK` and `out_cap = net_cap[RISE/FALL]` on `Q` to seed `Q`'s `arrival_max[RISE/FALL]` and `arrival_min[RISE/FALL]`.
2. **Levelized Forward Pass ($L_0 \to L_K$)**:
   - For each net $e$ in level $L_l$:
     - Transfer driver pin $d$'s arrival and slew across net $e$ to each sink pin $s \in \text{sinks}(e)$:
       $$\text{arr}(s, \text{pol}) = \text{arr}(d, \text{pol}) + \Delta t_{\text{wire}}(d \to s, \text{pol})$$
       $$\text{slew}(s, \text{pol}) = \sqrt{\text{slew}(d, \text{pol})^2 + \Delta \text{slew}_{\text{wire}}(d \to s, \text{pol})^2}$$
     - From each combinational input pin $s$, iterate over outgoing `TimingArcEdge`s $s \to y$:
       - Determine candidate `(src_pol, dst_pol)` pairs from `arc.sense`:
         - `PositiveUnate`: `[(RISE, RISE), (FALL, FALL)]`
         - `NegativeUnate`: `[(RISE, FALL), (FALL, RISE)]`
         - `NonUnate`: `[(RISE, RISE), (FALL, RISE), (RISE, FALL), (FALL, FALL)]`
       - For each `(src_pol, dst_pol)`:
         - Lookup `delay` and `out_slew` from `cell_rise`/`rise_transition` (if `dst_pol == RISE`) or `cell_fall`/`fall_transition` (if `dst_pol == FALL`) using `in_slew = slew[s][src_pol]` and `out_cap = net_cap[net(y)][dst_pol]`.
         - Update `arrival_max[y][dst_pol]` (and carry its `out_slew` if the new arrival exceeds the existing `arrival_max`, matching OpenSTA's critical-path slew propagation) and `arrival_min[y][dst_pol]`.
3. **Setup & Hold Endpoint Checks and Backward Pass**:
   - At each `TimingCheck` on flip-flop data pin $D$ with rising clock pin $CLK$:
     - **Setup (Max Path)** for data polarity $\text{pol} \in \{\text{RISE}, \text{FALL}\}$:
       $$t_{\text{setup}}(\text{pol}) = \text{eval\_nldm\_constraint}\bigl(\text{constraint\_table}(\text{pol}),\; \text{slew}_{\max}(D, \text{pol}),\; \text{slew}_{\max}(CLK, \text{RISE})\bigr)$$
       $$\text{req}_{\max}(D, \text{pol}) = T_{\text{period}} + \text{arr}_{\min}(CLK, \text{RISE}) - t_{\text{setup}}(\text{pol}) - t_{\text{uncertainty}}$$
       $$\text{slack}_{\text{setup}}(D, \text{pol}) = \text{req}_{\max}(D, \text{pol}) - \text{arr}_{\max}(D, \text{pol})$$
     - **Hold (Min Path)** for data polarity $\text{pol} \in \{\text{RISE}, \text{FALL}\}$:
       $$t_{\text{hold}}(\text{pol}) = \text{eval\_nldm\_constraint}\bigl(\text{constraint\_table}(\text{pol}),\; \text{slew}_{\min}(D, \text{pol}),\; \text{slew}_{\max}(CLK, \text{RISE})\bigr)$$
       $$\text{req}_{\min}(D, \text{pol}) = \text{arr}_{\max}(CLK, \text{RISE}) + t_{\text{hold}}(\text{pol}) + t_{\text{uncertainty}}$$
       $$\text{slack}_{\text{hold}}(D, \text{pol}) = \text{arr}_{\min}(D, \text{pol}) - \text{req}_{\min}(D, \text{pol})$$
   - Propagate `required_max` (taking `min` across fanout) and `required_min` (taking `max` across fanout) backward from level $L_K$ down to $L_0$.

---

### Step E3.2: Placement-Based Virtual RC (`parasitics.rs`)

When `--parasitics placement` is selected:
1. Build the RSMT Steiner tree for each net $e$ using the exact pin coordinates in micron units (reusing `rsmt.rs` from E1).
2. Let $r_0$ ($\text{k}\Omega/\mu\text{m}$ or $\Omega/\mu\text{m}$) and $c_0$ ($\text{pF}/\mu\text{m}$) be the horizontal/vertical signal layer unit resistance and capacitance (from LEF or OpenROAD's `set_wire_rc` values).
3. Root the Steiner tree at the driver pin $d$ via BFS/DFS:
   - Each tree edge $u \to v$ of length $L_{uv}$ microns has resistance $R_{uv} = r_0 L_{uv}$ and capacitance $C_{uv} = c_0 L_{uv}$ (split $\frac{1}{2} C_{uv}$ at node $u$ and $\frac{1}{2} C_{uv}$ at node $v$).
   - **Bottom-up pass**: accumulate total downstream capacitance $C_{\text{down}}(u)$ for every tree node $u$ (including sink pin capacitances $C_{\text{pin}}(s, \text{pol})$).
   - Total driver load: $C_{\text{load}}(e, \text{pol}) = C_{\text{down}}(d, \text{pol})$.
   - **Top-down pass**: compute Elmore delay to each node $v$ with parent $u$:
     $$\tau_{\text{Elmore}}(v) = \tau_{\text{Elmore}}(u) + R_{uv} \, C_{\text{down}}(v)$$

---

### Step E3.3: SPEF Reader & RC Tree Elmore Propagation (`readers/spef.rs`)

1. **SPEF Units & Header**:
   - Parse `*T_UNIT` (convert to `ns`), `*C_UNIT` (convert to `pF`), `*R_UNIT` (convert to $\text{k}\Omega$ so $R \cdot C = \text{k}\Omega \cdot \text{pF} = \text{ns}$ directly!), and `*NAME_MAP` (`*123 -> net_or_inst_name`).
2. **Per-Net `*D_NET *net_id total_cap`**:
   - Parse `*CONN` (identifies `*I inst:pin` and `*P port` nodes, with `*D` marking the driver).
   - Parse `*CAP` (ground capacitors `id node cap_val`; ignore coupling capacitors or lump them to ground).
   - Parse `*RES` (resistors `id node1 node2 res_val`).
3. **Two-Pass Elmore Delay & Slew Degradation**:
   - Root the RC graph at the driver node $d$ (spanning tree BFS if parallel resistor loops exist):
   - Add sink pin Liberty capacitance $C_{\text{pin}}(s, \text{pol})$ to each sink node $s$ (or verify whether the SPEF already lumps pin cap vs. wire-only cap; OpenROAD SPEFs contain **wire-only** capacitance in `*CAP`, so always add `LibPin::cap_rise` / `cap_fall` at sink pins!).
   - Run the same bottom-up $C_{\text{down}}$ and top-down $\tau_{\text{Elmore}}$ pass as in Step E3.2.
   - Slew degradation at sink $s$ (matching OpenSTA's Elmore slew approximation):
     $$\text{slew}(s) = \sqrt{\text{slew}(d)^2 + \bigl(\ln(9) \cdot \tau_{\text{Elmore}}(d \to s)\bigr)^2}$$
     (Compare against `sta` on `gcd.spef` and adjust the slew factor if `sta` uses $0.2\text{--}0.8$ threshold factor $\ln(0.8 / 0.2) = \ln(4) \approx 1.3863$ from the Liberty library's `slew_lower_threshold_pct` / `slew_upper_threshold_pct`).
