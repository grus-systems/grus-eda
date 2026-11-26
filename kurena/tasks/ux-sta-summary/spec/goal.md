# Goal

E9 ticket 2 (review items 2+3 from
`docs/history/run-2026-11-22-large-design.md`):
per-clock-domain slack summary and a slack
histogram in `sta` output — at-a-glance timing
closure without parsing per-endpoint rows.

## Work

- `StaReport` gains `domains:
  Vec<DomainSummary>`: one row per `create_clock`
  domain present in the run, with `clock`,
  `endpoints` (count of endpoints whose check is
  captured by that domain), `wns_max_ns`,
  `tns_max_ns`, `wns_min_ns`, `tns_min_ns`,
  `setup_violations`, `hold_violations`.
  Domain attribution: each endpoint's capture
  clock is already known to the check machinery —
  reuse it; endpoints not attributable to a named
  domain (unclocked checks, async
  `set_clock_groups`-excluded-but-timed paths)
  land in an explicit `""`/null-domain row, never
  dropped. Record in docs how port checks anchored
  on multiple `-clock`s are attributed (the
  `EndpointReport` comment notes last-group-wins —
  state the rule chosen).
- `slack_histogram: Vec<SlackBucket>`: worst
  max-slack per endpoint bucketed by fraction of
  the relevant clock period — `<0`, `0–20%`,
  `20–50%`, `>50%`, plus a `no_period` bucket for
  endpoints without a period reference. Bucket
  bounds documented; counts must sum to the
  endpoint total.
- Stdout: a compact per-domain table + histogram
  after the existing wns/tns lines (the review's
  "at a glance" requirement); keep it ≤ ~20 lines.
- Verification fixture: `tests/designs/sta_clocks/`
  `t2_selmc.sdc` already exercises two domains
  (`cka` 2.0 ns on `clk1`, `ckb` 1.0 ns on `clk2`)
  on the `sta_clocks` DEF — use it; assert the two
  domain rows exist with correct endpoint counts
  and that domain wns/tns ≤ the global values.
  Also assert histogram bucket counts sum to the
  endpoint count.
- Consistency check: per-domain WNS/TNS must equal
  the same quantity recomputed over that domain's
  endpoint subset — prove with a test that
  recomputes from `report.endpoints`.
- Fail-proof: attribute all endpoints to the first
  domain → test must go red; restore.
- `docs/results.md` + `docs/status.md` E9 row;
  version bump (patch or minor — state which and
  why; JSON schema gains fields but nothing
  breaks).

## Done when

`sta --json` on the two-clock fixture emits correct
per-domain summaries and a histogram whose counts
reproduce from `endpoints`; stdout shows both; the
fail-proof went red and was restored.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-eda/src/main.rs` (stdout rendering)
- `docs/results.md`, `docs/status.md`
