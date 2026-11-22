# e6-drt-ref — Golden files and JSON schema

## Artifact naming and commit rules

| artifact | path | committed? |
|---|---|---|
| per-variant JSON | `tests/designs/<d>/<d>.drt.reference.json` | always |
| DRC report | `tests/designs/<d>/<d>.drc.<variant>.rpt` | if < 1 MB, else stays under `runs/drt/` |
| antenna report | `tests/designs/<d>/<d>.antenna.<variant>.rpt` | if < 1 MB, else stays under `runs/drt/` |
| routed DEF | `tests/designs/<d>/<d>.drt.<variant>.def` | never (gitignored; SHA-256 prefix + size in JSON) |
| baseline guide | `tests/designs/<d>/runs/drt/<d>.baseline.guide` | never (under `runs/`; SHA + size in JSON) |
| transcripts | `tests/designs/<d>/runs/drt/{baseline,delegated}.log` | never (under `runs/`) |

`<variant>` ∈ `baseline`, `delegated`.

`.gitignore` additions (one block, matching the existing per-design
style):

```gitignore
# e6-drt-ref routed DEFs (regenerate via tests/designs/generate_drt_ref.sh)
tests/designs/gcd/gcd.drt.*.def
tests/designs/picorv32/picorv32.drt.*.def
```

Note `tests/designs/gcd/gcd.drc.rpt` is already ignored and
`tests/designs/picorv32/picorv32.drc.rpt` is an already-tracked empty
file — the new `<d>.drc.<variant>.rpt` names do not collide with
either.

## `<d>.drt.reference.json` schema

```json
{
  "design": "gcd",
  "input_def": "gcd.placed.def",
  "input_sdc": "gcd.sdc",
  "image": "ghcr.io/librelane/librelane:3.0.16",
  "generated_by": "tests/designs/generate_drt_ref.sh",
  "prior_routed_def": {
    "file": "gcd.routed.def",
    "size_bytes": 225000,
    "sha256_16": "...",
    "baseline_def_identical": true
  },
  "variants": {
    "baseline": { "...": "..." },
    "delegated": { "...": "..." }
  },
  "delta": { "...": "..." }
}
```

`prior_routed_def` is `null` + `prior_routed_def_present: false` when
`<d>.routed.def` does not exist in the worktree (it is gitignored for
picorv32; it is a cross-check, not an input).

### Per-variant block

```json
{
  "flow": "global_route+detailed_route",
  "guide": {
    "file": "runs/drt/gcd.baseline.guide",
    "source": "emitted_by_global_route",
    "size_bytes": 0,
    "sha256_16": "..."
  },
  "signal_nets_in_def": 0,
  "signal_nets_routed": 0,
  "drc_violations": 0,
  "drc_report": {
    "file": "gcd.drc.baseline.rpt",
    "committed": true,
    "size_bytes": 0,
    "sha256_16": "..."
  },
  "antenna_check_command": "check_antennas -report_file",
  "antenna_violations": 0,
  "antenna_report": {
    "file": "gcd.antenna.baseline.rpt",
    "committed": true,
    "size_bytes": 0,
    "sha256_16": "..."
  },
  "antenna_log_lines": ["..."],
  "total_wirelength_um": 0.0,
  "wirelength_source": "routed_def_parse",
  "vias_per_cut_layer": {"mcon": 0, "via": 0, "via2": 0, "via3": 0, "via4": 0},
  "via_count_total": 0,
  "routed_def": {
    "file": "gcd.drt.baseline.def",
    "committed": false,
    "size_bytes": 0,
    "sha256_16": "..."
  },
  "drt_summary_lines": ["..."],
  "timing": {
    "global_route_s": 0.0,
    "detailed_route_s": 0.0,
    "check_antennas_s": 0.0,
    "total_route_s": 0.0
  }
}
```

- `flow` is `"read_guides+detailed_route"` on the delegated variant;
  `guide.file` there is `runs/guide-check/<d>.ours.guide` with
  `source: "grus-eda route --guide-out"` and `guide.sha256_16` of the
  committed-input file.
- `timing.detailed_route_s`/`check_antennas_s` are shared;
  `global_route_s` ↔ `read_guides_s` swap per variant;
  `total_route_s` = the `clock milliseconds` span from first routing
  call to `check_antennas` end (`write_def` is outside — it is an
  output dump, not routing work). All from `DRT-TIME` markers,
  ms → s at 3 decimals.
- `drt_summary_lines` / `antenna_log_lines`: verbatim transcript lines
  (final DRT tally, antenna verdict) — the same `report_lines` idea as
  `generate_irdrop_ref.sh`, so the measured grammar is pinned in the
  golden itself.
- `antenna_check_command` records the actual flag form that ran after
  the surface probe settled (e.g. `-report_file`, `-verbose`, bare).

### `delta` block

`delegated − baseline`, computed in the same python step:

```json
{
  "drc_violations": 0,
  "antenna_violations": 0,
  "total_wirelength_um": 0.0,
  "total_wirelength_pct": 0.0,
  "via_count_total": 0,
  "vias_per_cut_layer": {"mcon": 0, "via": 0},
  "total_route_s": 0.0
}
```

## DEF-derived metrics (step C arithmetic)

The routed DEF is the tool's own emitted artifact; the numbers below
are read off it verbatim — no tool-side estimate is substituted.

- **Wirelength**: inside `NETS`, each `+ ROUTED <layer> ( x y )` chain
  is a list of points with optional `NEW <layer>` segment breaks and
  via names. Sum Manhattan distance between consecutive points that
  continue on the same layer; via tokens and `NEW` breaks contribute
  length 0. `total_wirelength_um` = total DBU / 1000. If a non-Manhattan
  pair ever appears, record the segment verbatim and abort — that is a
  grammar surprise, not data to fold silently.
- **Vias**: a bare identifier token following a point inside a routed
  path is a via-def name (in sky130 DRT output these are the LEF via
  defs `mcon`, `via`, `via2`, `via3`, `via4`, plus any generated DEF
  `VIAS` names). `vias_per_cut_layer` keys on the via-def name as
  written — each sky130 via name denotes exactly one cut layer pair;
  the JSON comment field `via_names_meaning` maps name → cut layer
  (resolved by parsing the `VIA` blocks' `LAYER` lines in the same PDK
  LEF the run read; unknown names are kept under their own key and
  counted — never dropped).
- **Net counts**: `signal_nets_in_def` = non-SPECIAL `NETS` entries in
  `<d>.placed.def`; `signal_nets_routed` = `NETS` entries carrying
  `+ ROUTED` in the routed DEF. A routed count below the placed count
  is expected (port-only nets — picorv32's 67 `irq`/`pcpi_*` nets carry
  no guide block on either side) and is recorded, not warned on.

## Determinism contract

- The whole JSON byte-reproduces on rerun **except** the `timing`
  blocks — `timing` is the named non-reproducing metadata field
  (acceptance line 1). Everything else — counts, SHA-256 prefixes,
  sizes, verbatim log lines — must `git diff` clean on a second run.
- `routed_def.sha256_16` per variant must itself be stable across
  reruns: TritonRoute is deterministic on identical inputs. If the DEF
  SHA varies, that is a named residual recorded in `docs/results.md`
  — never committed as a stable-looking golden.
- `write_def` carries no date field, so no `*DATE`-style
  normalization is needed (contrast `gcd.rcx.spef`).
- No hand-written numbers anywhere: every scalar in the JSON comes out
  of the step-C parser.
