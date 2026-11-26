# e7-drc-spacing — Test plan

Unit tests live in-module (`lef.rs`, `drc/geom.rs`, `drc/spacing.rs`);
design-scale tests extend the existing
`crates/grus-core/tests/drc.rs` (`load(name)` helper, golden access,
`print_census` already in place).

## Unit tests

`readers/lef.rs` (extend the `W_LEF`-style mini fixture):

- `LAYER x \n SPACING 0.19 ;` on a CUT layer → `spacing_dbu == 190`.
- `SPACINGTABLE PARALLELRUNLENGTH 0 WIDTH 0 0.14 WIDTH 3 0.28 ;` on a
  ROUTING layer → `spacing_table` = `[(0,140),(3000,280)]`,
  `min_spacing_dbu() == 140`, `wide_spacing() == Some((3000, 280))`.
- Layer with neither → `spacing_dbu == 0`, `min_spacing_dbu() == 0`,
  `wide_spacing() == None` (rule-skips downstream).
- Non-plain `SPACING` form (`SPACING 0.3 RANGE 3 100 ;` in a fixture)
  → parses through `skip_to_semi` without error, `spacing_dbu` unset.
- Real-tech bound parity (asserted in `drc::spacing` tests on the
  parsed sky130 techlef? — No: the integration `load()` already
  proves them via `check_spacing` results; keep a metadata pin test
  in spacing.rs only).

`drc/geom.rs`:

- Via instances get distinct nonzero `group` ids in chain order
  (signal vias first, then special); wires/pins/ports carry `group ==
  0`. Assert on the existing `DRC_DEF` fixture.

`drc/spacing.rs` — synthetic `DrcDesign`s built by
`LayerShapes::from_rects` (same style as `width::tests`), a mini LEF
with met2 `SPACINGTABLE` 140/280@3000 + `via` cut `SPACING 0.17`:

- `SPACING_RULES` metadata pin: 20 entries, ids / verbatim names /
  layer / cut / kind, including the `met3.3c`/`met3.3d` and
  `met4.5a`/`met4.5b` id irregularities.
- **Gap boundary**: different-net pair, gap `bound-1` → 1 violation;
  gap `bound` → 0; gap `bound+1` → 0.
- **`touching_ok`**: gap 0 (edge abut) → 0; corner touch
  (`gx==gy==0`) → 0; overlapping different-net pair → 0.
- **Euclidean**: `gx=gy=99`, bound 140 → `19602 < 19600` false → 0;
  `gx=gy=98` → `19208 < 19600` → 1. (Proves `drc euclidean on` —
  Manhattan `max` would flag the 99/99 pair.)
- **Merge semantics** (the acceptance's explicit fixture): same-net
  chain `A—B—C` where `A`,`C` are gap-30 apart but both touch `B` →
  same island → 0 violations. Same-net `A`,`C` gap-30 with no
  connector → different islands → **1 violation** (`met2.2`). Same-net
  *abutting* stubs → 0.
- **Via-instance merge**: two rects with shared `group`, gap 30 on the
  `via` cut layer → 0 violations (island merge); the same two rects
  with `group == 0` → 1 (`via.2`). Pins the group mechanism directly.
- **Wide `*b`**: pair (rect `min=3100`, narrow) gap 200 on met1 →
  exactly `met1.3b` (bound 280; base 140 passes). Gap 100 → both
  `met1.2` and `met1.3b` (independent statements). `min` exactly
  3000 → not wide (strict `>`): only `met1.2` can fire.
- **Wide `*a`**: island `{wide plate, narrow stub touching it}` vs
  unrelated narrow at gap 200 → exactly `met1.3a`. Stub moved 281
  from the plate (outside `window` = 280) same gap → 0. Plate vs
  unrelated narrow at 200 → `met1.3b` only, not `3a` (disjointness).
- **Missing layer** (mini LEF without li1/mcon/met3+) → those rules
  contribute nothing, no panic.

## Integration tests — extend `tests/drc.rs`

Reuse `load(name)` and the golden JSON; add `use
grus_core::drc::spacing::{SPACING_RULES, check_spacing}`.

1. **`test_spacing_zero_violations`** (both designs):
   `check_spacing(&l.drc, &l.tech)` empty; print a per-rule census —
   layer, bound, candidate different-island pairs within reach, and
   violations (0) — feeding `results.md`. The census must name rules
   with zero candidates (`li.3`, `li.c2`, `mcon.2` are expected empty
   on both designs — honest coverage statement per AGENTS §7.4; they
   are exercised by unit fixtures).
2. **`test_spacing_rules_join_golden`** (both designs): every
   `SPACING_RULES[i].rule_name` ∈ `magic_rules_checked.names`
   verbatim, `classification == "lef_expressible"`; and no
   `magic_per_rule` key carries any implemented id under the same
   paren-token extraction `test_width_rules_join_golden` uses (pull
   that token split into a shared helper fn in the test file — do not
   duplicate).
3. **`test_spacing_mutation_met2_probe`** (gcd): **golden-driven**
   reproduction of the committed sanity control — read
   `sanity_control.mutation` from `gcd.drc.reference.json`
   (`net_b`, `layer`, `segment_dbu`), insert the identical
   `NEW met2 ( x1 y1 ) ( x2 y2 )` statement into net `_001_`'s
   NETS record in the DEF text (before its terminating `;`),
   re-parse, `check_spacing`. Assert: non-empty violations, **every**
   violation has `rule == "met2.2"` (no other spacing rule fires),
   and at least one pair contains the probe's expanded wire rect —
   `segment_dbu` centerline ± `layer_width_dbu/2` square-capped,
   recomputed from the golden fields (do not hardcode it). Control: unmutated DEF → 0. Magic's
   recorded verdict (`new_rules_vs_clean == ["Metal2 spacing <
   0.14um (met2.2)"]`, 3 bbox fragments) is cited in `results.md` —
   our count is violating rect *pairs*, not merged regions, so the
   assertion is on the rule set, not the count.

## Fail-proof (recorded in `docs/results.md`, per AGENTS §7.7)

Primary break — **disable the merge** (the acceptance's named
control): make `find` return each rect's own id (identity
union-find) → run the same-net island unit fixture (goes red: the
chained `A—C` pair now flags) and the via-group fixture (the two
grouped cuts flag) → and `test_spacing_zero_violations` on gcd (goes
red via via2 array cuts at 170 < 200 and any same-net loop-backs;
record the actual count) → restore, green.

Secondary observation — also break `<` to `<=` on the squared gap iff
it goes red quickly; if bound-exact pairs happen not to exist on the
real designs, record that the merge-disable is the effective
fail-proof and *why* (the `<`→`<=` break is only guaranteed red where
a bound-exact pair exists — via2 CUTSPACING 170 sits under bound 200,
not at it).

## Verification commands

```
cargo test -p grus-core drc
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

picorv32's routed DEF is git-ignored — generate via
`tests/designs/picorv32/generate_ref.sh` step 5b first if absent
(existing `require()` panics loudly; no skips).

## Risk register

- **Tighter via bounds**: our `via.2`/`via2.2`/`via3.2`/`via4.2`
  bounds (170/200/200/800) are 2–4× Magic's cut-plane bounds
  (60/120/80/420) — two *different-net* cuts the router placed closer
  than LEF spacing would flag where Magic's 60/120 didn't. If the
  0-gate trips here, identify the offending pair's `src`/`net` and
  record it in `results.md` before touching anything — do not loosen
  the bound.
- **Wide-metal straps**: met4/met5 power straps ≥ 3µm create real
  `wide_mark` islands; if a strap-to-signal gap under 400 exists the
  `*b` rule fires where `met4.2` passed. Magic-clean says it
  shouldn't, but this is asserted, not assumed.
- **`MacroObs` adjacency**: obs rects excluded from findings but
  present in the layer store; a wire hugging an obs edge is not
  flagged (Magic's GDS-side metal may differ — recorded divergence
  direction: under-report).
- **Determinism**: union-find + pair emission order must be
  input-order stable — `by_x` iteration is index order; assert
  deterministic double-run in the integration test (cheap: compare
  `Violation` vecs on two parses like the existing determinism test).

## Files

- `crates/grus-core/tests/drc.rs`
- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/drc/geom.rs`
- `crates/grus-core/src/drc/spacing.rs`
- `docs/results.md`
