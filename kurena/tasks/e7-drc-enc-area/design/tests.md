# e7-drc-enc-area — Test plan

Unit tests live in-module (`lef.rs`, `drc/enclosure.rs`,
`drc/union.rs`, `drc/area.rs`); design-scale tests extend
`crates/grus-core/tests/drc.rs` (`load(name)`, golden access,
`print_census`, `paren_tokens` already in place).

## Unit tests

### `readers/lef.rs` (extend the `W_LEF`-style mini fixture)

- `LAYER mcon TYPE CUT ... ENCLOSURE BELOW 0 0 ; ENCLOSURE ABOVE 0.03
  0.06 ;` → `enc_below_dbu == [0,0]`, `enc_above_dbu == [30,60]`.
- `LAYER met1 TYPE ROUTING ... AREA 0.083 ; MINENCLOSEDAREA 0.14 ;` →
  `area_dbu2 == 83000`, `min_enclosed_area_dbu2 == 140000`.
- `ENCLOSURE` without `BELOW|ABOVE` (e.g. `ENCLOSURE 0.1 0.1 ;`) →
  skips via `skip_to_semi`, fields stay `[0,0]`, no error.
- Layers without the statements → `area_dbu2 == 0`,
  `min_enclosed_area_dbu2 == 0` (rule-skip semantics downstream).

### `drc/enclosure.rs`

Mini LEF: met1/met2 routing + `mcon` (`BELOW 0 0`, `ABOVE 30 60`) +
`via` (`BELOW/ABOVE 55 85`) cut layers; `DrcDesign`s built by
`LayerShapes::from_rects` — cut rects carry `net`/`group`.

- `ENCLOSURE_RULES` metadata pin: 9 entries — ids, verbatim names,
  cut/metal/side/mode.
- **Both-dir boundary** (`met1.4`): metal covering cut by exactly 30
  on both axes → pass; 29 on one side → 1 violation with
  `failed == Both`.
- **One-dir rescue** (`met1.5`): enc_x 30, enc_y 60 → pass (y ≥ 60);
  enc 59/30 → violation `failed == One`.
- **`Full` clauses** (`via.5a` name on `via`/met1): enc 55/55 → both
  pass? No — 55 ≥ 55 both-dir, max(55,55) < 85 → 1 violation
  `failed == One`. enc 40/90 → both-dir fails → `failed == Both`.
- **Union coverage**: two same-net rects each covering half of a side
  slab tile the overhang → pass; the same rects on a *different* net
  → violation (net filter).
- **Protrusion**: cut edge uncovered → negative `enc_*`, violation.
- **Wire-provided overhang**: via's own metal under-encloses but a
  same-net wire covers the slab → pass.
- **Multi-cut array**: 3 cuts under one metal rect overhang 30 on
  both axes → only the 2 outer cuts can flag when overhang drops on
  the outer side; interior cuts' side slabs are metal-covered.
- **Absent layer** (`via2`/`via3`/`via4`/`mcon` without rules or
  layers) → contributes nothing, no panic.

### `drc/union.rs`

- `components`: net-agnostic touch merge — different-net touching
  rects → one component; `MacroObs` never a member; separate islands
  → separate ids. Corner-only touch → same component.
- `union_area`: two overlapping rects dedupe (a∪b, not a+b);
  disconnected sum; exact arithmetic on i64.
- `enclosed_voids`: donut of 4 rects in a window → exactly 1 void
  with the inner area; C-open shape → 0 (reaches boundary); two
  stacked donuts → 2; point-pinched channel (void exits through a
  corner) → still a hole; foreign rect inside the void shrinks its
  area.

### `drc/area.rs`

- `AREA_RULES`/`HOLE_RULES` metadata pins (6 + 2, verbatim names).
- **Min-area boundary**: island area `bound` → pass, `bound − 1` → 1
  violation (`met1.6` on a 140-bound... pick a rect set with
  `union_area == 83000` and `== 82999`).
- **Drawn-member gate**: a `MacroPin`-only island of small rects →
  skipped (no candidate, no violation); add a wire rect touching →
  evaluated.
- **Net-agnostic merge**: two sub-bound different-net rects touching
  → one component above bound → pass (documents the plane model).
- **Hole fixture** (the acceptance's synthetic): met1 ring
  `100×100` void → area 10000 < 140000 → `met1.7`, `failed` region =
  void bbox; `200×700 = 140000` void → pass (exact bound). Also
  `met2.7` on a met2 ring.
- **OBS discard**: same ring + a `MacroObs` rect touching the void →
  no violation; obs fully outside → still flags.
- **Absent LEF bound** (`area_dbu2 == 0`) → rule skipped.

## Integration tests — extend `tests/drc.rs`

1. **`test_enclosure_zero_violations`** — `check_enclosure_report`
   on both designs: all violations empty; census print per rule
   (layer, bound, cut-rect candidates, violations). `met1.4`/`met1.5`
   are expected 0-candidate (no mcon shapes — real coverage named per
   AGENTS §7.4; exercised by unit fixtures). Deterministic double-run
   on the report like the spacing test.
2. **`test_enclosure_rules_join_golden`** — every `rule_name` ∈
   `magic_rules_checked.names` verbatim, `classification ==
   "lef_expressible"`; no `magic_per_rule` key carries an implemented
   id (`paren_tokens` helper already shared).
3. **`test_area_zero_violations`** + **`test_area_rules_join_golden`**
   — same pattern for `check_area_report` (candidates = evaluated
   components; `li.6` expected 0 — li1 has no drawn members) and
   `AREA_RULES`.
4. **`test_hole_zero_violations`** + **`test_hole_rules_join_golden`**
   — `check_hole_report`; census prints enclosed voids detected
   (likely 0 — record honestly).
5. **`test_enclosure_mutation_def_via_enclosure`** (gcd) — mutate the
   committed `VIAS` record `via2_3_1600_480_1_5_320_320`'s
   `+ ENCLOSURE 85 165 55 85` → `+ ENCLOSURE 10 10 55 85` (single
   `replacen` on the ENCLOSURE text, asserted changed). The met1
   (BELOW) side drops to 10/10 < 55 → `via.5a - via.4a` flags on
   every instance's 5 cuts (155 = 31 instances × 5 expected — assert
   on rule set and nonzero, print count); met2 side stays 55/85 →
   clean. Assert: nonempty, **all** violations `rule == "via.5a"`,
   `failed == Both`, no other rule fires; unmutated control → 0.
6. **`test_area_mutation_met1_island`** (gcd) — append a `+ RECT
   met1 ( x y ) ( x+140 y+140 )` stub to an existing net's record
   (coordinates chosen ≥ ~2× spacing from all met1 shapes — found by
   scanning the assembled design during implementation, recorded in
   `results.md`). 140×140 = 19600 < 83000 → exactly `met1.6`, no
   width flag (140 ≥ 140), no spacing pair. Assert every violation
   has `rule == "met1.6"`; control → 0.

The hole fixture stays unit-level (synthetic) per the acceptance —
"flags `met1.7` or `met2.7`".

## Fail-proofs (per family, recorded in `docs/results.md`)

- **Enclosure**: flip the clause comparison `enc >= b` to `enc > b`
  — real generated vias sit *exactly* at the LEF bound (`via2_3`
  met2 side 55/85, `via3_4`/`via4_5` edges) → the zero gate goes red;
  restore. The DEF-VIAS mutation is the flag-positive control.
- **Min-area**: disable the component merge (each rect its own
  component) → wire segments below bound flag `met*.6` → red;
  restore.
- **Holes**: drop the "touches window boundary ⇒ exterior" test or
  the OBS discard — whichever turns real-design free space red is
  the recorded one; the donut unit fixture is the flag-positive
  control. Record which break was used and the count.

## Verification commands

```
cargo test -p grus-core drc
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

picorv32's routed DEF is gitignored — `generate_ref.sh` step 5b if
absent (`require()` panics loudly; no skips).

## Risk register

- **LEF-tighter bounds vs Magic** — our enclosure bounds exceed every
  Magic `surround` value (55/85 vs 30, 65 vs 25/5, 310 vs 120). If a
  0-gate trips, dump the cut's `group`, `net`, measured `enc_*` and
  the covering rects into `results.md` *before* touching code — most
  likely a measurement/merge gap, not the bound.
- **Min-area false flags on stub islands** — DEF `RECT` stubs and
  square-cap ends merge into the wire island by closed-touch; if
  small components flag on a real design, check whether a
  `MacroPin`-adjacent island lost a member to the drawn-member gate.
- **Hole false flags on cell regions** — voids between cell outlines
  are the main risk; the OBS discard must be the mechanism that keeps
  the gate at 0. If it trips, record the void bbox + nearby obs rects
  in `results.md`.
- **Sweep determinism** — all sweeps iterate sorted coordinates and
  stable orders; assert deterministic double-run in the integration
  tests.
- **Performance** — picorv32 met1 ~144k rects: per-component sweeps
  are bounded, but the rail component spans the die; if the drc suite
  regresses badly on debug, profile `enclosed_voids`' active-set
  recompute per slab (O(a log a)) before optimizing.

## Files

- `crates/grus-core/tests/drc.rs`
- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/drc/enclosure.rs`
- `crates/grus-core/src/drc/union.rs`
- `crates/grus-core/src/drc/area.rs`
- `docs/results.md`
