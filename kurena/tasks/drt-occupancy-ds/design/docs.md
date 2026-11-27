# docs — required doc edits

Three doc files move with the code (AGENTS rule 8 — docs in step).

## `docs/e6-drt.md` — Occupancy paragraph (lands in commit 2)

The §Occupancy paragraph (~L560–575) currently reads
"`lines_y`/`lines_x` map a line coordinate to a `Line`" — that is
the structure being removed. Rewrite the sentence to: per layer,
`lines_x`/`lines_y` are `Vec<Line>` **indexed by track position**
(the mark domain is exactly the track index set — rasterization
only ever touches track lines), with the sorted coordinate lists
held alongside for `Hit.line` reconstruction and
`scan_rect`/`rasterize` window `partition_point`s. The `Line`
contents description (`lo`-sorted `ivs`, `SKIP_CHUNK` skip index)
is unchanged and stays.

## `docs/results.md` — dated entry (lands in commit 4)

Heading and contents per `measurement.md` §5. Conventions to keep:

- Numbers from the committed commands run as written; rep policy
  stated; medians are the table figure.
- `route_ms` is the `guided=…` census field, not `/usr/bin/time`;
  the reps' DEF sha256 joins to the golden's recorded sha.
- The scan-share figure is stated as what `sample` resolved —
  if `scan_*` inlined into `edge_cost`, the entry says "share
  attributable: N% (scan frames folded into `edge_cost`; residual
  measured as route_ms delta)" rather than a fake split.
- picorv32 arm: the bounded-run transcript or the words "not
  measured — deferred" (acceptance: name it if measured).
- Failed/divergent observations stay in the log with root cause
  (AGENTS §2).

## `docs/status.md` — "Last updated" line (lands in commit 4)

Append a `v0.14.2 drt-occupancy-ds` entry in the existing style:
one sentence naming the internal swap (`BTreeMap<i32, Line>` →
dense track-indexed `Vec<Line>`), the recorded gcd `route_ms`
before→after, and "route digest + emitted DEF byte-identical
(differential test + `GCD_ROUTE_DIGEST` pin added)".
