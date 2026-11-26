# e8-solve — Output contract

## Voltage CSV

Every `+ USE POWER`/`GROUND` mesh produces one CSV under `--out-dir`:

```text
<design_name>.irdrop.voltage.ours.<net lowercase>.csv
```

For the committed inputs the exact outputs are:

- `gcd.irdrop.voltage.ours.vpwr.csv`
- `gcd.irdrop.voltage.ours.vgnd.csv`
- `picorv32.irdrop.voltage.ours.vpwr.csv`
- `picorv32.irdrop.voltage.ours.vgnd.csv`

Header, byte-for-byte:

```text
Instance,Terminal,Layer,X location,Y location,Voltage
```

Row order is `mesh.sinks` order. It is not resorted. On the committed
designs that order is DEF component order because `AllInstances` expands
by `DesignDb` cell order and `build_mesh` takes distinct attached pins in
first-appearance order. A test compares the full ordered
`(Instance, Terminal)` sequence, not just a set or count.

## Field rules

| column | rule |
|---|---|
| `Instance` | resolved `cell_names[pin_cells[pin]]` |
| `Terminal` | resolved `pin_names[pin]` |
| `Layer` | `tech_db.routing_layers[0].name` — the measured PDNSim convention (`li1` for sky130) |
| `X location` | `pin_pos_dbu(pin).0 / dbu_per_micron`, `{:.4}` |
| `Y location` | `pin_pos_dbu(pin).1 / dbu_per_micron`, `{:.4}` |
| `Voltage` | `node_voltages_v[sink.node]`, `{:.6}` |

The `Layer` choice is intentional: PDNSim labels every committed row
`li1` although sky130 VPWR/VGND macro-port geometry is `met1` and the
mesh sink node is `met1`. Printing `mesh.nodes[node].layer` would say
`met1` and fail a byte-level first-five-column comparison. The mesh still
models the true attach layer; the CSV's first routing layer is a
reference output convention.

Formatting is fixed rather than locale-dependent:

- coordinates always four fractional digits;
- voltage always six fractional digits;
- `-0.000000` is normalized to `0.000000`;
- LF line endings;
- no trailing spaces;
- instance/terminal fields are written raw unless a general CSV escape
  is required (`,`/`"`/newline), in which case standard double-quote CSV
  escaping is used. The committed names need no escaping.

## Golden row-set check

Tests generate rows before writing and compare to the embedded golden
CSV:

- exact header equality;
- exact row count;
- exact ordered `(Instance, Terminal)` list;
- exact `Layer`, `X location`, `Y location` fields per row;
- only `Voltage` is allowed to differ.

This is stronger than the acceptance's "same row set": it also catches
ordering and presentation drift. A per-element diff reports the first
mismatched row index and both values.

## Summary JSON

The CLI always writes one sidecar:

```text
<design_name>.irdrop.summary.ours.json
```

Schema follows `IrDropReport` in `api.md`. Required content:

- design name;
- `current_scale_v` and estimator total power;
- per net: CSV file name, row count, elapsed solve time;
- mesh counts (`nodes`, `source_nodes`, `resistors`, `sinks`);
- source clamp voltage and `max_source_error_v`;
- injected power, uninjected power, total current;
- average/worst voltage and positive drop/bounce magnitude;
- `drop_semantics`;
- solver method, iterations (`0` for direct), absolute and normalized
  residual;
- embedded reference scalars and `ours - reference` deltas when a
  reference was supplied.

The JSON is pretty-printed and deterministic: `nets` follows DEF
`pdn_nets` order, and no wall-clock timestamp is included.

## Console output

Per net the CLI prints one summary line containing:

```text
irdrop <NET>: nodes=<n> sources=<n> sinks=<n> current=<a> A
  ref_current=<a> A delta=<a> A avg_<drop|bounce>=<v> V
  worst_<drop|bounce>=<v> V residual=<a> A
  method=nalgebra-sparse-CscCholesky iterations=0
  source_max_error=<v> V elapsed=<ms> ms
```

Reference/delta fields are omitted only if a future non-reference mode is
added; the E8 CLI requires `--reference`, so they are present here.

## Files

- `crates/grus-core/src/pdn/solve.rs`
- `crates/grus-eda/src/main.rs`
- `tests/designs/gcd/gcd.irdrop.voltage.ours.vpwr.csv`
- `tests/designs/gcd/gcd.irdrop.voltage.ours.vgnd.csv`
- `tests/designs/gcd/gcd.irdrop.summary.ours.json`
- `tests/designs/picorv32/picorv32.irdrop.voltage.ours.vpwr.csv`
- `tests/designs/picorv32/picorv32.irdrop.voltage.ours.vgnd.csv`
- `tests/designs/picorv32/picorv32.irdrop.summary.ours.json`
