# e7-drc-enc-area — Implementation steps

Ordered so each step compiles and commits green. Suggested commit
split (one ticket, several focused commits per AGENTS §5).

## Step 1 — LEF AREA / MINENCLOSEDAREA / ENCLOSURE retention

- `tech_db.rs`: `RoutingLayer.{area_dbu2, min_enclosed_area_dbu2}`
  (`i64`), `CutLayer.{enc_below_dbu, enc_above_dbu}` (`[i32;2]`) —
  all `#[serde(default)]`.
- `lef.rs`: `parse_layer` arms for `AREA`, `MINENCLOSEDAREA`
  (routing), `ENCLOSURE {BELOW|ABOVE}` (cut; non-side keyword falls
  back to `skip_to_semi`). Top-level skip counting untouched —
  these arms never reach `LefSkipReport`.
- Unit tests per `tests.md` §lef.rs.
- **Commit**: `feat: Retain LEF AREA, MINENCLOSEDAREA and ENCLOSURE
  per layer` + version `0.10.1 → 0.11.0` (public-struct fields; the
  field additions ride this bump per AGENTS).

## Step 2 — `drc::enclosure`

- `drc.rs`: `pub mod enclosure;`.
- `enclosure.rs`: `Side`, `EncMode`, `EnclosureRule`,
  `ENCLOSURE_RULES` (9, table order), `Violation`, `RuleReport`,
  `check_enclosure_report`/`check_enclosure` — (cut,side)-grouped
  measurement pass per `enclosure.md` (probe inflate by `bmax`,
  same-net candidates, per-side span sweeps, clause evaluation).
- Unit tests per `tests.md` §enclosure.rs — metadata pin, Both/One/
  Full boundaries, union coverage, net filter, protrusion,
  multi-cut array, absent layers.
- **Commit**: `feat: Add via enclosure DRC family (LEF overhang bounds)`.

## Step 3 — `drc::union` + `drc::area`

- `drc.rs`: `pub mod union; pub mod area;`.
- `union.rs`: `is_member`, `components` (net-agnostic closed-touch
  union-find via `for_each_pair_within(1)`), `union_area` x-sweep,
  `enclosed_voids` (free-space components, edge-connected, window-
  boundary test).
- `area.rs`: `AREA_RULES` (6), `HOLE_RULES` (2), `Violation`,
  `RuleReport`, `check_area*`/`check_hole*` — drawn-member gate,
  foreign-metal subtraction via `iter_overlapping`, OBS discard,
  strict `<`.
- Unit tests per `tests.md` §union.rs/§area.rs — merge semantics,
  union dedup, donut/C-shape/point-pinch voids, drawn-member gate,
  OBS discard, bound boundaries.
- **Commit**: `feat: Add min-area and min-hole-area DRC families`.

## Step 4 — Integration gates + mutations + fail-proofs

- `tests/drc.rs`: the six gate/join tests plus the two DEF
  mutations (`+ ENCLOSURE 10 10` on `via2_3_*`, `+ RECT met1` island)
  per `tests.md` — including per-rule census prints and deterministic
  double-runs.
- Find and record the isolated `+ RECT` coordinates from the
  assembled design (≥ ~2× spacing clearance on met1); keep them
  fixed in the test.
- Run the three fail-proofs (`>` break on enclosure, merge-disable
  on area, boundary/OBS break on holes); keep red excerpts for
  `results.md`.
- **Commit**: `test: Gate drc enclosure/area/hole rules against
  Magic goldens`.

## Step 5 — Docs + verify

- `docs/e7-drc.md` §7 table + §13–15 per `docs.md` (Magic values
  re-derived from `sky130A.tech` in this step), `docs/results.md`
  dated entry, `docs/status.md`, `docs/roadmap.md`.
- `cargo test --workspace` + `cargo clippy --workspace
  --all-targets -- -D warnings` clean.
- **Commit**: `docs: Record e7-drc-enc-area verification and
  complete lef_expressible coverage`.

## Done-when map

| Acceptance | Covered by |
|---|---|
| Enclosure cut-vs-metal incl. one-direction variants; 0 on both DEFs | Steps 1–2, 4 (`test_enclosure_zero_violations`, `test_enclosure_rules_join_golden`) |
| Min-area merged-polygon per island; 0 both | Steps 3–4 (`test_area_zero_violations`, `test_area_rules_join_golden`) |
| Hole detection; synthetic sub-min fixture flags `met1.7`/`met2.7` | Steps 3–4 (`enclosed_voids` donut fixture, `test_hole_zero_violations`) |
| Mutation: undersized enclosure → `met*.4`/`via*.4a`-family; undersized island → `*.6` | Step 4 (`test_enclosure_mutation_def_via_enclosure` → `via.5a`; `test_area_mutation_met1_island` → `met1.6`) |
| Fail-proof per family in results.md | Steps 4–5 |
| `cargo test --workspace` + clippy `-D warnings` | Step 5 |
| `e7-drc.md` family table all `lef_expressible` done; `results.md` dated entry | Step 5 |

## Watch-items during implementation

- **`ENCLOSURE` namespaces**: `VIARULE` sub-blocks are skipped at top
  level, but assert the new arm only fires inside `LAYER` blocks —
  `ENCLOSURE` on a non-cut layer records nothing (`is_cut` gate),
  not an error.
- **Cut-layer index vs name**: rules key on cut *name* (`via`,
  `via2`, …) via `cut_layer_by_name` — via *names* (`via2_3`,
  `via4_5`) are decoration; `via2_3`'s cut is `via` (via1).
- **Net filter, not island filter**: enclosure candidates are
  `net == cut.net` — do not reuse `spacing`'s union-find (a foreign
  wire inside the probe region must not enclose).
- **Multi-cut arrays**: each cut checks independently — interior
  cuts' side slabs are metal-covered, so only boundary cuts see
  small overhangs; the DEF-VIAS mutation count is `instances ×
  cuts`, not instances.
- **`met4`-side of `via4` is intentionally unchecked** (no deck
  name) — do not invent a rule row for it.
- **Component member vs drawn member**: `MacroPin` merges into
  polygons (conservative) but a `MacroPin`-only component is never
  evaluated — the `li.6` census reads 0 candidates on both designs;
  print it honestly (AGENTS §7.4).
- **OBS is not solid**: keep `MacroObs` out of `components` and out
  of `solid`; it only discards candidate voids. Treating it as
  metal inflates area coverage in the wrong direction.
- **i64 areas**: `RectDbu::area()` already returns i64 — propagate;
  `union_area`/`Hole::area_dbu2`/`bound_dbu2` are i64.
- **`exact_overlap` statements in the deck** (`mcon/li`, `v1/m1`…) —
  not `why` names, no rule rows; do not confuse with `surround`.

## Files

- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/tech_db.rs`
- `crates/grus-core/src/drc.rs`
- `crates/grus-core/src/drc/enclosure.rs`
- `crates/grus-core/src/drc/union.rs`
- `crates/grus-core/src/drc/area.rs`
- `crates/grus-core/tests/drc.rs`
- `docs/e7-drc.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
