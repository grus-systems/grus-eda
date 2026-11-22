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
  "magic_total": 0,
  "magic_per_rule": {},
  "magic_listall_why_count": 0,
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
  "antenna_per_net": [],
  "antenna_command": "check_antennas -verbose",
  "antenna_command_surface": ["..."],
  "antenna_verbose_flag": "accepted | rejected",
  "sanity_control": {
    "mutation": {
      "name": "add_spacing_probe_wire",
      "statement": "+ NEW",
      "net_a": "<host net>",
      "net_b": "<wire-added net>",
      "layer": "met2",
      "gap_um": 0.03,
      "segment_dbu": [x1, y, x2, y],
      "layer_width_dbu": 140,
      "layer_min_spacing_dbu": 140
    },
    "broken_def": "tests/designs/gcd/runs/drc/gcd.drc.broken.def",
    "broken_def_sha256_16": "...",
    "magic_total": 0,
    "magic_per_rule": {"<verbatim rule name>": 0},
    "violated_rule_names": ["<verbatim rule name>"],
    "violated_rules_in_checked_deck": true
  },
  "runtime_s": {"antenna": 0, "drt_regen": 0, "magic_clean": 0, "magic_broken": 0, "assembly": 0}
}
```

### Field notes

- `input_def_provenance`: `"explicit arg"` when `$2` was a non-default
  path, `"e6-drt-ref baseline"` for `*.drt.routed.def`, else
  `"generate_ref.sh step 5b"`.
- `magic_per_rule`: `{verbatim why-string: count}` from
  `DRCREF-WHY` lines grouped — `{}` on a clean DEF (the empty map is
  the truthful breakdown; the broken run exercises the nonzero path).
  `magic_listall_why_count` is the raw item count; assert
  `magic_total` (from `drc count total`) equals it — disagreement is a
  surface finding, record + abort.
- `magic_rules_checked.names`: every rule name extracted from the tech
  file `drc` section (extraction grammar pinned at implementation —
  likely the quoted `why` strings on `spacing`/`edge`/`edge4way`/
  `width`/`area`/etc. statements). `count` asserts nonempty.
- `drt_drc_count`: `wc -l` of the report file (0 when the route is
  clean, matching the step-5b `-s` convention). `drt_report_source`
  records which path produced it; `drt_reference_json_violations`
  cross-reads `e6`'s `<d>.drt.reference.json` when present (`null`
  otherwise) — a divergence is recorded, never reconciled silently.
- `antenna_per_net`: verbatim per-net violation lines when
  `-verbose` prints them; `[]` when zero. If the command's output
  grammar yields only a total, `antenna_per_net` stays `[]` and
  `antenna_violations` comes from the parsed total — the
  `antenna_command`/`verbose_flag` fields record exactly which surface
  produced the number.
- `sanity_control.violated_rules_in_checked_deck`: each verbatim
  `violated_rule_names` entry looked up in
  `magic_rules_checked.names` — `true` when all intersect (ties the
  sanity evidence to the taxonomy), `false` if Magic reports a name the
  tech-file extraction missed (then the extraction grammar is
  incomplete — record + flag in `docs/results.md`).
- `runtime_s`: per-step wall seconds — the named varying metadata
  field, excluded from the byte-repro expectation (`docs/results.md`
  names it).

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
