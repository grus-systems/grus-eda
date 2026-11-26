# e5-cts-tree — Report Schema

`--report <json>` emits one JSON object per run, `serde_json::to_string_pretty`
on `CtsReport`. Field names mirror `<d>.cts.reference.json` wherever a
direct analogue exists, so `docs/results.md` can diff the two censuses.

```json
{
  "design": "gcd",
  "clock": "core_clk",
  "cts_root_buf": "sky130_fd_sc_hd__clkbuf_16",
  "cts_buf_list": "sky130_fd_sc_hd__clkbuf_8,sky130_fd_sc_hd__clkbuf_4,sky130_fd_sc_hd__clkbuf_2",
  "inserted_instances": 6,
  "tree_buffers": {
    "sky130_fd_sc_hd__clkbuf_16": 1,
    "sky130_fd_sc_hd__clkbuf_8": 5
  },
  "dummy_loads": {},
  "clock_subnets": 6,
  "sinks": 35,
  "tree_depth": 2,
  "avg_sink_wirelength_um": 000.000,
  "clock_net_wirelength_um": 000.000,
  "clock_net_wirelength_basis": "hpwl",
  "def_components": 390,
  "def_signal_nets": 274
}
```

Field mapping vs `gcd.cts.reference.json`:

| ours | reference | same definition? |
|---|---|---|
| `design` | `design` | yes |
| `clock` | `clock` | yes — comma-joined if >1 design clock |
| `cts_root_buf` | `cts_root_buf` | yes |
| `cts_buf_list` | `cts_buf_list` | yes |
| `inserted_instances` | `inserted_instances` | yes — counts every cell CTS added |
| `tree_buffers` | `tree_buffers` | yes — census by macro, `BTreeMap` order |
| `dummy_loads` | `dummy_loads` | yes — ours is `{}` until repair lands |
| `clock_subnets` | `clock_subnets` | yes — `num_nets` gain on clock trees |
| `sinks` | `sinks` | yes — non-driver pins on root nets |
| `tree_depth` | `tree_depth` | yes — max buffer count root→sink |
| `avg_sink_wirelength_um` | `avg_sink_wirelength_um` | same quantity class; ours = mean sink→driver-pin Manhattan distance |
| `clock_net_wirelength_um` | `clock_net_wirelength_um` | yes — Σ HPWL over tree nets |
| `clock_net_wirelength_basis` | `clock_net_wirelength_basis` | fixed `"hpwl"` |
| `def_components` | `def_components` | yes — `num_cells` after insertion |
| `def_signal_nets` | `def_signal_nets` | yes — `num_nets` after mutation |

Omitted from the reference schema, with the omission recorded in
`docs/e5-cts.md`: `cts_wire_unit` (no `-cts_wire_unit` concept — the
sizing wire-cap uses `met2` RC), `skew_setup_ns`, `skew_hold_ns`,
`latency_max_ns`, `latency_min_ns` (STA outputs of the *reference* DEF;
ours arrive in ticket 4's e2e run — the cts tests print them for the
`results.md` table instead of baking them into the report schema).

`tree_buffers`/`dummy_loads` values are `BTreeMap<String, usize>` —
deterministic key order, census-style like the reference's two-level
`{"macro": n}` objects.

`docs/results.md` gets the ours-vs-reference census table for both
designs beside the 2026-11-22 `e5-cts-ref` entry, per criterion 4.
