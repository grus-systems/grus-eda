# e7-drc-ref — Golden artifact

## `<d>.drc.reference.json`

One object per design, emitted by the step-E python heredoc from the
three transcripts, the DRT report, the input DEF, and the tech file.
Field names follow the spec (`magic_total`, `magic_per_rule`,
`magic_version`, `drt_drc_count`, `antenna_violations`,
`antenna_per_net`, `input_def_sha_prefix`); the assembler only parses
and formats — classification is committed code, not hand-written
values (the `delta`-arithmetic precedent from the irdrop golden).

```json
{
  "design": "gcd",
  "image": "ghcr.io/librelane/librelane:3.0.16",
  "input_def": "tests/designs/gcd/gcd.routed.def",
  "input_def_sha_prefix": "<sha256 first 16 hex>",
  "input_def_size_bytes": 224344,
  "input_def_provenance": "generate_ref.sh step 5b",
  "magic_version": "8.3.623",
  "magic_version_source": "version_cmd | banner",
  "magic_rcfile": "/pdk/sky130A/libs.tech/magic/sky130A.magicrc",
  "magic_style": "drc(full)",
  "magic_count_flat": 0,
  "magic_total": 0,
  "magic_per_rule": {},
  "magic_listall_items": 0,
  "magic_listall_bbox_total": 0,
  "magic_count_matches_bbox_sum": true,
  "magic_rules_checked": {
    "source": "/pdk/sky130A/libs.tech/magic/sky130A.tech drc section",
    "count": 0,
    "names": ["..."],
    "classification": {"<name>": "lef_expressible|antenna|magic_only|unclassified"},
    "classification_method": "keyword classifier in generate_drc_ref.sh step E; reviewed in docs/e7-drc.md"
  },
  "drt_drc_count": 0,
  "drt_report_file": "tests/designs/gcd/gcd.drc.rpt | runs/drc/gcd.drc.regen.rpt | runs/drt/...",
  "drt_report_source": "existing | regenerated_identical | regenerated_divergent",
  "drt_report_sha256_16": "...",
  "drt_reference_json_violations": null,
  "antenna_violations": 0,
  "antenna_pin_violations": 0,
  "antenna_per_net": [{"net": "...", "pins": ["..."], "violated_layers": ["..."]}],
  "antenna_report_lines": ["..."],
  "antenna_command": "check_antennas -verbose",
  "antenna_command_surface": ["..."],
  "antenna_verbose_flag": "accepted | rejected",
  "sanity_control": {
    "mutation": {
      "name": "add_spacing_probe_wire",
      "statement": "NEW",
      "net_a": "<host net>",
      "net_b": "<wire-added net>",
      "layer": "met2",
      "host_orientation": "vertical | horizontal",
      "host_segment_dbu": [x1, y1, x2, y2],
      "gap_um": 0.03,
      "segment_dbu": [x1, y1, x2, y2],
      "layer_width_dbu": 140,
      "layer_min_spacing_dbu": 140
    },
    "broken_def": "tests/designs/gcd/runs/drc/gcd.drc.broken.def",
    "broken_def_sha256_16": "...",
    "magic_total": 0,
    "magic_per_rule": {"<verbatim rule name>": 0},
    "violated_rule_names": ["<verbatim rule name>"],
    "new_rules_vs_clean": ["<verbatim rule name>"],
    "violated_rules_in_checked_deck": true
  }
}
```

### Field notes

- `input_def_provenance`: `"explicit arg"` when `$2` was a non-default
  path, `"e6-drt-ref baseline"` for `*.drt.routed.def`, else
  `"generate_ref.sh step 5b"`.
- `magic_per_rule`: `{verbatim why-string: bbox-fragment count}` from
  `drc listall why`'s alternating `{why} {bbox-list}` pairs. Measured
  correction vs this design's expectation: **the clean routed DEFs are
  not zero** — the `drc(full)` deck reports nwell violations inside
  the sky130_fd_sc_hd masters (gcd 305 merged regions / 426 bbox
  fragments). Three distinct count surfaces are kept separate:
  `magic_count_flat` (`drc count`, top-level only — 0 on both
  designs), `magic_total` (`drc count total`, hierarchy-wide merged
  regions — the headline), and `magic_listall_bbox_total` (the
  per-rule fragment sum — larger by construction, since `count total`
  merges regions and listall enumerates fragments).
  `magic_listall_items` is the raw alternating-item count;
  `magic_count_matches_bbox_sum` records (never asserts) whether the
  two merged/fragment numbers agree — `drc check` is asynchronous, so
  `drc catchup` precedes the counts (the P-DRC probe's "0 errors" was
  premature counting).
- `magic_rules_checked.names`: every rule name extracted from the tech
  file `drc` section (quoted `why` strings, 278 statements / 239
  unique names measured). Two `rect_only` statements (`*mimcap`,
  `*mimcap2`) carry an unclosed `"` — a sky130A.tech defect; the
  extractor joins continuation lines, works per logical line, and
  takes an odd-quote line's post-quote tail as the name. `count`
  asserts nonempty.
- `drt_drc_count`: `wc -l` of the report file (0 when the route is
  clean, matching the step-5b `-s` convention). `drt_report_source`
  records which path produced it; `drt_reference_json_violations`
  cross-reads `e6`'s `<d>.drt.reference.json` when present (`null`
  otherwise) — a divergence is recorded, never reconciled silently.
- `antenna_per_net`: measured grammar — `check_antennas -verbose`
  prints one `Net: <n>` block per **violating** net (nothing when
  clean), with indented `Pin:` rows and `(VIOLATED)` on the failing
  `Required ratio` line; entries carry `{net, pins, violated_layers}`.
  `antenna_violations` is the `Found N net violations.` count (which
  is also the Tcl return — asserted equal); `antenna_pin_violations`
  is the `Found N pin violations.` count. `antenna_report_lines`
  keeps the `[INFO ANT-*]` lines verbatim. When `-verbose` runs, the
  Net-block count is asserted equal to `antenna_violations`.
- `sanity_control.violated_rules_in_checked_deck`: each verbatim
  `violated_rule_names` entry template-matched against
  `magic_rules_checked.names` — the deck names are printf templates
  (`%d`/`%a` → `.*?` regex), while `listall` renders them — `true`
  when all match (ties the sanity evidence to the taxonomy), `false`
  if Magic reports a name the extraction missed (then the grammar is
  incomplete — record + flag in `docs/results.md`). `new_rules_vs_clean`
  is the set difference, which isolates the mutation's contribution
  from the clean baseline's nwell deck hits.
- Runtime: **not in the golden.** Per-step wall seconds (shell
  `SECONDS`) go to the gitignored `runs/drc/runtime.json` and a
  `DRC-RUNTIME` stdout line, so the committed JSON byte-reproduces
  outright — stronger than the "one varying field" allowance.
  (`docs/results.md` records the measured figures.)

## Scratch (gitignored — not committed)

`tests/designs/<d>/runs/drc/`: `antenna.log`, `magic_clean.log`,
`magic_broken.log`, `<d>.drc.broken.def`, regen artifacts
(`<d>.regen.guide`, `<d>.routed.regen.def`, `<d>.drc.regen.rpt`), any
`.mag` byproducts. The committed JSON's SHAs make these re-verifiable.

## What stays out

- No committed broken DEF or full Magic transcripts — the verbatim
  evidence is in `sanity_control` (names + counts + sha). If a broken
  transcript is ever wanted committed it is < 1 MB and can be added
  then; the `*.drc.rpt` gitignore rules stay untouched.
- No `.mag` files, no routed DEF copies in the design dir.
