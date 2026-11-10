# E3: Static Timing Analysis (STA) — Implementation Spec

This document breaks E3 into three progressive implementation steps (`E3.1`–`E3.3`) with exact data structures, NLDM extrapolation code, and Elmore RC equations for the implementer.

---

## 1. Done Criteria & Scope

| Sub-Step | What | Reference Command (`ghcr.io/librelane/librelane:3.0.16`) | Done When |
|---|---|---|---|
| **E3.1** | Zero-Wire STA: levelized DAG, dual Rise/Fall propagation, NLDM extrapolation, Setup/Hold slack | `generate_ref.sh` step 9 → `tests/designs/gcd/gcd.sta.reference.json` (`zero_wire`) | Worst slack (Setup & Hold) and, for **all 53 endpoints**, `max/min` arrival, required, slack and slew agree within **1% or 5 ps** on `gcd` (`picorv32` once it is added under `tests/designs/`) |
| **E3.2** | Placement-Based Virtual RC STA (`parasitics.rs`) | `generate_ref.sh` step 9 → `gcd.sta.reference.json` (`placement_rc`) | Worst slack and, for all 53 endpoints, arrival, required and slack within **1% or 5 ps** on `gcd` with OpenROAD's met2 `r0, c0`; slews within 3% (see E3.2 item 7) |
| **E3.3** | SPEF-Based STA (`readers/spef.rs` + Elmore tree) | `sta`: `read_spef`, `report_checks -path_delay min_max` | Worst slack and every endpoint's arrival agree within **1% or 5 ps** on extracted `.spef` |

> **Scope Rule**: E3 targets edge-triggered flip-flop designs (`dfxtp`, `dfrtp`, `dfxbp`, etc.). Transparent level-sensitive latches (`dlxtp`, time borrowing across cycle boundaries) are out of initial scope; if a design instantiates a latch, emit a clear diagnostic warning.

---

## 2. Concrete Data Structures (`crates/grus-core/src/sta/`)

Files: `nldm.rs` (table lookup), `sdc.rs` (SDC subset), `analysis.rs` (graph, propagation, endpoint report).
The first implementation is the simple CPU baseline (AGENTS.md Section 2): adjacency is `Vec<Vec<u32>>`, not CSR.

```rust
pub const RISE: usize = 0;
pub const FALL: usize = 1;

struct ArcEdge { dst: PinId, lib_arc: u32 }          // combinational arc; edges_from[src_pin] lists them
struct LaunchArc { clk: PinId, q: PinId, lib_arc: u32 } // RisingEdge CLK -> Q
struct RegisterCheck { clk: Option<PinId>, setup_arc: Option<u32>, hold_arc: Option<u32> } // keyed by D pin

struct PinTimingState {
    arrival_max: [f32; 2], arrival_min: [f32; 2], // [RISE, FALL], ns; UNREACHED = -inf / +inf
    slew_max: [f32; 2],    slew_min: [f32; 2],
}
```

`required_*` are **not** stored per pin: zero-wire reporting needs them only at endpoints, so they are
computed there. A full backward pass is added when a feature needs required times at internal pins.
Endpoints are sorted by name; `PinId` has no `Ord`, so the check map is keyed by the raw `u32`.

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
    let m = table.index_1.len();
    let n2 = table.index_2.len();
    if m == 1 && n2 == 1 {
        return table.values[0];
    }
    let (i, u) = find_interval_unclamped(&table.index_1, x);
    let i1 = (i + 1).min(m - 1);
    if n2 <= 1 {
        let v0 = table.values[i];
        let v1 = table.values[i1];
        return (1.0 - u) * v0 + u * v1;
    }
    let (j, v) = find_interval_unclamped(&table.index_2, y);
    let j1 = (j + 1).min(n2 - 1);
    if m <= 1 {
        let v0 = table.values[j];
        let v1 = table.values[j1];
        return (1.0 - v) * v0 + v * v1;
    }
    let t00 = table.values[i * n2 + j];
    let t10 = table.values[i1 * n2 + j];
    let t01 = table.values[i * n2 + j1];
    let t11 = table.values[i1 * n2 + j1];
    (1.0 - u) * (1.0 - v) * t00
        + u * (1.0 - v) * t10
        + (1.0 - u) * v * t01
        + u * v * t11
}
```

#### Minimal SDC Reader (`sta/sdc.rs`)
To ensure `grus-eda sta` and OpenSTA evaluate the exact same boundary conditions, parse the benchmark `.sdc` subset:
- `create_clock [-name <name>] -period <period_ns> [get_ports <clk_port>]`
- `set_input_delay <delay_ns> [-clock <clk>] <port_pattern_or_all_inputs>`
- `set_output_delay <delay_ns> [-clock <clk>] <port_pattern_or_all_outputs>`
- `set_input_transition <slew_ns> <port_pattern_or_all_inputs>` (or `set_driving_cell -lib_cell <cell> -pin <pin>`)
- `set_load [-pin_load] <cap_pf> <port_pattern_or_all_outputs>`
- `set_clock_uncertainty <unc_ns> <clk>`

#### Dual Polarity Propagation & OpenSTA Worst-Slew Merge Rule (`propagate.rs`)
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
         - **Independent Arrival & Slew Merge (OpenSTA Rule)**: OpenSTA's `GraphDelayCalc` merges slews independently of which arc wins arrival time:
           ```rust
           state[y].arrival_max[dst_pol] = state[y].arrival_max[dst_pol].max(cand_arr_max);
           state[y].arrival_min[dst_pol] = state[y].arrival_min[dst_pol].min(cand_arr_min);
           state[y].slew_max[dst_pol]    = state[y].slew_max[dst_pol].max(cand_slew_max);
           state[y].slew_min[dst_pol]    = state[y].slew_min[dst_pol].min(cand_slew_min);
           ```
3. **Setup & Hold Endpoint Checks**:
   - At each `TimingCheck` on flip-flop data pin $D$ with rising clock pin $CLK$:
     - **Setup (Max Path)** for data polarity $\text{pol} \in \{\text{RISE}, \text{FALL}\}$:
       $$t_{\text{setup}}(\text{pol}) = \text{eval\_nldm\_constraint}\bigl(\text{constraint\_table}(\text{pol}),\; \text{slew}_{\max}(D, \text{pol}),\; \text{slew}_{\max}(CLK, \text{RISE})\bigr)$$
       $$\text{req}_{\max}(D, \text{pol}) = T_{\text{period}} + \text{arr}_{\min}(CLK, \text{RISE}) - t_{\text{setup}}(\text{pol}) - t_{\text{uncertainty}}$$
       $$\text{slack}_{\text{setup}}(D, \text{pol}) = \text{req}_{\max}(D, \text{pol}) - \text{arr}_{\max}(D, \text{pol})$$
     - **Hold (Min Path)** for data polarity $\text{pol} \in \{\text{RISE}, \text{FALL}\}$:
       $$t_{\text{hold}}(\text{pol}) = \text{eval\_nldm\_constraint}\bigl(\text{constraint\_table}(\text{pol}),\; \text{slew}_{\min}(D, \text{pol}),\; \text{slew}_{\max}(CLK, \text{RISE})\bigr)$$
       $$\text{req}_{\min}(D, \text{pol}) = \text{arr}_{\max}(CLK, \text{RISE}) + t_{\text{hold}}(\text{pol}) + t_{\text{uncertainty}}$$
       $$\text{slack}_{\text{hold}}(D, \text{pol}) = \text{arr}_{\min}(D, \text{pol}) - \text{req}_{\min}(D, \text{pol})$$
   - (Not implemented in E3.1: required times at internal pins. See Section 2.)

#### E3.1 Reference Semantics (measured with OpenSTA on `gcd`, 2026-11-10; implemented 2026-11-10)

Every rule below was checked against `report_checks -fields {slew cap input_pins} -digits 6` on
`gcd.placed.def` + `gcd.sdc` in `ghcr.io/librelane/librelane:3.0.16`. Where this section and the
generic text above disagree, this section wins.

1. **Inputs.** `grus-eda sta --lib <.lib> --tlef --lef --def gcd.placed.def --sdc gcd.sdc --parasitics none`.
   Load the design with `parse_def(def, &tech_db, Some(&lib_db))` so `pin_lib_pins` is filled. The
   golden file is `tests/designs/gcd/gcd.sta.reference.json`; the test must deserialize it and fail
   loudly if it is missing.
2. **Net load is per transition.** For an output pin `y` driving net `e` with transition `pol`:
   `load(e, RISE) = Σ_sinks cap_rise + port_load`, `load(e, FALL) = Σ_sinks cap_fall + port_load`,
   where `port_load` is the SDC `set_load` value on any primary-output port on `e` (0.05 pF in gcd).
   Check: `_384_/Q` rise load `0.008223 = 0.001689 + 0.001614 + 0.002495 + 0.002425`; fall
   `0.007406`. Zero-wire mode adds no wire cap, no wire delay, and sink slew = driver slew.
3. **Ideal clock.** The clock net is not propagated. Every register `CLK` pin has
   `arrival = 0.0` (rise edge at t=0) and **`slew = 0.0`** for both max and min, even though
   `set_input_transition 0.15 [all_inputs]` matches `clk`. Only `RisingEdge` arcs are used
   for `dfxtp`; `min_pulse_width` and other non-delay arcs are ignored.
4. **Clock-to-Q.** `RisingEdge` arcs are `non_unate`: evaluate `CLK(RISE) → Q(RISE)` with
   `cell_rise`/`rise_transition` and `CLK(RISE) → Q(FALL)` with `cell_fall`/`fall_transition`,
   both at `in_slew = 0.0` and `out_cap = load(net(Q), pol)`. Check: `_384_/Q` rise
   delay `0.309059`, slew `0.055303` (linear extrapolation below `index_1[0] = 0.01`).
5. **Primary inputs.** Arrival = `set_input_delay` (1.0 ns) for both transitions and both
   max/min; slew = `set_input_transition` (0.15 ns) regardless of load (ideal driver, no
   `set_driving_cell` in gcd). `clk` is excluded (`-no_clocks`).
6. **Combinational arcs.** Use `timing_sense` exactly (`positive_unate`, `negative_unate`,
   `non_unate`). Multiple `timing()` groups may share the same `related_pin` with different
   `when` conditions (e.g. `xnor2` has a negative and a positive arc from `A`); evaluate **every**
   arc and merge with max (late) / min (early). Ignore `when`.
7. **Max and min are independent propagations.** Late: arrival and slew merged with `max`.
   Early: arrival and slew merged with `min`. Slews are merged independently of which arc wins
   the arrival (OpenSTA `GraphDelayCalc`).
8. **Register setup/hold.** For data pin `D` with transition `pol` and the `setup_rising` /
   `hold_rising` arc: table = `rise_constraint` if `pol == RISE` else `fall_constraint`. The
   `vio_3_3_1` template has `variable_1 = related_pin_transition`, `variable_2 =
   constrained_pin_transition`, so call `eval_nldm_constraint(table, constrained = slew(D, pol),
   related = 0.0)` (ideal clock slew), using late slew for setup and early slew for hold.
   - `req_max(D, pol) = period + 0.0 − setup(pol)`; `slack_max = req_max − arr_max(D, pol)`.
   - `req_min(D, pol) = 0.0 + hold(pol)`; `slack_min = arr_min(D, pol) − req_min`.
   - Check: `_395_/D` fall, slew 0.048424 → setup 0.124895, req 4.875105; `_399_/D` rise, early
     slew 0.025549 → hold −0.033064.
9. **Primary-output checks.** `req_max = period − output_delay` (4.0), `req_min = −output_delay`
   (−1.0); `slack_max = req_max − arr_max`, `slack_min = arr_min − req_min`.
10. **Endpoint reduction (what the golden file stores).** For each endpoint the reported row is
    the transition with the **worst slack** (smallest `slack_max` for max; smallest `slack_min`
    for min). Store that transition's `arr`, `req`, `slack`, and the endpoint pin's slew for the
    same transition and min/max. `wns_max_ns = min over endpoints of max_slack_ns`; likewise
    `wns_min_ns`. `tns = Σ min(0, slack)`.
11. **Units.** Liberty time unit `1ns`, cap unit `1pF` in sky130; keep everything in ns / pF `f32`.
12. **Comparison.** Per endpoint and per field: pass if `|ours − ref| <= max(0.005, 0.01 * |ref|)`.
    Report the max abs diff per field in `docs/results.md`, and list any failing endpoint by name.

---

### Step E3.2: Placement-Based Virtual RC (`sta/parasitics.rs`)

Enabled with `grus-eda sta --parasitics placement [--wire-layer met2]`. Every rule below was measured
with OpenROAD (`set_wire_rc -layer met2; estimate_parasitics -placement`) on `gcd.placed.def`,
2026-11-10. `gcd.routed.spef` is byte-identical (except the date) to OpenROAD's placement estimate:
despite its name it is **not** a routed SPEF, so `placement_rc` and `placed_spef` in the golden file agree.

1. **Layer RC.** `r0 = RPERSQ / WIDTH` (met2: 0.125 / 0.14 = 0.892857 ohm/um = 8.92857e-4 kohm/um) and
   `c0 = CPERSQDIST * WIDTH + 2 * EDGECAPACITANCE` (met2: 7.7890e-5 pF/um). Checked on 159 two-pin nets:
   `c0` and `r0` match the SPEF to 6 digits for every net. Same value for horizontal and vertical wires.
2. **Pin positions.** `DesignDb::pin_pos_dbu`, converted to um by `dbu_per_micron`.
3. **Tree topology.** Two pins: one Manhattan segment. Three pins: the median Steiner point (total
   length = HPWL; matches OpenROAD on all 46 three-pin nets). Four or more pins: OpenROAD uses FLUTE,
   which is shorter than the greedy MST + L-flip of E1 (`reset`: 23.585 vs 24.323 um by MST; `req_rdy`,
   40 pins: 350.160 vs 358.864 um by MST). We use the E1 core (`build_rsmt_points`, now generic over integer
   points); the length gap on these nets is a **known divergence**, not yet measured at the net level.
4. **RC tree.** Rooted at the driver (first pin of the net). Segment `u-v` of length `L`: `R = r0 L`,
   `C = c0 L`, half of `C` at each end. Pin-connection stubs (1e-6 kohm in the SPEF) are ignored.
5. **Load.** `load(net, pol) = total wire C + sum of sink pin caps (per transition) + port loads`. The driver
   delay uses this lumped value (see the DMP note below).
6. **Wire delay.** Elmore: `tau(v) = tau(u) + R_uv * Cdown(v)`, with sink pin caps (per transition) in
   `Cdown`. Added to both max and min arrival. Sink slew: `sqrt(slew_d^2 + (ln4 * tau)^2)`.
7. **Known gap: DMP driver slew.** OpenSTA's `dmp_ceff_elmore` reduces the tree to a pi model and takes the
   driver slew from a waveform model. `report_dcalc` shows the result differs from the plain table lookup
   (`Slew = 0.282240` from the table, `Driver waveform slew = 0.278732`; `Ceff = 0.054675` vs total
   0.054938). Arrival, required time and slack are not affected beyond tolerance on `gcd`; slews at output
   ports are 1.7-2.7% high. Closing this is the Ceff work of E3.3 Stage 2.
8. **Comparison.** Arrival, required time and slack: `max(5 ps, 1%)`. Slew: `max(5 ps, 3%)` until item 7 is done.

---

### Step E3.3: SPEF Reader & RC Tree Elmore Propagation (`readers/spef.rs`)

> **Status (2026-11-10)**: Stage 1 (reader, spanning tree, Elmore, wire slew) is implemented in `readers/spef.rs`
> and `sta/parasitics.rs::spef_parasitics`. Stage 2 (pi model, DMP `Ceff`) is implemented in `sta/dmp.rs` with a
> fitted driver resistance; the driver waveform slew is not. The notes below are the history that led there:
> the formulas below were never checked against OpenSTA, and `report_dcalc` shows that OpenSTA's driver slew
> comes from a waveform model, so a pi model and `Ceff` alone would not close the slew gap. Measure
> `report_dcalc` pi models first, then write the rule here before coding.
>
> **Correction (picorv32, 2026-11-10)**: the O'Brien-Savarino charge-sharing formula in Stage 2 below does not
> reproduce OpenSTA's `Ceff`: on the pi models OpenSTA prints it removes < 1% of the shielding that OpenSTA
> applies (up to 75% of `C2`). The pi-model moments (`y1..y3`) are right in form; the `Ceff` solve is not.
> See `docs/results.md`, picorv32 entry.

1. **SPEF Units & Header**:
   - Parse `*T_UNIT` (convert to `ns`), `*C_UNIT` (convert to `pF`), `*R_UNIT` (convert to $\text{k}\Omega$ so $R \cdot C = \text{k}\Omega \cdot \text{pF} = \text{ns}$ directly!), and `*NAME_MAP` (`*123 -> net_or_inst_name`).
2. **Per-Net `*D_NET *net_id total_cap`**:
   - Parse `*CONN` (identifies `*I inst:pin` and `*P port` nodes, with `*D` marking the driver).
   - Parse `*CAP` (ground capacitors `id node cap_val`; ignore coupling capacitors or lump them to ground).
   - Parse `*RES` (resistors `id node1 node2 res_val`).
3. **Two-Pass Elmore Delay, Driving-Point $\pi$-Model, and $C_{\text{eff}}$**:
   - Root the RC graph at the driver node $d$ (spanning tree BFS if parallel resistor loops exist):
   - Add sink pin Liberty capacitance $C_{\text{pin}}(s, \text{pol})$ to each sink node $s$ (OpenROAD SPEFs contain **wire-only** capacitance in `*CAP`, so always add `LibPin::cap_rise` / `cap_fall` at sink pins).
   - **Stage 1 (Lumped Cap + Elmore Delay Baseline)**:
     - Run the bottom-up $C_{\text{down}}$ and top-down $\tau_{\text{Elmore}}$ pass as in Step E3.2, with total lumped capacitance $C_{\text{total}} = C_{\text{down}}(d)$.
     - Verify against OpenSTA first with `sta`'s lumped-cap / Elmore calculator (`set_delay_calculator lumped_cap` or short-wire `gcd` where $R_{\text{wire}} \ll R_{\text{driver}}$).
   - **Stage 2 (pi model and DMP effective capacitance, `sta/dmp.rs`)**: implemented 2026-11-10 from the
     published method (Dartu, Menezes, Pileggi, IEEE TCAD 1996), not from OpenSTA's source (GPL).
     - Pi model from the driving-point admittance moments `y1, y2, y3` of the RC tree
       (`NetRcTree::pi_model`): `y2(u) = sum_v (y2(v) - R_uv y1(v)^2)`,
       `y3(u) = sum_v (y3(v) - 2 R_uv y1(v) y2(v) + R_uv^2 y1(v)^3)`, `C_far = y2^2 / y3`,
       `C_near = y1 - C_far`, `R_pi = -y3^2 / y2^3`. Matches OpenSTA's `report_dcalc` pi models to 6 digits
       on six picorv32 arcs (OpenSTA calls the near capacitor C2 and the far one C1).
     - Driver: a saturated ramp (`t0`, `dt`) behind `Rd`. For a trial `Ceff` the ramp is fitted so that a
       single capacitor `Ceff` reproduces the table delay (50%) and slew (20-80%); `Ceff` is bisected until the
       charge the ramp delivers into the pi load up to the 50% point equals the charge into `Ceff`.
       `Rd = RD_SCALE * d(table delay)/dC`.
     - **`RD_SCALE` is fitted, not derived.** The natural value is 1/ln 2 = 1.44; the best arrival agreement on
       picorv32 is at 4.5, and no single value fits every arc (see docs/results.md). The charge-sharing
       formula that stood here before (`Ceff = C1 + C2 (1 - Rpi C2 / td (1 - e^(-td / Rpi C2)))`) was removed:
       it leaves `Ceff` within 1% of the total, while OpenSTA shields up to 75% of `C2`.
     - Delay is the table delay at `Ceff`. Slew is the table slew at `Ceff`; OpenSTA's driver waveform slew
       differs from it by 1-18% on the arcs inspected and is **not** modelled.
   - **Wire Slew Degradation**:
     $$\text{slew}(s) = \sqrt{\text{slew}(d)^2 + \bigl(\ln(4) \cdot \tau_{\text{Elmore}}(d \to s)\bigr)^2}$$
     where $\ln(0.8 / 0.2) = \ln(4) \approx 1.3863$ matches `sky130_fd_sc_hd`'s $20\%\text{--}80\%$ slew thresholds (`slew_lower_threshold_pct_rise : 20.0`, `slew_upper_threshold_pct_rise : 80.0`).
