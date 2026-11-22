# ux-sta-summary — Test plan

All tests live in `#[cfg(test)]` in `analysis.rs`, next to
`sta_evaluates_two_clock` (~11676), reusing its lib/tech/def/sdc load
pattern (`sky130_hd()`, `design_dir("sta_clocks")`, `run_sta`).

## 1. Fixture: two populated domain rows (`test_domain_summary_two_clock`)

`two_clock.def` + `t2_selmc.sdc` — `cka` (2.0 ns on `clk1`), `ckb`
(1.0 ns on `clk2`); the reference scenario set already exercises this
fixture (`two_clock.reference.json`).

Expected attribution — verify against the run before hardcoding;
from the fixture topology (`u_fa*` on `cka`, `u_fb*` on `ckb`,
`u_fx` launched `cka`→captured `ckb`, `u_fy` launched `ckb`→captured
`cka`, `ya` anchored `cka`, `yb` anchored `ckb`):

- `domains` has exactly 2 rows, `clock == "cka"` and `"ckb"` in that
  order (SDC file order), no `null` row — every endpoint is attributed.
- `cka` members: `u_fa0/D`, `u_fa1/D`, `u_fy/D`, `ya` → `endpoints == 4`.
- `ckb` members: `u_fb0/D`, `u_fb1/D`, `u_fx/D`, `yb` → `endpoints == 4`.
  Assert membership by grouping `report.endpoints` on `domain_row` and
  matching pin sets, not just the counts.
- Bounds vs globals: for each corner
  `domain.wns_* >= report.wns_*` (a subset's min ≥ the global min) and
  `domain.tns_* >= report.tns_*` (a subset of a non-positive sum);
  `min(domains.wns_max) == report.wns_max`.
- If the run does produce an unattributed row (possible if a delay
  endpoint exists in this SDC), assert it explicitly instead —
  `clock.is_none()` + correct member set; record the actual shape in
  `docs/results.md`.

## 2. JSON schema (`test_domain_summary_json_schema`)

`serde_json::to_value(&report)`:

- `domains` is an array; each row has exactly
  `clock/endpoints/wns_max_ns/tns_max_ns/wns_min_ns/tns_min_ns/
  setup_violations/hold_violations`; `clock` is a string or `null`.
- `slack_histogram` is a 5-element array in the fixed order with
  `bucket` values `negative/p0_to_20/p20_to_50/over_50/no_period`.
- `endpoints[]` rows carry no `domain_row`/`domain_period_ns` keys
  (serde-skip is real, not just rename).

## 3. Consistency recompute (`test_domain_summary_consistency`)

The acceptance's "equal the same quantities recomputed from
`endpoints`" — recompute, do not re-derive:

- Per row: `wns_*` = `fold(INFINITY, min, worst_*_slack_ns)`,
  `tns_*` = `Σ min(0, worst_*)`, violations = counts of `worst_* < 0`
  over `endpoints` filtered to `e.domain_row` matching the row index
  (rows are `domains[i]` ↔ `domain_row == i`; the null row ↔ `None`).
- Partition algebra: `Σ domain.endpoints == report.endpoints.len()`,
  `min(domain wns) == global wns`, `Σ tns == global tns`,
  `Σ violations == global violations`.
- Histogram: recompute each count from `worst_max_slack_ns` and
  `domain_period_ns` per `attribution.md` §4; assert all five equal and
  `Σ == endpoints.len()`; assert `negative == setup_violations`.
- Run this on `t2_selmc` *and* on a partitioned fixture —
  `sta_exceptions/exceptions.def` + `md_basic.sdc` — so the
  `run_sta_partitioned` compose path is covered too (it exercises the
  `..w`-inherited `dom` and any `u8::MAX` leaves). If `md_basic` yields
  no `null` row, note it; if it does, assert the members are exactly
  the delay-check leaf endpoints.

## 4. Merge (`test_domain_summary_merge`)

- On the `endpoint()`/`report()` helpers (`None` domains): merged
  report emits a consistent null row — `Σ endpoints ==
  merged.endpoints.len()`, histogram `no_period == len`. This guards
  `merge_corner_reports`' new code on the degenerate input.
- If cheap to wire: two reports where `endpoint()` takes a `domain_row`
  parameter — merged max side keeps setup's `domain_row`, min-only
  endpoint takes hold's. Otherwise the partitioned-fixture consistency
  run in §3 is the merge-adjacent coverage; state which was done.

## 5. Fail-proof (spec: "attribute all endpoints to the first domain")

The corruption: in `endpoint_report`'s derivation (or
`summarize_domains`'s group key), force `domain_row = Some(0)` for every
endpoint.

- Expected red: `test_domain_summary_two_clock` — `ckb` row gets
  `endpoints == 0` and the pin-set assertion fails (members named).
  Also `test_domain_summary_consistency` stays green **by design** —
  it recomputes over the same corrupted `domain_row`, which is exactly
  why §1 hardcodes expected membership; say this in the results entry.
- Record the failing assertion output in `docs/results.md` (same
  format as the `ux-sta-paths` entry), restore, re-run green.

## 6. Stdout verification

No CLI test exists for stdout; verify manually like `ux-sta-paths` did:

```text
grus-eda sta --lib <tt lib> --tlef <nom tlef> --lef <hd lef> \
  --def tests/designs/sta_clocks/two_clock.def \
  --sdc tests/designs/sta_clocks/t2_selmc.sdc --json /tmp/t2_selmc.json
```

- stdout shows the `domains` table (two rows) + the histogram line.
- `/tmp/t2_selmc.json` has `domains` (2 rows) and `slack_histogram`
  (5 buckets, sum = 8).
- Record the printed block verbatim in `docs/results.md`.
