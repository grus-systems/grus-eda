# ux-sta-summary — Overview

E9 ticket 2 (`docs/history/run-2026-11-22-large-design.md` items 2+3,
`docs/roadmap.md` §E9 items 2–3): `StaReport` gains a per-clock-domain
slack summary (`domains`) and a slack histogram (`slack_histogram`), both
printed on `sta` stdout after the existing wns/tns line and serialized by
`--json`. One glance answers "which domain broke" and "how close to
closure are we" without parsing per-endpoint rows.

## Why this is small

The engine is already domain-aware: `StaPrepared.clock_domain`
(`Vec<u8>`, per-pin `Sdc::clocks` row, `u8::MAX` = none) gives every
check and gate its capture domain, and `Sdc::declared_output_clock_rows`
gives a port's surviving `-clock` anchors. What is missing is carrying
the anchor *onto the check candidate*: the right place is `Worst`, which
already carries non-serialized bookkeeping (`class`, `from_pin`) for the
path tracer. Adding `dom: u8` to `Worst` makes attribution exact — the
reported `EndpointReport` derives its domain from its own reported
candidate, in both the single-state (`collect_endpoints`) and the
partitioned (`run_sta_partitioned`) paths, with no second lookup.

## Semantics decisions (each also documented in `docs/e9-reporting.md`)

1. **Domain attribution = the reported check's anchor.** `Worst` gains
   `dom: u8` — the `Sdc::clocks` row of the check that produced this
   candidate, `u8::MAX` when the candidate has no clock anchor (internal
   `set_max_delay` leaf endpoints, `req = N` checks, unclocked captures).
   `EndpointReport` gains two `#[serde(skip)]` `pub(crate)` fields,
   `domain_row: Option<usize>` and `domain_period_ns: Option<f32>`,
   derived in `endpoint_report` as *the reported candidate's* `dom`:
   the picked `max` `Worst` first, `min`'s as fallback. A register/gate
   check's `dom` is `clock_domain[clk]`; a port check's is the `out_row`
   of the anchored entry that produced the candidate — so a port
   anchored on several `-clock`s lands in the domain of its
   **last-producing max-corner anchor** (min corner's when it has no max
   check): the same last-group-wins rule the `EndpointReport` comment
   already documents for the reported row (`t2_clkcor2`), applied at
   endpoint granularity. `details in attribution.md`.

2. **One `domains` row per `create_clock` + an unattributed row when
   needed.** Rows follow `sdc.clocks` file order (virtual rows included —
   a `create_clock -name` is a domain even though only port checks can
   anchor on it), then a trailing `clock: null` row iff ≥1 endpoint has
   `domain_row == None`. A domain with zero captured endpoints still
   emits a row (`endpoints: 0`, wns `null` via the `INFINITY` fold,
   tns 0, violations 0) — "present in the run" means declared, not
   populated. Sum of `endpoints` over all rows equals
   `endpoints.len()` — attribution is a partition.

3. **Domain metrics use `worst_*_slack_ns`, same as the globals.** A
   domain row's `wns_*`/`tns_*`/`*_violations` are the same folds as
   `StaReport`'s, restricted to that row's endpoint subset. Because the
   subsets partition the endpoint set: `min(domains.wns_max) ==
   report.wns_max`, `Σ tns == report.tns_*`, `Σ violations ==
   report.*_violations` — the consistency test asserts this partition
   algebra rather than only "domain ≤ global".

4. **Histogram buckets worst max slack against the capture period.**
   Key: `worst_max_slack_ns` (the wns-driving value — makes the
   `negative` bucket count identical to `setup_violations` by
   construction). Five buckets, always all five emitted in order:
   `negative` (`s < 0`), `p0_to_20` (`0 <= s < 0.2·P`),
   `p20_to_50` (`0.2·P <= s <= 0.5·P`), `over_50` (`s > 0.5·P`),
   `no_period` (endpoint has no `domain_period_ns`, or no max-side
   candidate at all). `P` is the endpoint's *capture* period — a
   cross-domain check is judged against its own clock's period, matching
   the required time the slack was computed against. Counts sum to
   `endpoints.len()` by construction.

5. **Both blocks are computed from `endpoints` by one pure helper.**
   `summarize_domains(&[EndpointReport], &[String]) -> (Vec<DomainSummary>,
   Vec<SlackBucket>)` groups by `domain_row` — `assemble_sta_report`
   passes `sdc.clocks` names, `merge_corner_reports` passes the clock
   names of `setup.domains` (row order is identical; same SDC). Since
   the blocks are a pure recompute over the endpoint list, the
   "recomputed from `endpoints`" acceptance is true by construction and
   the `--lib-hold` merge needs no Sdc. The merge copies `domain_row`/
   `domain_period_ns` per endpoint: setup's when its reported max check
   exists (that is the merged report's max side and its anchor), else
   hold's `domain_row`/period falling back to setup's — the merged min
   side comes from the hold run.

6. **`domains`/`slack_histogram` are always serialized** (no
   `skip_serializing_if`): `domains` is an empty array when nothing is
   declared/reported, `slack_histogram` always has all five buckets.
   `endpoints[]` rows are byte-stable — the two new per-endpoint fields
   are `#[serde(skip)]`.

7. **Stdout:** a table (one line per domain row) plus one histogram
   line, printed after the existing wns/tns + pulse-width lines and
   before `print_skip_summary`. ≤ ~20 lines total: header + row per
   domain + 2 histogram lines. The unattributed row prints clock as
   `(none)`.

## What stays the same

- `run_sta*` signatures — the summary needs no flag; it is always
  computed (`O(endpoints)`).
- Every existing `EndpointReport` field and every existing `--json` key;
  both new blocks are additive.
- Eval, endpoint, partition, and skip logic: `dom` is bookkeeping on
  `Worst` like `class`/`from_pin` — never serialized, never part of any
  assertion. `merge_corner_reports`' endpoint-set equality check is
  unchanged.
- `critical_paths` semantics and `pulse_width_checks` (a separate family
  that never enters `endpoints` — pulse checks do not appear in the
  domains table or the histogram; documented).

## Out of scope

- Per-anchor (path-group) breakdown of multi-clock ports: the endpoint
  row is the unit, matching OpenSTA's endpoint table.
- Min-corner histogram (the spec'd histogram is the max/setup side).
- `--paths` interplay, `--top` ergonomics (`ux-top-sdc`), SDC templates.
- Exposing `domain` per endpoint in JSON — the `domains` block is the
  surface; the per-endpoint fields stay crate-internal.

## Risks

- **`dom` plumbing coverage**: ~19 `Worst {}` literals plus `..w`
  spreads across `collect_endpoints` and the partitioned compose. Each
  site is audited in `steps.md`; a missed site silently yields
  `u8::MAX` → the endpoint lands in the unattributed row. The fixture
  test asserts per-domain membership/counts, which catches this on
  `two_clock`.
- **Merge attribution drift**: `merge_corner_reports` re-derives
  `domain_row` per the rule in decision 5 — an approximation only in the
  corner-divergent multi-anchor-port case (documented).
- **The consistency test cannot catch mis-attribution**: recomputing
  from `endpoints` shares the `domain_row` it recomputes against. The
  *fixture* test (hardcoded expected pin→domain membership and endpoint
  counts) is the one the fail-proof targets — see `tests.md`.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/mod.rs`
- `crates/grus-eda/src/main.rs`
- `docs/e9-reporting.md`
- `docs/results.md`
- `docs/status.md`
- `Cargo.toml`
- `Cargo.lock`
