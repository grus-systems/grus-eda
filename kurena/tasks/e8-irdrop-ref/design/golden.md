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
  "convention_mirrored": "embedded",
  "conventions": ["embedded", "sta_per_instance"],
  "inst_power_feed": {
    "source": "report_power -instances [get_cells *] -digits 12",
    "def_components": 384,
    "inst_count": 384,
    "report_rows": 384,
    "applied": 384,
    "design_report_total_w": 1.036152e-3,
    "inst_feed_total_w": 1.0361523807115639e-3
  },
  "nets": {
    "VPWR": {
      "use": "POWER",
      "connectivity": "All shapes on net VPWR are connected.",
      "connectivity_code": 0,
      "connectivity_result": "1",
      "conventions": {
        "embedded":  { ... },
        "sta_per_instance": { ... }
      },
      "zeroed_probe": { ... },
      "delta": { ... },
      "conventions_share_voltage_map": false
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
| `total_power_w` | analyze report `Total power` line | The report carries this line in the pinned image, so `total_power_source` is `"analyze_report"`. Note (measured): with `set_pdnsim_inst_power` applied the line reports the *annotated* sum, not the solved current — see `zeroed_probe`. |
| `total_power_source` | `"analyze_report"` or `"report_power"` | Names which tool surface produced `total_power_w`. |
| `supply_voltage_v` | analyze report `Supply voltage` | Verbatim reported value (P-IRD: 1.80 V; VGND: 0.00 V). |
| `worstcase_voltage_v` | analyze report `Worstcase voltage` | The extreme node voltage (VPWR embedded: 1.80 V at print precision; VGND: 5.59e-04 V). |
| `average_voltage_v` | analyze report `Average voltage` | |
| `average_drop_v` | analyze report `Average IR drop` | Signed/absolute as reported; `drop_semantics` carries the meaning. |
| `worst_drop_v` | analyze report `Worstcase IR drop` | The criterion-1 number (P-IRD VPWR embedded: 5.91e-04). |
| `drop_percent` | analyze report `Percentage drop` | P-IRD prints `Percentage drop  : 0.03 %`. |
| `report_wording` | deviation label in the report | `"drop"` or `"bounce"` — the word inside the `Average/Worstcase IR <word>` labels. The pinned image prints `IR drop` for **both** rails; anything else aborts the parse rather than being mapped silently. |
| `drop_semantics` | derived: net `use` + deviation sign | `"drop"` for a POWER rail (deviation falls below supply), `"bounce"` for a GROUND rail (deviation rises above the 0 V rail). The sign check aborts if the physics does not match the rail's `USE`. |
| `corner` | analyze report `Corner` | `"default"` in the pinned image. |
| `report_lines` | analyze report | The full verbatim metric lines of the report block, as a string array — evidence for the wording/semantics fields and for later grammar changes. |
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

### Per-net `zeroed_probe` block (added during implementation)

The step-C2 third process annotates every instance `-power 0.0` and
re-measures both nets. Same metric fields as a convention block plus:

- `voltage_map_identical_to_embedded`: byte-equality of the zeroed CSV
  vs the embedded CSV (the zeroed CSV itself is not committed — it is
  redundant once equality is recorded).
- `csv_rows`, `csv_sha256_16`: for the record.

Measured purpose (see `conventions.md`): `total_power_w` 0.00 while
`worst_drop_v` equals embedded and the map is byte-identical proves
`set_pdnsim_inst_power` is additive in this image — the report's Total
line shows the annotated sum while the solve keeps OpenSTA's computed
power plus the annotation.

### Top-level fields

- `convention_mirrored`: the convention the E8 engine will mirror —
  `"embedded"` (the additive-annotation measurement rules the explicit
  feed out as a substitution path; see `conventions.md` "Measured
  outcome"). Written by the assembler as a fixed string, and justified
  in `docs/results.md` with the measured delta.
- `inst_power_feed`: `def_components` (odb insts), `inst_count`
  (`llength [get_cells *]` — the list fed to `-instances`), the parsed
  `report_rows`, `applied` — the number of `set_pdnsim_inst_power`
  lines emitted, plus two measured totals: `design_report_total_w` (the
  standalone `report_power` Total row) and `inst_feed_total_w` (sum of
  the per-instance Total column — the annotated sum). `report_rows ==
  inst_count` and `applied == report_rows` are asserted; `inst_count`
  vs `def_components` records how many physical cells the feed skipped
  (see `script.md` step B.4 and the A2 contingency — on both designs
  the report covers every instance, physical cells included, at 0 W).

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
JSON and in `docs/results.md`. picorv32 (10,205 components) was the
likely gitignore case; measured 2026-11-23 its four CSVs are ~503 KB
each, so all of them commit (the `.reference.json` commits regardless,
following the `picorv32.cts.reference.json` precedent).

## What stays out

- No `<d>.irdrop.sdc` — `gcd.sdc`/`picorv32.sdc` are read unchanged.
- No SPEF/DEF outputs — `analyze_power_grid` mutates nothing the
  pipeline consumes.
- `runs/irdrop/` scratch (transcripts, `inst_power.rpt`,
  `inst_power.tcl`, intermediate CSVs including the `*.zeroed.csv`
  probe maps) is gitignored by the existing `tests/designs/<d>/runs`
  rules and is not committed.
