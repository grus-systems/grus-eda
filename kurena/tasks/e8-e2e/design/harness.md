# e8-e2e — Comparison harness

All comparison glue lives in `crates/grus-core/tests/irdrop.rs` as
test-local helpers. Nothing here is a new public API. The helpers read
only committed artifacts and generated artifacts that this ticket makes
the committed script emit.

## Inputs

For each design (`gcd`, `picorv32`):

- `tests/designs/<d>/<d>.placed.def` — picorv32's is gitignored;
  `require()` panics with the producer command when absent.
- `tests/designs/<d>/<d>.sdc`
- `tests/designs/<d>/<d>.irdrop.reference.json`
- `tests/designs/<d>/<d>.irdrop.voltage.<net>.embedded.csv`
- `tests/designs/<d>/<d>.irdrop.voltage.ours.<net>.csv`
- `tests/designs/<d>/<d>.irdrop.inst_power.tcl` — newly committed by
  this ticket's `generate_irdrop_ref.sh` update.
- sky130 Liberty/tech LEF/cell LEF through `PDK_ROOT` or `~/.ciel`.

`nets.<NET>.conventions.embedded.csv_file` supplies the golden CSV
name; do not construct it by convention. The test asserts top-level
`convention_mirrored == "embedded"` and each net's
`drop_semantics` is `"drop"` for POWER or `"bounce"` for GROUND.

## Test-side types

Keep the harness small and explicit:

```rust
struct CsvRow {
    instance: String,
    terminal: String,
    layer: String,
    x_um: f64,
    y_um: f64,
    voltage_v: f64,
}

struct Deviation {
    instance: String,
    terminal: String,
    x_um: f64,
    y_um: f64,
    ours_v: f64,
    reference_v: f64,
    delta_v: f64,      // ours − reference, signed
    tolerance_v: f64,
}

struct MapStats {
    rows: usize,
    max_abs_v: f64,
    mean_abs_v: f64,
    median_abs_v: f64,
    p99_abs_v: f64,
    over_tolerance: usize,
    top5: Vec<Deviation>,
}

struct NetComparison {
    net: String,
    stats: MapStats,
    our_worst_deviation_v: f64,
    ref_worst_deviation_v: f64,
    worst_delta_v: f64,
    avg_delta_v: f64,
    power_delta_w: f64,
}
```

The concrete field set may shrink if the test does not use it; the
output contract is what matters.

## CSV parsing and row-set assertion

Implement a small RFC-style parser for one CSV row: fields split on
unquoted commas, `""` inside a quoted field unescapes to `"`. The
committed maps have no escaped fields, but `write_voltage_csv` can
emit them; the parser should not silently misjoin such a row.

Per file:

1. Header must equal
   `Instance,Terminal,Layer,X location,Y location,Voltage` exactly.
2. Every row must have six fields and numeric X/Y/Voltage.
3. `(Instance, Terminal)` must be unique within each side.
4. The ours and reference key sets must be identical in both
   directions. On mismatch print up to ten keys only-in-ours and
   only-in-reference, then panic.
5. Only after the set passes, join values by key and compare.

The E8 acceptance requires set equality, not ordering. Ordering and
the first five fields are already pinned by `e8-solve`; this harness
uses `(Instance, Terminal)` as its only join key and reports
coordinates from the golden row. If our coordinate differs from the
golden for a joined key, print it in the top-deviation diagnostic but
do not make it part of the voltage criterion.

## Metrics and tolerances

For each joined row:

```text
delta_v      = V_ours − V_reference
abs_delta_v  = |delta_v|
pin_tol_v    = max(0.05 * |V_reference|, 0.001)
```

For each net, compute:

- `rows`
- `over_tolerance = count(abs_delta_v > pin_tol_v)`
- max, mean, median, and P99 of `abs_delta_v`
  (`sorted[ceil(0.99 * n) − 1]`)
- top 5 by `abs_delta_v`; break ties deterministically by
  `(Instance, Terminal)`

For scalar deviations:

```text
POWER:  deviation = supply_voltage_v − V_node    (positive drop)
GROUND: deviation = V_node − supply_voltage_v    (positive bounce)

worst_tol_v = max(0.05 * |reference_worst_drop_v|, 0.005)
worst_delta = our_summary.worst_drop_v − reference.worst_drop_v
avg_delta   = our_summary.average_drop_v − reference.average_drop_v
power_delta = our_summary.injected_power_w − reference.total_power_w
```

The JSON still calls a GROUND value `worst_drop_v`; carry
`drop_semantics` in every printed line so it reads as bounce. The
comparison uses absolute voltages on both nets and positive
drop/bounce scalars for the scalar summary — the two views are
consistent.

## Recomputed ours map

A helper `run_design(design: &str)` performs the same no-annotation
regime as `generate_irdrop_ref.sh` and `e8-solve`:

1. `load_liberty`, `load_tech_db`, `parse_def`, `load_sdc`.
2. `sta_pin_slews(..., parasitics: None)`.
3. `estimate_power(..., VcdActivity::default(), ActivityOverrides::default())`.
4. `cell_power_w[CellId.0] = report.instances[i].total_w`.
5. `current_scale_v = abs(VPWR_supply − VGND_supply)` from the golden
   embedded supplies; fail if not finite/positive.
6. For each `POWER`/`GROUND` `PdnNet`: `build_mesh`, `solve_pdn_net`,
   `voltage_rows`, `write_voltage_csv`.

`write_voltage_csv` on the recomputed rows is compared byte-for-byte
with the committed ours CSV. That check is separate from the golden
value gate: it catches a committed artifact left stale after a solver
or power-model change.

## Print contract

Every gate and diagnostic prints a compact, copyable block per design
and net:

```text
gcd VPWR: rows=384 over_tol=0 max=... median=... p99=...
  drop ours=... ref=... delta=... tol=...
  avg_drop_delta=... power_delta=...
  top5:
    inst terminal x y ours ref delta tol
```

The top-5 fields satisfy the acceptance exactly: instance, terminal,
coordinates, and signed `ΔV`. Do not truncate instance names in the
printout.

## Reference-power parser

`read_inst_power_tcl(path)` accepts exactly the emitted line shape:

```text
set_pdnsim_inst_power -inst {<instance>} -power <watts>
```

Rules:

- one parseable line per DEF component;
- duplicate or unknown instance names are errors;
- every `DesignDb` cell name must appear exactly once;
- non-finite power is an error (signed values are legal);
- sum parsed watts and compare it to
  `inst_power_feed.inst_feed_total_w` within `1e-12 W`;
- map by resolved `cell_names` into `Vec<f64>` in `CellId` order.

The emitted file's brace syntax is intentionally reused rather than a
new ad-hoc CSV: it is the exact feed the reference script sources.

## Files

- `crates/grus-core/tests/irdrop.rs`
- `tests/designs/gcd/gcd.irdrop.inst_power.tcl`
- `tests/designs/picorv32/picorv32.irdrop.inst_power.tcl`
