# docs — the corrected record (revision 2)

AGENTS rule 8 — `docs/` in step with the code. All English. This file
specifies every change to the 2026-11-27 `drt-scale-memory` entry in
`docs/results.md`; each numbered section closes one review item.

## §1 Machine context + rep policy (review item 1)

Insert a `**Machine context**` block after the entry's opening
paragraph, same format as the `drt-rayon` entry's:

- Hardware/OS/toolchain line — re-read `sw_vers` and
  `rustc --version` at write time (today: Mac mini, Apple M4 (4P+6E),
  16 GB; macOS 27.2.1 (26B131); `rustc 1.100.0 (0ef1bc386
  2026-11-09)`), `cargo build --release`, binary
  `target/release/grus-eda`.
- Rep policy sentence: **gcd ≥3 reps per binary via `/usr/bin/time
  -l`, the median is the tabled figure; picorv32 one detached rep per
  binary — each leg is a multi-hour route, the named cost reason.**

## §2 Complete split table (review item 2)

The designed rows are `measurement.md` §5. All numbers already exist
in the retained stderr logs — transcribe, don't recompute. Builds:
`base` = HEAD baseline (uninstrumented, `*.mem.base.*`); `inst` =
commit-1 instrumented baseline (`*.mem.inst.log`); `new` = this
change (`*.mem.new.log`). "Before" census rows come from `inst`
(measurement-only build — it holds HEAD's structures); "before" peak
RSS comes from `base` (see §3).

| row | census key | where |
|---|---|---|
| `route_graphs` | `post_graphs graphs=` | `inst` / `new` (new: summed capacity of the streamed builds — transient, ≤threads resident) |
| `net_topos — attach` | `post_topos attach=` | both |
| `net_topos — access` | `post_topos access=` | both — **separate row, not merged with cover** |
| `net_topos — cover` | `post_topos cover_inner=` exact + `cover_entries × BTREE_NODE_EST` `~est` | both |
| `net_topos — members` | `members_inner=` exact + `members_entries × …` `~est` | before only — flattened into `NetAdj` CSR after; report under `attempt_hi` |
| `net_topos — relax` | `post_topos relax=` | before only — rebuilt per attempt after |
| `net_topos — csr` | `post_topos csr=` | before only — rebuilt per attempt after |
| `occupancy` | `post_premark occ=` → steady `iter occ=` | both |
| `marks` | `iter marks=` hi + `post_route marks=` | both |
| `routes + residuals` | `post_route routes=` + `residuals=` | both |
| `outcomes hi-water` | max `iter outcomes=` (iter-1) | both; note the after value carries `EdgeProbe`s in place of `used_edges` — a deliberately larger outcome, named in the entry |
| `attempt_hi` | `iter attempt_hi=` | after only; serial-iter value per §6 |
| `rest` | `post_topos rest=` (`rss − Σmeasured`, `saturating_sub`) | both — picorv32 before reads 0; keep the 0 and name the saturation |
| **peak RSS (`time -l`)** | `maximum resident set size` | §3 |

Verified figures already in the logs (transcribe, in bytes):

- gcd `inst`: `topos=91025382 attach=64266 members_entries=7041
  members_inner=112656 members_est=563280 relax=44441600 access=133320
  cover_entries=877 cover_inner=64832 cover_est=70160 csr=46208708
  occ=2648284 rest=21508010`; iter-1 `marks=1580344 outcomes=261980`;
  `post_route routes=126264 residuals=12912`; peak `152829952`.
- gcd `new`: `topos=262418 attach=64266 access=133320
  cover_entries=877 cover_inner=64832 cover_est=70160 occ=2648284
  rest=17667602`; iter-1 `marks=1580344 outcomes=733484
  attempt_hi=6519748`; `post_route routes=126264 residuals=12912`.
- picorv32 `inst`: `topos=3127280120 attach=1997776
  members_entries=227610 members_inner=3641760 members_est=18208800
  relax=1478266880 access=3959600 cover_entries=26492
  cover_inner=2033024 cover_est=2119360 csr=1637381080 occ=82783876
  rest=0`; iter-1 `marks=69483840 outcomes=8067768`; `post_route
  routes=3937544 residuals=323712`; peak `3138633728`.
- picorv32 `new`: iter-1 `marks=69483840 outcomes=24261976
  attempt_hi=217516920`; steady `rss=958464000`; `post_route
  routes=3937544 residuals=323712`.

After-rows whose before-column cannot exist (`attempt_hi`) get `—`,
and rows that moved to the transient budget (`members`/`relax`/`csr`
after) report `→ attempt_hi` rather than 0.

## §3 Baseline reconciliation (review item 3)

The two "before" figures are two different builds:

- **153,092,096 B (153.1 MB)** — `base` HEAD baseline, median of the
  3 retained reps (153,092,096 / 154,255,360 / 152,551,424 — rep 1 is
  the median). This is the canonical `before` for the before/after
  table.
- **152,829,952 B (152.8 MB)** — `inst` instrumented baseline, single
  rep. The census code itself allocates; it is legitimately a
  different number and was mislabeled as the same "before".

Fix: the census table's peak row is labeled `inst` build; the
before/after table uses the `base` median; one sentence states the
reconciliation ("the census table's peak is the instrumented build's
`time -l` figure; the headline before/after uses the un-instrumented
HEAD median — a ~0.3 MB, 0.2 % build difference, inside the rep
spread"). List the three baseline rep values so the median is
checkable. `e6-drt.md`/`status.md` already carry 153.1 — consistent
after the fix. Re-run the `new` gcd binary twice more so its figure
is also a ≥3-rep median; adjust 82.6 MB only if the median moves.

## §4 `route_ms` delta (review item 4)

`route_ms` is on the unconditional `guided=… route_ms=` stdout line —
read it from the same logs/reps:

- gcd `base` median **5705 ms** (5874 / 5705 / 5522); `new` median
  from the fresh reps — record it with Δ%.
- picorv32: `route_ms` `7229267 → 4130770` (−43 % — the existing
  wall-time claim stays; make `route_ms` an explicit row so the
  reduction provably did not trade runtime for memory on gcd either).

## §5 Version typo (review item 5)

"minor bump 0.14.4 → 0.15.0" → "**0.14.3 → 0.15.0**" (the parent was
`0.14.3`, per `git log Cargo.toml`). Sweep the entry for any other
stale version string.

## §6 Serial-arm `attempt_hi` (review item 7)

Two-part record, paired with the `code-fixes.md` §2 code change:

- The census now measures the serial arm. Record the fresh gcd
  `iter=8` `attempt_hi` from the post-fix `GRUS_DRT_MEM` run.
- picorv32 was **not re-run** (the change is a `mem_on`-gated
  read-only size walk; the leg is ~70 min). The entry states the
  bound instead: the hard iteration commits inline holding exactly
  one `(graph, adj)` pair, and iter-1 attempts every guided net, so
  the recorded `attempt_hi=217516920` (the global max pair over the
  full net set) already bounds any serial-arm pair — the retained
  leg's `iter=8 attempt_hi=0` predates the serial-arm census key, and
  that gap is named, not hidden.
- Also state once: `outcomes=` is 0 on the hard iteration by
  construction — the serial arm has no outcomes buffer (inline
  commits).

## §7 Rest of the entry

Keep the existing honest-bound paragraph, identity evidence,
prove-it-red, exact commands, and checks-run sections; amend:

- Checks run: append the revision-2 verification (fresh gcd census
  run + reps, `cargo test --workspace --profile verify`, clippy).
- What-was-run honesty: the picorv32 numbers are extracted from the
  three retained run logs; the gcd after-reps and the serial-arm
  census are fresh runs — say which.

## `docs/e6-drt.md`

One clause in the Resident footprint block: the per-iteration
`attempt_hi` row now covers the serial arm too (previously the
snapshot arm only). No figure changes (153.1 stays).

## `docs/status.md`

Bump the entry's version token to `v0.15.1`; no figure changes.
