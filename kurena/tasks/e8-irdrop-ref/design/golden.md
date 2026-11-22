# e8-irdrop-ref — Golden artifacts

## `<d>.irdrop.reference.json`

One object per design, emitted by the step-D python heredoc from the
two transcripts + the CSV files. Every metric value comes from the
tool's own output; the assembler only parses and formats (the
`guide_caps.py` precedent — nothing hand-written or rounded).

```json
{
  "design": "gcd",
  "input_def": "gcd.placed.def",
  "input_sdc": "gcd.sdc",
  "image": "ghcr.io/librelane/librelane:3.0.16",
  "convention_mirrored": "sta_per_instance",
  "conventions": ["embedded", "sta_per_instance"],
  "inst_power_feed": {
    "source": "report_power -instances [get_cells *] -digits 12",
    "def_components": 384,
    "inst_count": 384,
    "report_rows": 384,
    "applied": 384
  },
  "nets": {
    "VPWR": {
      "use": "POWER",
      "connectivity": "All shapes on net VPWR are connected.",
      "conventions": {
        "embedded":  { ... },
        "sta_per_instance": { ... }
      },
      "delta": { ... },
      "conventions_share_voltage_map": true
    },
    "VGND": { ... }
  }
}
```

### Per-net convention block

Each `conventions.<conv>` block carries the fields the step-B/D parser
pulls out of the `analyze_power_grid` report, plus the CSV record:

| Field | Source | Notes |
|---|---|---|
| `total_power_w` | analyze report total-power line | If the report grammar carries no total-power line, use the same-process `report_power` design total and set `total_power_source` accordingly — record, never guess. |
| `total_power_source` | `"analyze_report"` or `"report_power"` | Names which tool surface produced `total_power_w`. |
| `supply_voltage_v` | analyze report supply/worstcase-voltage line | Verbatim reported value (P-IRD: 1.80 V). |
| `average_voltage_v` | analyze report | |
| `average_drop_v` | analyze report | Signed/absolute as reported; `drop_semantics` carries the meaning. |
| `worst_drop_v` | analyze report | The criterion-1 number (P-IRD VPWR embedded: 5.91e-04). |
| `drop_percent` | analyze report | P-IRD printed "Percentage of IR drop: 0.03 %". |
| `drop_semantics` | report wording | `"drop"` or `"bounce"`. Derived from the report's own labels — for `VGND` the run must record which the tool prints (a rise above the 0 V rail or a drop toward it); an unrecognized wording aborts the parse rather than being mapped silently. |
| `report_lines` | analyze report | The full verbatim metric lines of the report block, as a string array — evidence for `drop_semantics` and for later grammar changes. |
| `csv_file` | step-D naming | Repo-relative file name, or `null` when the file is gitignored. |
| `csv_rows` | CSV line count − header | The "analyzed count" for the net. |
| `csv_size_bytes` | file stat | |
| `csv_sha256_16` | sha256 | First 16 hex chars. |

### Per-net `delta` block

Measured (computed by the assembler from the two parsed blocks — both
sides are tool numbers, so this is arithmetic on measurements, not a
derived reference):

- `worst_drop_v`, `average_drop_v`, `total_power_w`,
  `average_voltage_v`: `sta_per_instance − embedded`.
- `voltage_maps_identical`: byte-equality of the two CSVs.

### Top-level fields

- `convention_mirrored`: the convention the E8 engine will mirror —
  expected `"sta_per_instance"` (our N7 `power` emits per-instance
  power; see `conventions.md`). Written by the assembler as a fixed
  string, and justified in `docs/results.md` with the measured delta.
- `inst_power_feed`: `def_components` (odb insts), `inst_count`
  (`llength [get_cells *]` — the list fed to `-instances`), the parsed
  `report_rows`, and `applied` — the number of
  `set_pdnsim_inst_power` lines emitted. `report_rows == inst_count`
  and `applied == report_rows` are asserted; `inst_count` vs
  `def_components` records how many physical cells the feed skipped
  (see `script.md` step B.4 and the A2 contingency).

## `<d>.irdrop.voltage*.csv`

The `analyze_power_grid -voltage_file` outputs — the criterion-2
per-instance voltage map. Header shape (P-IRD):
`Instance,Terminal,Layer,X location,Y location,Voltage`.

Naming (step D decides after comparing conventions):

| Situation | Files |
|---|---|
| embedded == sta CSV (byte-identical) | `<d>.irdrop.voltage.<net>.csv` (one file, `conventions_share_voltage_map: true`) |
| they differ | `<d>.irdrop.voltage.<net>.embedded.csv` + `<d>.irdrop.voltage.<net>.sta.csv` |

`<net>` is lowercase (`vpwr`, `vgnd`). For gcd that yields 2–4 files,
each ~384 rows — far under 1 MB, all committed.

Commit rule (`AGENTS.md` §2): commit when < 1 MB; otherwise gitignore
and record `csv_file: null` + `csv_size_bytes` + `csv_sha256_16` in the
JSON and in `docs/results.md`. picorv32 (10,205 components) is the
likely gitignore case for its CSVs — decide by measured size, and add
the exact patterns to `.gitignore` (e.g.
`tests/designs/picorv32/picorv32.irdrop.voltage.*.csv`); the
`.reference.json` commits regardless, following the
`picorv32.cts.reference.json` precedent.

## What stays out

- No `<d>.irdrop.sdc` — `gcd.sdc`/`picorv32.sdc` are read unchanged.
- No SPEF/DEF outputs — `analyze_power_grid` mutates nothing the
  pipeline consumes.
- `runs/irdrop/` scratch (transcripts, `inst_power.rpt`,
  `inst_power.tcl`, intermediate CSVs) is gitignored by the existing
  `tests/designs/<d>/runs` rules and is not committed.
