# ux-sta-summary — Documentation updates

## `docs/e9-reporting.md`

The file is written ticket by ticket; append two sections after the
`critical_paths` material, renumbering as needed:

### `domains` JSON schema

```jsonc
"domains": [
  {
    "clock": "cka",               // create_clock name; null = unattributed row
    "endpoints": 4,               // endpoints whose reported check anchors here
    "wns_max_ns": -0.123456,      // min(worst_max_slack_ns) over the row's subset
    "tns_max_ns": -1.234567,      // Σ min(0, worst_max_slack_ns)
    "wns_min_ns": 0.012345,       // same folds, min corner
    "tns_min_ns": 0.0,
    "setup_violations": 1,        // worst_max_slack_ns < 0
    "hold_violations": 0          // worst_min_slack_ns < 0
  }
]
```

- One row per `sdc.clocks` entry in file order (virtual `create_clock`s
  included; a zero-capture domain emits `endpoints: 0`, `wns: null`,
  `tns: 0`, `violations: 0`), then a `clock: null` row iff any endpoint
  is unattributed.
- Attribution: the `Sdc::clocks` row of the endpoint's *reported*
  check — `clock_domain[clk]` for register/gate checks, the
  last-producing `-clock` anchor for output ports (the same
  last-group-wins rule `EndpointReport` documents for the reported row,
  `t2_clkcor2`), delay-rewritten checks keep the replaced check's
  anchor, leaf/`req = N` checks and unclocked captures are unattributed.
- Under `--lib-hold` the merged rows are recomputed from the merged
  `endpoints` with setup's clock names (same SDC row order).

### `slack_histogram` JSON schema

```jsonc
"slack_histogram": [
  { "bucket": "negative",  "count": 1 },   // s < 0 — equals setup_violations
  { "bucket": "p0_to_20",  "count": 3 },   // 0 <= s < 0.2*P
  { "bucket": "p20_to_50", "count": 2 },   // 0.2*P <= s <= 0.5*P
  { "bucket": "over_50",   "count": 2 },   // s > 0.5*P
  { "bucket": "no_period", "count": 0 }    // no period reference or no max check
]
```

- `s` = `worst_max_slack_ns`, `P` = the endpoint's *capture* period.
  All five buckets always present, in this order; counts sum to
  `endpoints.len()`.
- Note for JSON consumers: `worst_*` is crate-internal; `max_slack_ns`
  reproduces every bucket except multi-anchor ports (one endpoint may
  shift between `negative`/positive buckets).
- `pulse_width_checks` never enter `endpoints` and are in neither block
  — same exclusion as the global wns/tns.

### Stdout

Show the printed table + histogram example (same block as `api.md` §6).

## `docs/status.md`

E9 row: move items (2) and (3) to DONE with `v0.6.1` and
`ux-sta-summary`; evidence line: `sta_clocks` `two_clock.def` +
`t2_selmc.sdc` — two populated domain rows with pin-set membership
asserted, partition algebra (Σ endpoints, min wns, Σ tns, Σ violations)
and 5-bucket histogram recomputed from `endpoints`; merge + partitioned
fixtures covered; fail-proof recorded. Items (4),(5) stay OPEN
(`ux-top-sdc`).

## `docs/results.md`

New dated entry `ux-sta-summary`, appended chronologically at the end of
the file (entries are date-ordered, newest last), recording:

- Fixture run numbers: `two_clock.def` + `t2_selmc.sdc` — the actual
  domain rows and bucket counts verbatim, and whether an unattributed
  row appeared.
- The multi-anchor port rule chosen (last-producing anchor, max
  preferred) and the merge rule.
- Partitioned-fixture consistency run (`md_basic.sdc`) outcome.
- Fail-proof: corruption applied, the red assertion output pasted,
  restore confirmed — plus the note that the consistency test stays
  green under mis-attribution by design (§1 hardcodes membership).
- CLI stdout block verbatim + `--json` key check.
- Version: `0.6.0 -> 0.6.1`, patch — additive JSON fields and stdout
  text, no new flag or breaking change (per `api.md` §7).
- Checks run: `cargo test --workspace`, `cargo clippy --workspace
  --all-targets -- -D warnings` (+ `cargo test --release -p grus-core
  sta` if the workspace run leaves `sta` paths cold — match what the
  ux-sta-paths entry recorded).

## `docs/README.md`

No change needed — `e9-reporting.md` is already linked (line 11) with no
per-ticket description.
