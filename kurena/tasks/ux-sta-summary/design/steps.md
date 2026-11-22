# ux-sta-summary — Implementation steps

Ordered; each step compiles before the next. All edits in
`crates/grus-core/src/sta/analysis.rs` unless noted.

## 1. `Worst.dom` plumbing

- Add `dom: u8` to `struct Worst` (~line 5885) with the doc from
  `api.md` §2.
- Stamp it at all 17 `Worst {}` literal sites:

| site | value |
|---|---|
| ~4897 port delay rewrite (max), `..w` | inherited |
| ~4925 latch/FF delay rewrite (max), `..w` | inherited |
| ~4977, ~5045, ~5073, ~5087 partitioned rewrites | `..w` inherit, or the in-scope `cap_row`/`out_row` as `u8` on full literals |
| ~5339, ~5393, ~5417, ~5437 deferred-endpoint union compose | `cap_row`/`out_row` as `u8`; `u8::MAX` when the path anchors at `N` only |
| ~5517, ~5531 internal leaf cands | `u8::MAX` |
| ~7343, ~7352 register-check candidates | `cap` |
| ~7541, ~7550 clock-gate candidates | `cap` (the gate's `clock_domain[g.clk_pin]`) |
| ~7728 port anchor candidate | `out_row as u8` |

  Rule of thumb: the value is the same row the candidate's
  latency/uncertainty/period terms were read from; `u8::MAX` when the
  check anchors at a bare delay value.

## 2. `EndpointReport` fields + `endpoint_report` derivation

- Add `domain_row`/`domain_period_ns` (`#[serde(skip)] pub(crate)`) per
  `api.md` §2.
- `endpoint_report` gains `sdc: &Sdc`; derives the fields from the
  picked reported candidates (`max` first, `min` fallback).
- Update its two call sites: `record_endpoint` (~6469, pass `cx.sdc`),
  partitioned entries push (~5561, pass `sdc`).
- `merge_corner_reports` `EndpointReport` literal (~7853): copy per
  `api.md` §4 (`e.max_slack_ns.is_some()` → setup's, else
  `h.domain_row.or(e.domain_row)` + matching period).
- Test helper `endpoint()` (~13737): `domain_row: None,
  domain_period_ns: None`.

## 3. Report types + `summarize_domains`

- `DomainSummary`, `SlackBucketKind`, `SlackBucket` per `api.md` §1.
- `StaReport` += `domains`, `slack_histogram` (always serialized).
- `fn summarize_domains` per `api.md` §3 — grouping by `domain_row`
  index; unattributed bucket under `None`; rows in `clock_names` order +
  conditional null row; histogram always 5 buckets in the fixed order.
- `assemble_sta_report` gains `sdc: &Sdc`; fills the new fields via
  `summarize_domains(&endpoints, &clock_names)`. Both call sites
  (`run_sta_detail` ~2530, `run_sta_partitioned` ~5865) pass `sdc`.
- `merge_corner_reports` fills them via `summarize_domains(&endpoints,
  &names)` with `names` from `setup.domains` (`api.md` §4).
- Test helper `report()` (~13771): `domains: Vec::new(),
  slack_histogram: Vec::new()`.
- `sta/mod.rs`: re-export `DomainSummary`, `SlackBucket`,
  `SlackBucketKind`.

## 4. Stdout rendering (`crates/grus-eda/src/main.rs`)

- After the pulse-width line, before `print_skip_summary` (~1014):
  domain table + histogram line per `api.md` §6. No new CLI flag, no
  JSON code.

## 5. Tests (`#[cfg(test)]` in `analysis.rs`)

Per `tests.md`:

- `test_domain_summary_two_clock` — `two_clock.def` + `t2_selmc.sdc`:
  two populated rows `cka`/`ckb`, expected pin→domain membership and
  endpoint counts, domain wns/tns bounds vs globals.
- `test_domain_summary_consistency` — recompute every domain field and
  every histogram bucket from `report.endpoints` and assert equality +
  the partition algebra (`tests.md` §3).
- `test_domain_summary_json_schema` — `serde_json::to_value` key/field
  assertions incl. the null-row form.
- `test_domain_summary_merge` — `merge_corner_reports` keeps domains:
  merged `domain_row` per endpoint, `summarize_domains` on the merged
  set (the existing `endpoint()`/`report()` helpers carry `None`
  domains — assert the merged report still emits a consistent null row).
- Fail-proof per `tests.md` §5; record red output in `docs/results.md`.

## 6. Docs + version

- `docs/e9-reporting.md`: new sections `domains` and
  `slack_histogram` — schema, attribution rules (`attribution.md` §§2–4),
  multi-anchor port rule, bucket bounds, merge rule.
- `docs/status.md` E9 row: items (2)+(3) DONE with version + evidence.
- `docs/results.md`: dated entry — fixture numbers, checks run,
  fail-proof red output, the `0.6.0 -> 0.6.1` patch rationale.
- `Cargo.toml` `0.6.0 -> 0.6.1`, `cargo build` regenerates `Cargo.lock`.

## 7. Verify

`cargo test --workspace`, `cargo clippy --workspace --all-targets --
-D warnings`. Manual CLI smoke on the fixture, recorded in
`docs/results.md` (`tests.md` §6).
