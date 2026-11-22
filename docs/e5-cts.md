# E5: Clock-Tree Synthesis — Implementation Spec

E5 ticket 3 (`e5-cts-tree`, `v0.5.0`): build a depth-balanced buffer tree on
every design clock's root net, legalize the inserted cells only, emit a CTS
DEF and a census report. TritonCTS reference artifacts for `gcd` and
`picorv32` were committed by `e5-cts-ref`; the `DesignDb` mutation API came
from `e5-insert-api`. The strict skew/latency golden comparison is the next
ticket (`e5-cts-e2e`) — this stage asserts the tree *class* (depth, census,
legality, propagated-clock reachability), not field-exact equivalence.

---

## 1. Done Criteria (this ticket)

| What | Reference | Done When | Status |
|---|---|---|---|
| `grus-eda cts` end to end | TritonCTS `clock_tree_synthesis` on `gcd.placed.def`, `picorv32.placed.def` | emitted DEF `parse_def` re-reads + `verify_legality` accepts; clock nets decomposed into buffer-driven segments | **met** (`cts::tests::test_cts_def_roundtrip_*`, `cli_cts_parse_and_gcd_e2e`) |
| Depth + buffer census | `<d>.cts.reference.json` | TritonCTS league: depth 2 / single-digit buffers on gcd, ~depth 4 / ~200 on picorv32; never one-buffer-per-sink | **met** (gcd 6 bufs/depth 2, picorv32 230 bufs/depth 4 — see `docs/results.md`) |
| Report JSON | `<d>.cts.reference.json` | emitted per run (`--report`); buffer counts per type, sink count, depth, clock-net HPWL; field names mirror the reference | **met** (schema below) |
| Propagated-clock STA | `<d>.cts.sta.reference.json` | runs in `cargo test` on our CTS DEF; gcd arrival spread + insertion delay recorded as **informational** | **met** (`test_cts_propagated_sta_*`); strict comparison deferred to `e5-cts-e2e` — stated, not claimed |
| Mutation boundary | — | non-clock components/nets byte-identical in emitted DEF; control-net HPWL unchanged | **met** (`test_cts_untouched_def_identical`) |
| Checks | — | `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings`; fail-proof red/restore recorded | **met** (see `docs/results.md`) |

---

## 2. Algorithm

`crates/grus-core/src/cts/` — `mod.rs` holds `CtsConfig`, `CtsReport`,
`run_cts` and the tests; `tree.rs` holds the constructor.

### 2.1 Clock-root identification (reuses the SDC clock model)

`clock_roots(db, sdc)` resolves each `create_clock` with `port.is_some()`
(design clocks; virtual I/O clocks have no tree) to a root net: the io pin
named `Clock::port` → `pin_nets` → `net_driver`. Same resolution
`sta/analysis.rs` performs when seeding the propagated-clock walk; no second
SDC parse. Two clock rows on the same port deduplicate to one tree. Loud
bails on: missing port, unconnected port pin, driverless net, multi-driver
net, zero resolved roots. A cell-driven root net (internal/generated clock)
is accepted as-is — the tree hangs off the cell's output.

### 2.2 Sink collection

Every non-driver pin on the root net is a sink, snapshotted before any
mutation (never iterate a CSR segment while mutating — the insert-api
lesson). On the committed designs that is exactly the register `CLK` pins
(gcd 35, picorv32 1,597).

### 2.3 Partition — count-equalized median bisection

`partition(loads, max_loads)` with `max_loads = 8` (the TritonCTS load band:
35/5 ≈ 7, 1597/197 ≈ 8.1 on the references):

- `g = ceil(n / max_loads)`; `g <= 1` returns one group.
- Else sort by the coordinate of the bbox's **longer axis** (ties: other
  axis, then `PinId` — fully deterministic) and split so the left
  sub-problem owns `floor(n·⌊g/2⌋/g)` loads; recurse. Result: exactly `g`
  groups, sizes within ±1 pin, each spatially coherent.

Every sink descends exactly `tree_depth` buffers — depth balance by
construction, which is the property that bounds skew (a PD/RSMT backbone
optimizes wirelength instead; rejected for this stage).

### 2.4 Level construction (bottom-up)

```
loads := sinks; loop:
    groups := partition(loads, max_loads)
    for g in groups:
        b := insert_cell("clkbuf_<ctr>_<root>", pick_buffer(g)|root_buf if top)
        m := create_net("clknet_<ctr>_<root>")
        connect_pin(b.out, m)     # connect_pin keeps drivers at index 0
        connect_pin(p, m) for p in g   # auto-disconnects from the old net
        seed b at centroid(g); status stays Unplaced
    if one buffer: connect_pin(b.in, root_net); break   # the tree root
    loads := buffer input pins at their seed positions
```

The fanout uses the mutation primitives directly — `insert_buffer_on_net`'s
all-sinks move cannot express a k-way group split. Naming follows TritonCTS
(`clkbuf_<n>_<root>`, `clknet_<n>_<root>`); a collision on re-run fails
loudly in `insert_cell`/`create_net` — the honest failure for running CTS
on an already-buffered design.

### 2.5 Buffer sizing

`pick_buffer` sums the group's sink `cap_nom`s plus `c_pf_per_um ×` group
bbox half-perimeter (met2 RC, the STA placement layer) and picks the first
`buf_list` entry (`clkbuf_8` → `clkbuf_4` → `clkbuf_2`, strongest→weakest)
whose output-pin Liberty `max_cap` covers the estimate; falls back to the
strongest entry when nothing covers it. The top level always takes
`root_buf` (`clkbuf_16`). Measured outcome: every group lands under
`clkbuf_8`'s 0.909 pF `max_cap`, so all non-root buffers are `clkbuf_8` —
TritonCTS's own census is also dominated by `clkbuf_16`/`clkbuf_8`.

### 2.6 Pin-and-place legalization

`legalize_inserted_only` temporarily marks every non-inserted
`Placed`/`Unplaced` cell `Fixed` (any macro class — `Fixed`/`Cover` are the
only statuses Abacus buckets as row obstacles), seeds `gp_x/gp_y` from
current positions, runs `legalize_placement`, then restores original
statuses even on failure. Only inserted buffers move; pre-existing
component/net lines in the emitted DEF stay byte-identical. OpenROAD's own
CTS displaces neighbors freely — pin-and-place is the acceptance's literal
requirement, not a quality claim; a design too dense for a buffer's group
centroid pushes it to the nearest free gap (named divergence from
TritonCTS's neighborhood shifts).

`legalize_placement` self-verifies under the pinned statuses; `run_cts`
re-runs `verify_legality` on the restored state before reporting.

### 2.7 STA surface change

`sta/analysis.rs`: `run_sta_with_state` is now `pub(crate)` (still
`#[cfg(test)]`) and `PinTimingState.arrival_max`/`arrival_min` are
`pub(crate)` so the cts tests can read propagated clock-pin arrivals on a
re-parsed CTS DEF. No production surface changes.

---

## 3. Report schema (`CtsReport`, `--report <json>`)

Field names mirror `<d>.cts.reference.json` wherever a direct analogue
exists:

| Ours | Reference field | Notes |
|---|---|---|
| `design`, `clock` | same | `clock` comma-joins when several roots build |
| `cts_root_buf`, `cts_buf_list` | same | echo of config |
| `inserted_instances` | same | cells added — tree buffers only; the reference's count includes its dummy loads |
| `tree_buffers` | same | census by macro name |
| `dummy_loads` | same | always `{}` — `repair_clock_nets` is deferred work |
| `clock_subnets` | same | `clknet_*` nets created = `tree_buffers` total (same relation as the reference: subnets count only net-driving buffers) |
| `sinks` | same | non-driver pins on the clock root nets |
| `tree_depth` | same | buffers on the longest root→sink path (the `CTS-0102` convention) |
| `avg_sink_wirelength_um` | same | **different quantity**: ours = mean sink→leaf-buffer-output Manhattan distance; the reference is TritonCTS's internal `CTS-0101` "Average sink wire length" (120.66/756.54 µm vs our 18.3/16.7 µm). Recorded definition — the e2e ticket aligns the metric before comparing |
| `clock_net_wirelength_um` + `*_basis` | same | Σ per-net HPWL over the tree's nets (root + `clknet_*`), `pin_pos_dbu` bboxes — the same set the reference census HPWL'd |
| `def_components`, `def_signal_nets` | same | `num_cells()`/`num_nets()` post-mutation |
| — | `cts_wire_unit`, `skew_*_ns`, `latency_*_ns` | deliberately absent: outputs of the *reference* DEF, arriving with `e5-cts-e2e` |

---

## 4. Tests

`cts::tests` (`mod.rs`) + `main.rs` CLI test:

| Test | Covers |
|---|---|
| `test_clock_roots_gcd` | T1: one `core_clk` root on `clk`, `PIN clk` driver, 35 `_*_ CLK` sinks |
| `test_cts_tree_structure_gcd` | T2: clk segment `{PIN clk, root buf A}`; every `clknet_*` driver-first by an inserted `X` pin; all 35 sinks on leaf nets; exact depth balance |
| `test_cts_def_roundtrip_{gcd,picorv32}` | T3: emitted DEF re-parses, `verify_legality` passes in-memory and on re-read, every pre-existing cell keeps status + position |
| `test_cts_untouched_def_identical` | T4: PINS block, all component lines, all non-`clk` net lines byte-identical; control net `ctrl.state.out[1]` member positions + HPWL unchanged |
| `test_cts_propagated_sta_{gcd,picorv32}` | T5: propagated STA on the re-parsed CTS DEF; every original sink keeps a propagated arrival; gcd spread soft-bounded (≤ 0.5 ns — the "tens-of-ps band" is informational) |
| `test_cts_census_picorv32` | T6: 1,597 sinks, depth 3–6, census 100–450 (the ~200-buffer league) |
| `test_cts_root_errors` | T7: same-port dedup; missing port errors without mutating; virtual-only `Sdc` (hand-built — `parse_sdc` rejects design-clock-free files) errors |
| `cli_cts_parse_and_gcd_e2e` | parse: missing any of `--tlef/--lef/--lib/--def/--sdc/--def-out` errors; e2e: gcd run emits DEF + report JSON, re-parse + legality pass |

**Fail-proof** (recorded in `docs/results.md`): collapsing `partition` to a
single group turns `test_cts_tree_structure_gcd` and
`test_cts_census_picorv32` red (`depth 1`); restored green. A one-group-per-
sink mutation was also tried first — it loops the level construction
forever (group count never shrinks), which is itself the constructor's
load-bearing property.

---

## 5. Known gaps / limitations

- `dummy_loads` always `{}` (`repair_clock_nets` deferred) — the
  reference's `inserted_instances` is therefore larger than its
  `clock_subnets`; ours are equal.
- `avg_sink_wirelength_um` is a recorded proxy, not TritonCTS's `CTS-0101`
  quantity (see schema table).
- Inserted buffers seed `Unplaced` and legalize into free gaps;
  TritonCTS's tree placement optimizes for trunk topology we do not model —
  our clock-net HPWL is consequently higher (428 vs 370 µm gcd; 16,261 vs
  12,749 µm picorv32) and insertion delay comparable but not identical
  (0.306 vs 0.314 ns gcd).
- A clock-as-data tap or pre-existing buffer input on the root net would be
  treated as a sink — legal but not skew-modelled (named, unexercised on
  the committed designs).
- Cell-driven root nets (internal/generated clocks) are accepted by
  `clock_roots` but untested — no fixture (tests.md T7 names this
  reviewed-only). Multi-root runs across distinct nets are likewise
  unexercised; the committed SDCs carry one design clock each.
- picorv32's negative setup WNS (−1.77 ns) matches the reference's own
  (−1.78 ns on its `cts_placement_rc` golden) — a design property, not a
  CTS regression.
