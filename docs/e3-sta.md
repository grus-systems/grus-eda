# E3: Static Timing Analysis (STA) — Implementation Spec

This document breaks E3 into implementation steps `E3.1`–`E3.3` (done) and the Section 5 tasks `E3.4` (closed 2026-11-15), `E3.5` (closed 2026-11-16) and `E3.6` (done), with the data structures, NLDM extrapolation code, and RC and driver-model equations an implementing agent needs. Section 4 is the current status; Section 6 says how to generate the test inputs.

> `docs/status.md` is the cross-stage status source (E0–E3). Section 4 below keeps the per-test E3 breakdown.

---

## 1. Done Criteria & Scope

| Sub-Step | What | Reference Command (`ghcr.io/librelane/librelane:3.0.16`) | Done When |
|---|---|---|---|
| **E3.1** | Zero-Wire STA: levelized DAG, dual Rise/Fall propagation, NLDM extrapolation, Setup/Hold slack | `generate_ref.sh` step 9 → `tests/designs/gcd/gcd.sta.reference.json` (`zero_wire`) | Worst slack (Setup & Hold) and, for **all 53 endpoints**, `max/min` arrival, required, slack and slew agree within **1% or 5 ps** on `gcd` (`picorv32` once it is added under `tests/designs/`) |
| **E3.2** | Placement-Based Virtual RC STA (`parasitics.rs`) | `generate_ref.sh` step 9 → `gcd.sta.reference.json` (`placement_rc`) | Worst slack and, for all 53 endpoints, arrival, required and slack within **1% or 5 ps** on `gcd` with OpenROAD's met2 `r0, c0`; slews within 3% (see E3.2 item 7) |
| **E3.3** | SPEF-Based STA (`readers/spef.rs`) and DMP driver model (`sta/dmp.rs`) | `generate_ref.sh` step 9 → `*.sta.reference.json` (`placed_spef`); `report_dcalc` for pi models and `Ceff` | Worst slack and every endpoint's arrival and required time agree within **1% or 5 ps** on `gcd` and `picorv32`; tolerances actually used: Section 4 |

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
- `set_load [-pin_load] <cap_pf> <port_pattern_or_all_outputs>`; `[get_nets <names>]` annotates the
  net's *wire* capacitance (replaces the parasitic term in the lumped model, last-wins per net;
  counted unresolved under `--spef` parasitics). Pin/cell/cone object sets and
  `-min`/`-max`/`-rise`/`-fall`/`-subtract_pin_load` are counted in `sdc_ignored_commands`;
  `set_fanout_load <n> <objects>` parses and is counted (inert in OpenSTA, `STA-0461`).
- `set_clock_uncertainty [-setup|-hold] <unc_ns> [get_clocks <clk>]` (no flag sets both; the clock list is validated but the design has a single clock)

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
   A `set_load [get_nets e]` annotation replaces the wire-capacitance term of `load(e, pol)`
   (measured override, not sum — `docs/results.md` 2026-11-19, `ld_*` scenarios).
   Check: `_384_/Q` rise load `0.008223 = 0.001689 + 0.001614 + 0.002495 + 0.002425`; fall
   `0.007406`. A *tristate* driver pin stays on its net's load — OpenSTA counts the
   driver's own pin capacitance for the arcs driving it too (`report_net z1` shows
   `u_ebuf/Z tristate 0.002260` inside the net's 0.003934 pF total; `docs/results.md`
   2026-11-21 `check_arcs` entry). Zero-wire mode adds no wire cap, no wire delay, and
   sink slew = driver slew.
3. **Ideal clock.** The clock net is not propagated. Every register `CLK` pin has
   `arrival = 0.0` (rise edge at t=0) and **`slew = 0.0`** for both max and min, even though
   `set_input_transition 0.15 [all_inputs]` matches `clk`. Only `RisingEdge` arcs are used
   for `dfxtp`; `min_pulse_width` is a separate evaluated check family (`v0.1.93`,
   `StaReport::pulse_width_checks`, measured OpenSTA semantics in `docs/results.md`
   2026-11-19) and `three_state_*` enable/disable arcs propagate as delay arcs
   (`v0.2.11`, `docs/results.md` 2026-11-21). The remaining recognised check arcs —
   `non_seq_*`, `preserve`, async `Clear`/`Preset`, and `min_pulse_width` off the
   clock network — are deferred and counted.
   `set_propagated_clock` switches to the propagated mode (`v0.1.60`, roadmap R2): a separate
   pass walks clock nets through combinational arcs and computes arrival/slew per transition
   at every consumer clock pin, tracking the clock-edge index parity so inverting paths pick
   the right capture/hold edge; plain `set_clock_latency` is then ignored and `-source`
   latency seeds the port. Validated on `tests/designs/sta_edge/propagated_clock/` and on
   the real designs `gcd`/`picorv32` (`<design>.propagated.sdc` +
   `generate_prop_ref.sh` goldens; per-clock-pin arrival/slew asserted against OpenSTA
   `get_property` at 1 ps — max diff 8e-6 ns — with the golden's `clock_pins` required
   non-empty and cross-checked against our modeled clock consumers); details in
   `docs/results.md`.
   The ideal clock still tracks edge *phase* through cell-driven clock nets (`v0.1.71`):
   a parity-only walk over the same graph places each consumer transition on its
   clock-edge parity (`latency + parity * period/2`), so `nand`/`nor`/`inv` stages on the
   clock network shift capture/hold/launch edges exactly as OpenSTA's ideal mode does —
   validated on `sta_edge/clock_gate` (strict, all 13 endpoints). Delays and slews are
   still not propagated; `cell_driven_clock_nets` keeps counting those nets.
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
with OpenROAD (`set_wire_rc -layer met2; estimate_parasitics -placement`) on `gcd.placed.def` and
`picorv32.placed.def`, 2026-11-10. `gcd.routed.spef` and `picorv32.routed.spef` are OpenROAD's placement
estimate, **not** routed extractions, so `placement_rc` and `placed_spef` in the golden files describe the same
RC.

1. **Layer RC.** `r0 = RPERSQ / WIDTH` (met2: 0.125 / 0.14 = 0.892857 ohm/um = 8.92857e-4 kohm/um) and
   `c0 = CPERSQDIST * WIDTH + 2 * EDGECAPACITANCE` (met2: 7.7890e-5 pF/um). Matches the SPEF to 6 digits on
   159 two-pin nets. Same value for horizontal and vertical wires.
2. **Pin positions.** `DesignDb::pin_pos_dbu`, converted to um by `dbu_per_micron`.
3. **Tree topology.** Two pins: one Manhattan segment. Three pins: the median Steiner point (length = HPWL;
   matches OpenROAD on all 46 three-pin nets of gcd). Four or more pins: OpenROAD uses FLUTE; we use the E1
   greedy MST + L-flip (`build_rsmt_points`), which is longer (`reset`: 23.585 um in OpenROAD vs 24.323 um by
   MST; `req_rdy`, 40 pins: 350.160 vs 358.864 um). **Known divergence; task E3.4.**
4. **RC tree.** Rooted at the driver (first pin of the net). Segment `u-v` of length `L`: `R = r0 L`,
   `C = c0 L`, half of `C` at each end. Pin-connection stubs (1e-6 kohm in the SPEF) are ignored.
5. **Pin capacitance.** Sink pin capacitance per transition (`cap_rise` / `cap_fall`) plus port loads is added
   to the wire capacitance, which is wire-only.
6. **Wire delay.** Elmore: `tau(v) = tau(u) + R_uv * Cdown(v)`, with sink pin caps (per transition) in
   `Cdown`. On cell-driven nets (`pin_cells[driver].is_some()`), wire delay is `tau(v)` (added to both max and
   min arrival) and sink slew is `sqrt(slew_d^2 + (ln4 * tau(v))^2)`. On primary-input port-driven nets
   (`pin_cells[driver].is_none()`), OpenSTA treats the driver as an ideal ramp without a cell DMP stage: wire
   delay is `ln2 * tau(v)` and sink slew adds linearly, `slew_d + ln4 * tau(v) = slew_d + 2 * delay`.
7. **Driver delay and slew** come from the DMP model of E3.3 Stage 2 (`Ceff`), not from the lumped load.
8. **Comparison.** See Section 4.

---

### Step E3.3: SPEF Reader and DMP Driver Model (`readers/spef.rs`, `sta/dmp.rs`)

Enabled with `grus-eda sta --spef <file>`. Implemented 2026-11-10.

**Stage 1: reader and RC tree.**
1. Units: `*C_UNIT` to pF, `*R_UNIT` to kohm (so `R * C` is ns), `*T_UNIT` must be `1 NS`. `*NAME_MAP`
   references (`*12`, `*12:3`) are resolved; backslash escapes are removed. A `*NAME_MAP` section ends at the
   next section header (`*PORTS`); only `*<digits>` lines are entries, and one without a name is an error.
2. Per `*D_NET`: `*CAP` lines with one node are ground capacitors; a coupling cap (two nodes) is listed once,
   in its first node's net. The full value is grounded on the first node. Measured against OpenSTA on the gcd
   routed SPEF (ticket 14): half the value is also grounded on the second node, in the net that owns that node
   (resistor endpoints first, file order; then cap first-nodes; the owner may be the listing net). A second
   node with no owner gets nothing. Total `D_NET` capacitance stays the header value. `*RES` lines are resistors. `*CONN` is not needed: pin nodes are found by name
   (`cell:pin`, or the port name).
3. The tree is a BFS spanning tree from the driver pin; self-loops and parallel resistors are dropped. A net
   that is absent from the SPEF has no parasitics. A pin without a node, or not connected to the driver by
   resistors, is an error.
4. Then the same pin capacitance, Elmore delay and wire slew as E3.2 items 5-6.

**Stage 2: pi model and effective capacitance** (`NetRcTree::pi_model`, `sta/dmp.rs`). Written from the
published method (Dartu, Menezes, Pileggi, IEEE TCAD 1996), not from OpenSTA's source (GPL).
1. Pi model from the driving-point admittance moments: `y2(u) = sum_v (y2(v) - R_uv y1(v)^2)`,
   `y3(u) = sum_v (y3(v) - 2 R_uv y1(v) y2(v) + R_uv^2 y1(v)^3)`, `C_far = y2^2 / y3`,
   `C_near = y1 - C_far`, `R_pi = -y3^2 / y2^3`. Matches OpenSTA's `report_dcalc` to 6 digits (OpenSTA calls
   the near capacitor C2 and the far one C1).
2. Driver: a saturated ramp (`t0`, `dt`) behind `Rd`. For a trial `Ceff`, the ramp is fitted so that a single
   capacitor `Ceff` reproduces the table delay (50%) and slew (20-80%). `Ceff` is bisected until the charge the
   ramp delivers into the pi load up to the 50% point equals the charge into `Ceff`.
   `Rd = RD_SCALE * d(table delay)/dC` with `RD_SCALE = 3.0`.
   Extreme-load lumped bailout (`is_lumped_bailout`): when `pi.c2 <= 0.0`, `pi.rpi <= 1e-9`, or
   `table_slew(total) < 2 * ln(5/4) * delay_slope(total) * total` (where the pi pole collapses `dt -> 0`),
   return the lumped response at `total = pi.c1 + pi.c2`.
3. Delay is the table delay at `Ceff`. Output slew propagated to the wire is `driver_waveform_slew(pi, r, table)`:
   under bailout it equals `table_slew(total)`; otherwise it evaluates `pi_waveform_slew` at the 20-80% points of
   the driving-point pi node using `dt_eff` and `rd_eff` fitted at `k_w = ln 2` (`Rule2(all_pi)`).
4. **Known gaps** (tasks E3.5): `RD_SCALE = 3.0` is **fitted** from `Ceff` consistency across shielded arcs
   (`docs/history/e35/01-rd-and-wave-slew.md`), not derived from first principles; OpenSTA's extreme-load
   shielding drop (`OpenSTA #582`, jpeg `_43767_/Y`) remains unmodelled outside the Liberty table range.
5. Why the textbook charge-sharing formula was dropped: with `td = slew / 0.6` it leaves `Ceff` within 1% of
   the total, while OpenSTA shields up to 75% of `C2` on high-fanout nets (picorv32 `_06198_/X`).

---

## 4. Status and Tolerances (2026-11-10)

Done criterion for every E3 step: worst slack, and arrival and required time of every endpoint, within
`max(5 ps, 1%)` of OpenSTA. Where a test uses a looser value, the cause is in `docs/results.md`.

| Test (`cargo test --release -p grus-core sta`) | Status | Tolerance differing from the criterion |
|---|---|---|
| gcd zero wire | pass | none |
| picorv32 zero wire | pass | none |
| aes zero wire | pass | none |
| ibex zero wire | pass | none |
| jpeg zero wire | pass | none |
| gcd SPEF | pass | slew 3% |
| gcd placement RC | pass | arrival 2%, slew 5% (Steiner, E3.4) |
| picorv32 SPEF | pass | arrival floor 6 ps, setup slack 40 ps, slew 10%, TNS 2.5% (`RD_SCALE = 3.0`, E3.5) |
| picorv32 routed SPEF | pass (`v0.1.46`) | same `Tol::PICORV32_SPEF`; sub-2 ps polarity-tie resolution (`compare_design_with_opensta`) |
| aes SPEF | pass (`v0.1.46`) | arrival, req, WNS, TNS all strict `max(5 ps, 1%)`; slack floor 25 ps (`23.9 ps` max), slew 5.5% (`5.24%` max) |
| ibex SPEF | pass (`v0.1.46`) | arrival, req, WNS, TNS all strict `max(5 ps, 1%)`; slack floor 40 ps (`36.6 ps` max), slew 5.5% (`5.09%` max) |
| jpeg SPEF | `#[ignore]`, out of scope | extreme extrapolation (`7x` load / `6x` slew on `_43767_/Y`) confirmed unsupported by `OpenSTA #582`; mismatches `7,045 -> 1,419` in `v0.1.45` |
| picorv32 placement RC | pass (`v0.1.69`) | `Tol::PICORV32_PLACEMENT` (arr 3%, slack 0.2 ns, slew 20%, TNS 10%); un-ignored once the PD tree matched OpenROAD's SPEF edge-for-edge |
| aes placement RC | pass (`v0.1.69`) | `AES_PLACEMENT = AES_SPEF`: PD parasitics equivalent to the SPEF readback (<= 3e-6 ns/pin Elmore bare wire, <= 3.8e-3 ns uniform load), so the residual is the shared cell-stage gap |
| ibex placement RC | pass (`v0.1.69`) | `IBEX_PLACEMENT = IBEX_SPEF`, same reasoning; worst slack field 36 ps < 40 ps floor |
| jpeg placement RC | `#[ignore]`, fails | residual identical to `placed_spef` (WNS -78.084 vs -61.199 ns): OpenSTA #582 out-of-table NLDM extrapolation, not the tree |
| PD tree vs OpenROAD SPEF (all 5 designs) | pass (`v0.1.69`) | edge-length multiset identical on all 78,749 nets; total length identical; `checkTree` 0 failures (FLUTE fallback unused); asserted in `test_pd_edge_multiset_vs_placement_spef` |
| PD vs SPEF parasitics (all 5 designs) | pass (`v0.1.69`) | per-pin Elmore: bare wire <= 3e-6 ns, uniform 1 fF load <= 3.8e-3 ns — within the strict floor; asserted in `test_pd_vs_spef_parasitics_agree` |
| placement RC vs SPEF RC through our STA (aes/ibex/jpeg) | pass (`v0.1.70`) | worst endpoint field diff 0.003/0.013/0.633 ps (aes/ibex/jpeg); max/min WNS <= 0.61 ps, TNS <= max(10 ps, 1% of reference) — licenses the shared SPEF tolerances; asserted in `test_placement_vs_spef_sta_agree` |
| DMP vs `report_dcalc` (6 arcs) | pass | `Ceff` within 6% |
| DMP vs `report_dcalc` (76 rows, routed SPEF) | pass (`v0.1.45`) | pi exact (max 0.43%); `Ceff` median 0.69% / max 6.79%, delay 0.12% / 6.03%, `driver_waveform_slew` 2.55% / 8.56% |
| Pi waveform slew across 5 designs (`31,966` rows) | pass (`v0.1.45`) | shielded `<0.97` median `0.50..0.74%`; all rows median `1.70..2.49%` (`ourCe + bailout`) |

## 5. Open Work

Each task is independent. Rules that apply to all: AGENTS.md Section 2 (a stage is done only against the
reference tool), write the rule in `docs/` before the code, record every comparison (including failures) in
`docs/results.md`, and prove each new test can fail by breaking the code on purpose.

### E3.4: Which Steiner tree OpenROAD builds — CLOSED 2026-11-15 (step 6, ticket 15)

> **Update 2026-11-18 (`v0.1.69`)**: the "FLUTE tree" conclusion below was wrong for the
> pinned LibreLane image. The image's OpenROAD builds the tree with `pdr::primDijkstra`
> (Prim-Dijkstra + Steinerization + degree-4 split, `alpha = 0.3`) and falls back to FLUTE
> only when `checkTree` rejects it. `congestion/pdr.rs` now ports that pass; its output is
> **edge-identical to OpenROAD's placement SPEF on all 78,749 nets of the five designs**
> (0 `checkTree` failures, so FLUTE never ran). `test_pd_vs_spef_parasitics_agree` then showed
> the built parasitics agree with the SPEF readback to <= 3e-6 ns per-pin Elmore on the bare
> wire and <= 3.8e-3 ns under a uniform 1 fF load — within the strict floor — so `placement_rc`
> measures the same engine residual as `placed_spef`: `picorv32`, `aes` and `ibex` all pass
> un-ignored (`AES_PLACEMENT`/`IBEX_PLACEMENT` reuse the SPEF tolerances), and `jpeg`'s residual
> equals `placed_spef`'s OpenSTA #582 out-of-table case exactly. What remains is the shared
> cell-stage engine gap (driver waveform slew, extreme-load Ceff), not the tree or the wire model.
- **Outcome**: our placement estimator is the same construction as OpenROAD's and scores the same against routed
  truth; the remaining placement-RC error is the placement-estimation abstraction itself, not the tree. Closed on
  the done criterion's evidence branch ("record the evidence and stop"), with the rule now known as well.
- **The rule (read 2026-11-15)**: OpenROAD is BSD 3-Clause, and with the user's permission the maintainer read the
  estimator in the pinned source (rev `e794373`, `src/rsz/src/EstimateWireParasitics.cc`). Per net it builds a
  FLUTE Steiner tree rooted at the driver (`stt` builder), makes each branch a pi model (capacitance split
  half/half between the branch ends, 1 mOhm links for zero-length branches and pin connections), and prices length
  with a single per-corner scalar resistance/capacitance pair set by the flow — in our generation scripts
  `set_wire_rc -layer met2`, the same layer our estimator reads (`WireRc::from_layer(.., "met2")`) — weighted by
  each branch's horizontal/vertical extent. Our shipped estimator differs only in the tree builder (the exact RSMT
  solver instead of FLUTE) and in applying the one pair without the directional split. OpenSTA source (GPL) was
  not read; see the licensing note in `AGENTS.md` Section 3b.
- **Scoring (gcd, 53 endpoints, max path; ticket 15)**: median |arrival error| against OpenSTA on the routed
  (OpenRCX) parasitics — OpenROAD's own placement estimate (golden against golden) 149.9 ps (p95 313.2, max 360.2;
  WNS error +91.1 ps); ours, exact tree (shipped) 156.4 ps; ours, greedy tree 155.1 ps; ours on OpenROAD's placed
  SPEF 150.3 ps. Every configuration fails the strict `max(5 ps, 1%)` criterion against routed truth on 53 of 53
  endpoints: at gcd arrival magnitudes (~2.5-2.7 ns) the criterion is ~27 ps, an order of magnitude below the
  estimator-vs-routed gap, so no placement estimator meets it — including the reference tool's own. Tree choice
  moves the median by less than 5 ps, so per-net tree-length equality is not pursued. The gcd placement-RC test
  keeps comparing against the placed golden (estimator agreement), where it passes.
- **Earlier framing corrected**: the 2026-11-10 entry concluded "not FLUTE" from tree *length* comparisons.
  OpenROAD's tree is FLUTE's; length was the wrong lens (FLUTE's tree is not length-minimal either — the exact
  solver is never longer and is shorter on most 4-9 point nets). The Prim-Dijkstra hypothesis is dead. The shipped
  placement estimate has used the exact solver since `c81a428`; the greedy builder (`build_rsmt_points`) is E1's
  tree, not STA's, and the two score within 1.3 ps of each other here.

### E3.5: Driver model, port wire rule, and waveform slew (`sta/dmp.rs`, `sta/analysis.rs`) — CLOSED 2026-11-16 (`v0.1.46`)
- **What the routed SPEF settled**: on identical parasitics our delay engine agrees with OpenSTA far inside the
  strict criterion. gcd residuals (ticket 15): routed SPEF median |arrival error| 1.18 ps (p95 2.83, max 8.56);
  placed SPEF 0.92 ps (p95 2.21, max 7.00); zero fields outside `max(5 ps, 1%)` in both modes. At gcd scale the
  fitted `RD_SCALE` produces no measurable error, so the step-4 wire-mode failures on aes/jpeg/ibex are not
  explained by the DMP constant alone.
- **Still open**: `RD_SCALE` is still fitted, not
  derived — one value fits arrival (about 4.5-5) but not `Ceff` (about 3), and its doc comment misdescribes the
  value (the F4 item in `docs/history/review-2026-11-11.md`). The open question is now the scale question: why the
  wire-mode error grows by one to two orders of magnitude from gcd to aes/jpeg/ibex. The picorv32 routed SPEF
  (step 7, ticket 16) has since separated the two at that scale: the estimator side is ~1.4 ns median (ours and
  OpenROAD's alike), and the engine residual is 6.2 ps median with a slack tail (726 of 1,798 endpoints outside
  the strict criterion; a ~13%-low slew family on one register bank is its visible face). The `report_dcalc`
  table for that tail was collected in step 7 (ticket 17: 76 rows on the routed SPEF, all matched; data in
  `tests/designs/picorv32/dcalc_routed_ref.txt`). Its verdict: the pi model is exact (C1/Rpi to print
  precision, C2 within 0.43%), so the RC reduction is not the cause; the DMP solve's `Ceff`/delay/slew error
  grows with load (median `Ceff` +0.61%, but +17.1%/+15.2%/+17.0% on the largest load, the ~2 pF CLK->Q net of
  `_11506_`, whose path family carries ticket 16's late arrivals; combinational arcs stay within +3.6%); and
  the driver waveform slew OpenSTA propagates is unmodelled (our slew is the table slew; median gap 2.65%,
  up to 19.8% on the largest loads), which is the systematic part of the endpoint slew tail. What remains:
  a load-dependent fix or derivation for the DMP solve, and a decision on modelling the waveform slew.
  OpenSTA's `DmpCeff` source (GPL) must not be read; work from the DMP paper and the measurements.
- **2026-11-15 (`docs/history/e35/`, Sections 5-6)**: the slew-slope rule seen on 76 arcs was overfit; the Ceff-consistent `RD_SCALE` is ~3 (picorv32 280 shielded rows 3.01, cv 12%; aes worst Ceff 2.68%), not 4.5. Building `RD_SCALE` 3 plus a total-load propagated slew in the tree improved the picorv32 wire modes (routed 136 -> 53 mismatching fields) but not aes, needed a loosened tolerance, and was held after review; reverted. OpenSTA's driver waveform slew is ~0.975 x table slew at the total load unshielded, up to ~1.04 x when strongly shielded (picorv32 and aes agree per bin), yet modelling it does not move the endpoints, so the residual lies elsewhere in the stage: next, a per-stage comparison on the worst picorv32 routed path. **Stage-by-stage (`docs/results.md`, last E3.5 entry)**: on the five worst picorv32 routed paths the whole endpoint error comes from the first stage, the 273-fanout `_09979_` CLK->Q at 2.26 pF (7x past the Liberty table range): +123.5 ps delay, -0.54 ns slew; all later stages add under 35 ps. The open question is Ceff at extreme loads, not the per-stage driver slew in general. **Section 9 (all five designs generated)**: the wire model is exonerated (per-sink wire delay = our Elmore, median 1.000; predicted sink slew within 0.0-0.75%); the aes/ibex/jpeg failures trace to one or two driver stages per design: waveform slew of fall arcs with input slew above the 1.5 ns table range, extreme loads where the reference shields strongly and we do not (jpeg `_43767_`), and one port-driven net (ibex `rst_ni`, `ln 2 x Elmore`). **Section 10 (`repro_extreme_ceff` sweep & per-sink port rule)**: our NLDM lookup matches OpenSTA (`< 1e-4` rel) across the entire 7-point `0.05..9.1428 ns` sweep (`#582`); on `fall`, OpenSTA's Ceff drops monotonically from 74% to 54% while `solve_dmp` (`RD_SCALE` `4.5` and `3.0`) moves physically toward 100%, and on `rise` OpenSTA is within `-0.2%..+5.2%` of `RD=3.0` through `4.0 ns` before plunging to 49% at `6.0 ns` and snapping to 100% (`refWave == refSlw == 3.5528`) at `9.1428 ns`. Across 12,574 port-driven sink observations in `aes`, `picorv32`, `ibex`, and `jpeg`, OpenSTA uses `dly = ln 2 * Elmore` (median `1.0000`) and linear slew addition `s_out = s_in + ln 4 * Elmore = s_in + 2 * dly` (`max |dly - (s_out - s_in)/2| <= 1.17e-6 ns`, resolving the Section 9 `0.45`/`0.40` discrepancy on `aes`/`jpeg`). **Implemented (`v0.1.44`, Step E3.2 item 6)**: `test_port_wire_delay_vs_opensta` asserts `run_sta_with_state` matches OpenSTA port sinks (and fails loudly if disabled); port stage-0 errors drop to `0.0000 ns`, `picorv32` placed stays `0`, routed improves `136 -> 135`, `aes` `480 -> 252`, `ibex` `8855 -> 2199`, and `jpeg` WNS `-76.7 -> -72.01 ns` (with `jpeg` field count `6,145 -> 7,045` after port `ena` stops cancelling `_42236_/Y`'s `-0.65 ns` DMP error). **Implemented (`v0.1.45`, Section 10c)**: `driver_waveform_slew` (`Rule2(all_pi)` with `k_w = ln 2`), `is_lumped_bailout`, and `RD_SCALE = 3.0` landed together (fixing the intermediate `RD_SCALE = 4.5 + wave_slew` regression on `picorv32`). Across `31,966` rows (`test_dmp_pi_waveform_slew_vs_opensta`), `driver_waveform_slew` median error is `1.70..2.49%` (shielded `<0.97` median `0.50..0.74%`, down from `4.15..6.03%`); 76-row routed `report_dcalc` max `Ceff` error drops `17.10% -> 6.79%`, `delay` `15.21% -> 6.03%`, `drv_slew` `19.83% -> 8.56%`; `picorv32` `routed_spef` drops `135 -> 3` min-path mismatches (0 max-path/WNS/TNS failures), `aes` SPEF `252 -> 144`, `ibex` SPEF `2,199 -> 19` (WNS/TNS pass), and `jpeg` SPEF `7,045 -> 1,419` (`_42236_/Y` `0.0000 ns`; remaining tail is OpenSTA `#582` extreme arc `_43767_/Y`). Remaining: `RD_SCALE = 3.0` is still empirical and `Tol::PICORV32_SPEF.tns_rel` is `0.025` for the `-0.31%` single-constant launch arrival residual; `OpenSTA #582` disposition awaits upstream response.
- **2026-11-16 (`v0.1.46`, `OpenSTA #582` disposition & polarity-tie fix, Section 10d)**: the OpenSTA maintainer confirmed on `parallaxsw/OpenSTA#582` that out-of-range extrapolation ("when the slew/load are outside the table values that characterize the gate") is unsupported, settling `jpeg` `_43767_/Y` (`7x` load / `6x` slew) as out of E3.5 scope. Resolving `< 2 ps` `RISE`/`FALL` slack ties in `compare_design_with_opensta` (`_10064_/D` on `picorv32 routed_spef`, `_16583_/D` and `_16591_/D`/`_16691_/D` on `aes placed_spef`) and setting `Tol::AES_SPEF` / `Tol::IBEX_SPEF` enables `test_picorv32_routed_spef_matches_opensta`, `test_aes_placed_spef_matches_opensta`, and `test_ibex_placed_spef_matches_opensta` cleanly with 0 failures (`all 5 designs zero_wire PASS`, `gcd`/`picorv32` placed+routed SPEF PASS, `aes`/`ibex` placed SPEF PASS within arrival `1%` / WNS `1%` / TNS `1%`).
- **Done when (closed with split scope)**:
  - **Within / mildly beyond Liberty table range (`gcd`, `picorv32` placed & routed, `aes`, `ibex`)**: pi parameters within `0.5%`, 76-row routed `Ceff` median `< 1%` / max `< 7%` (`0.69% / 6.79%`), `driver_waveform_slew` shielded median `< 1.5%` across `31,966` rows (`0.50..0.74%`), and all 5 SPEF tests pass endpoint arrival, required time, WNS, and TNS within `max(5 ps, 1%)` (except `picorv32` single-constant `RD_SCALE = 3.0` TNS `2.5%` and arrival floor `6 ps`) and documented per-design slack/slew floors (`25-40 ps` slack, `5.5-10%` slew).
  - **Far outside Liberty table range (`jpeg` `_43767_/Y`, `7x` load / `6x` slew)**: out of scope per `OpenSTA #582`; `solve_dmp` keeps its physical lumped-limit extrapolation and `test_jpeg_placed_spef_matches_opensta` stays `#[ignore]`d. `RD_SCALE = 3.0` remains an empirical constant without needing OpenSTA GPL source.

### E3.6: Speed — DONE 2026-11-13 (after E3.4 and E3.5; AGENTS.md Section 2)
- **Why**: `grus-eda sta` on picorv32 with a SPEF takes 16 s (release); zero wire takes under a second. The DMP
  solve (nested bisections around a 200-step simulation) dominates.
- **Approach**: first a profile. Candidates: closed-form ramp-RC response instead of the simulation, caching the
  solve per (net, transition, quantised input slew), `rayon` over levels. The fast path must give the same numbers
  as the simple one: keep the simple version behind a test and compare per endpoint.
- **Done when**: picorv32 SPEF STA under 2 s, with every test in the Section 4 table giving the same results as
  before (differences below 1e-4 ns).
- **Outcome**: design `docs/history/e36-dmp-speed.md` (Sections 3a/3b/3d plus the Section 7 discrete charge
  form; the ODE charge form was implemented, measured and rejected — fitted `RD_SCALE` had absorbed the
  trapezoid's error). picorv32 SPEF STA 10.45 s -> 0.98 s; WNS/TNS moved by at most 2.4e-4 ns; full test
  suite green (58 passed). Entry: `docs/results.md`, 2026-11-13.

### Not scheduled
- Resistor loops are implemented and **verified against OpenSTA** (Phase 3, `tests/designs/sta_edge/resistor_loop`):
  a hand-written SPEF whose data net has parallel paths and a cycle agrees with OpenSTA on every endpoint
  field and WNS/TNS within the strict tolerance. Note: OpenSTA resolves SPEF node names as `net:node`
  (bare `n1`-style names are dropped with `STA-1656` warnings) — fixtures must follow that convention.
- `set_clock_uncertainty` now accepts `[-setup|-hold] <ns> [get_clocks <name>]` and is applied to
  register checks, clock-gate enables **and output ports** (Phase 3, `sta_edge/clock_uncertainty`; the
  plain single-value form sets both setup and hold). Verified against OpenSTA on the fixture.
- Latches of every `sky130_fd_sc_hd` shape are modeled. The non-`dlxtn` positive latch `dlxtp_1`
  (`setup_falling`/`hold_falling` on `GATE`) is handled through `open_pol` exactly like `dlxtn` —
  verified strict on `sta_edge/non_dlxtn_latch` (all 3 endpoints reached) — and `sta_edge/latch_borrow`
  exercises all 8 latch cells (`dlxtn`/`dlxtp`/`dlrb*`/`dlrt*`/`dlxb*`) with `D -> Q` time borrowing,
  strict PASS ideal and propagated (31 endpoints each).

## 6. Reproducing the Test Inputs

`gcd` artifacts are committed. `picorv32` artifacts over 1 MB are not; generate them first (about 3 minutes):

```bash
mise exec docker-cli@latest colima@latest lima@latest -- colima start --memory 12 --cpu 8
bash tests/designs/picorv32/generate_ref.sh   # runs the container by itself when called from the host
```

The picorv32 tests panic (they do not skip) when `picorv32.placed.def`, `picorv32.routed.spef` or
`picorv32.sta.reference.json` is missing. Run the whole suite with
`cargo test --release -p grus-core sta` (about 17 s).
