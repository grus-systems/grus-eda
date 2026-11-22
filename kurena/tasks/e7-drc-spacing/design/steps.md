# e7-drc-spacing — Implementation steps

Ordered so each step compiles and commits green. Suggested commit
split (one ticket, several focused commits per AGENTS §5).

## Step 1 — LEF spacing retention

- `tech_db.rs`: `SpacingTableRow`; `RoutingLayer.{spacing_dbu,
  spacing_table}` + `min_spacing_dbu()` / `wide_spacing()` helpers;
  `CutLayer.spacing_dbu` — all `#[serde(default)]`.
- `lef.rs`: `parse_layer` arms for `SPACING <f>` (plain scalar;
  non-scalar forms fall through to `skip_to_semi`) and
  `SPACINGTABLE` (consume `PARALLELRUNLENGTH <f>...` then `WIDTH w s`
  pairs until `;`). Top-level `SPACING` skip counting untouched.
- Unit tests per `tests.md` §lef.rs.
- **Commit**: `feat: Retain LEF SPACING and SPACINGTABLE per layer`.

## Step 2 — Via-instance group ids

- `drc.rs`: `ShapeRect.group: u32` (`#[serde(default)]`, 0 =
  ungrouped); update the doc comment.
- `geom.rs`: assign `group = k+1` (dense, signal-then-special chain
  order) inside the via loop; fix struct literals in tests/fixtures
  (`width::tests::design_with`, geom fixtures).
- Unit test: distinct groups per instance, `0` elsewhere.
- **Commit**: `feat: Tag via-instance rects for cross-layer net merge`
  + version `0.9.0 → 0.10.0` (public-struct break; the step-1 field
  additions ride this bump).

## Step 3 — `drc::spacing`

- `drc.rs`: `pub mod spacing;`.
- `spacing.rs`: `SpacingKind`, `SpacingRule`, `SPACING_RULES` (20,
  table order), `Violation`, `check_spacing` with the in-module
  union-find (same-net touching pass + group pass), bound resolution,
  `wide_mark` computation, pair predicate per `spacing.md`.
- Unit tests per `tests.md` §spacing.rs — metadata pin, gap/touching/
  Euclidean boundaries, island fixtures, group fixture, wide fixtures.
- **Commit**: `feat: Add same-layer spacing DRC family (LEF bounds)`.

## Step 4 — Integration gate + mutation + fail-proof

- `tests/drc.rs`: `test_spacing_zero_violations` (with the per-rule
  census printout), `test_spacing_rules_join_golden` (shared
  paren-token helper), `test_spacing_mutation_met2_probe`
  (golden-driven `sanity_control.mutation` replay into net `_001_`).
- Run the merge-disable fail-proof; keep red excerpts for
  `results.md`; try the `<`→`<=` secondary break and record the
  outcome either way.
- **Commit**: `test: Gate drc spacing rules against Magic goldens`.

## Step 5 — Docs + verify

- `docs/e7-drc.md` (family table + merge semantics + bound table +
  wide-metal paragraph), `docs/results.md` dated entry,
  `docs/status.md`, `docs/roadmap.md` per `docs.md`.
- `cargo test --workspace` + `cargo clippy --workspace --all-targets
  -- -D warnings` clean.
- **Commit**: `docs: Record e7-drc-spacing verification and family status`.

## Done-when map

| Acceptance | Covered by |
|---|---|
| Same-layer spacing on both routed DEFs, 0 violations, `magic_per_rule` join | Steps 3–4 (`test_spacing_zero_violations`, `test_spacing_rules_join_golden`) |
| Same-net merge documented + tested (stubs don't self-flag; separated islands do) | Steps 2–3 (`merge.md` → `docs/e7-drc.md`; island + via-group unit fixtures) |
| met2 0.03µm-gap probe → exactly `met2.2` | Step 4 (`test_spacing_mutation_met2_probe`) |
| Wide-metal rules, named provenance, both designs 0 | Steps 1, 3 (`wide_spacing` LEF rows + `lef-spacing.md` table → docs) |
| Negative/fail-proof (merge or edge-distance disabled → red → restore) in results.md | Step 4 |
| `cargo test --workspace` + clippy `-D warnings` | Step 5 |
| `docs/e7-drc.md` family table; `results.md` dated entry | Step 5 |

## Watch-items during implementation

- **`iter_overlapping` reach**: inflate the probe by the *rule's* bound
  (up to 1600 on met5); the merge pass inflates by exactly 1. Do not
  reuse the strict-overlap meaning — `gx<1 && gy<1` is the touch test.
- **Pair dedup**: raw-pointer order inside one `Vec` — do not compare
  `RectDbu` values (equal rects from different sources would
  collapse).
- **Union-find spans layers**: instance-group union crosses
  routing/cut layers; the touching pass is per layer. Flatten index
  space over the whole `DrcDesign`.
- **Cut-layer bounds**: `via.2` LEF 170 > Magic 60 etc. — if the
  zero gate trips on a via rule, dump the pair's `src`/`net`/`group`
  into `results.md` and reconsider the merge (most likely candidate:
  a merge case the island model misses), not the bound.
- **`li1`/`mcon` coverage**: expect zero candidate shapes on both
  designs — the census must print that explicitly rather than let the
  0 read as coverage (AGENTS §7.4); the synthetic fixtures carry the
  rules' real exercise.
- **`LefSkipReport`**: top-level `SPACING` counting stays as is — the
  new arms are inside `parse_layer` and never reach the counter; the
  pinned `report.skipped["SPACING"]` test must keep passing.
- **Def grammar**: the mutation inserts `NEW met2 ( x y ) ( x2 y2 )`
  with no explicit width — the parser defaults to layer `WIDTH`
  (proven by the committed `NEW met1` segments); assert the mutated
  text differs before parsing.

## Files

- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/tech_db.rs`
- `crates/grus-core/src/drc.rs`
- `crates/grus-core/src/drc/geom.rs`
- `crates/grus-core/src/drc/spacing.rs`
- `crates/grus-core/tests/drc.rs`
- `docs/e7-drc.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
