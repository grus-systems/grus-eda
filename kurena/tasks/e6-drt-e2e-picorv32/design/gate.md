# e6-drt-e2e-picorv32 — integration gate (r1)

All changes land in `crates/grus-core/tests/drt.rs`, additive to
the `ours_reference_gate` machinery the gcd leg already exercises.
**No new test re-routes picorv32 in the default suite** — dev-profile
picorv32 route time is hours; everything that routes it stays
`#[ignore]`d. The un-ignored test is a pure committed-JSON join
and costs seconds.

## 1. `test_e2e_ours_reference_picorv32` — un-ignore

Drop `#[ignore]`; the body stays `ours_reference_gate("picorv32")`.
Update the doc comment: the golden is committed now, so the test
runs the same `gate.md` §1 joins as gcd — `lef_expressible`
completeness + the §0 pinned join, `magic_only` all-inherited,
the two-name unclassified allowlist, the sanity-control liveness
asserts, the `cli_lines`-evidence census joins, the
`openroad.drt_report_source` pin, and the viability floor
(`signal_nets_routed == routed == 6,811 − unrouted_count`).

Joins that get real picorv32 data for the first time:

- `hull_masked > 0` — picorv32's hull/mask census is an order of
  magnitude up; still nonzero by construction.
- `unrouted` == `cli_lines` set == `unrouted_count` — a
  ~100-entry list (last emit: 116) exercises the join at scale.
- `unrouted_kinds` sum ≥ `unrouted_count` (per-pin failures —
  the v0.12.4 maze census pinned 522 pins across 108 nets).
- `stub_outside_cover` lines ≥ `Unattachable` (records accumulate
  across rip-up iterations — the `_165_` re-attach precedent).
- `notch_residuals`/`hole_residuals` == their `cli_lines` counts.

## 2. Per-design `PINNED_LEF_RESIDUALS` — required refactor

The const is `&[(&str, i64)]` and `ours_reference_gate` applies it
to whichever design's golden it joins. A picorv32-only pin
(say `("li.6", 3)` for the named `area_residuals` class) would
assert `ours == 3` on **gcd's** `li.6` too — gcd sits at 0 and
goes red. Scope pins per design:

```rust
/// (design, rule, count) — a pinned residual is ours-only on that
/// design's golden; `placed` firing the same rule means the pin is
/// wrong, not inherited. Empty since the r5 met1.2 attribution
/// closed all 7 gcd sites — a residual reappears only by
/// re-pinning it here with a named reason and site.
const PINNED_LEF_RESIDUALS: &[(&str, &str, i64)] = &[];
```

Lookup becomes
`PINNED_LEF_RESIDUALS.iter().find(|(d, r, _)| *d == name && *r == rid.as_str())`,
and the `pinned_ids` set for the sanity-control filter scopes the
same way. The refactor is behavior-neutral while the list is
empty — it can land in the prep commit ahead of the leg
(`steps.md` §A), so a red leg's pin list is the only delta left
to write.

Pin semantics unchanged: `ours == pin && placed == 0`, asserted
alongside `inherited == (ours == placed)` — a rule where the
placed baseline also fires is `inherited`, never pinnable.

## 3. Determinism leg — decision tree (spec: "if the picorv32 emit
time permits; record the choice")

| arm | what | cost | when |
|---|---|---|---|
| script `--verify` | `generate_drt_ours.sh picorv32 --verify`: second full emit + `cmp` → `OURS-DETERMINISTIC <sha>` | ~2× the route | **default** — run it; record the line in `docs/results.md` |
| test `test_e2e_ours_def_deterministic_picorv32` | fresh `emit_leg("picorv32")` + `write_routed_def`, byte-compare vs the committed DEF | hours — `#[ignore]`d always | **only if** `picorv32.drt.ours.routed.def < 1 MB` and commits (`emit-leg.md` §4); skipped otherwise — `require()` on an absent file is a loud failure, not a determinism check |
| existing `test_maze_determinism_picorv32` | two in-process `route_nets` runs, `PartialEq` on `RouteDb` | already shipped `#[ignore]`d | corroborating evidence — cite, don't re-run unless asked |

Default outcome (DEF ≥1 MB): the committed tree carries no
picorv32 byte-compare test; determinism evidence is the `--verify`
sha + the named existing ignored test, recorded as the choice in
`docs/results.md` and `docs/e6-drt.md` §9. If `--verify` was
dropped for wall clock, the record names the first emit's
`route_ms` and the projection, not a blank "skipped".

## 4. Fail arms / fail-proof

- The picorv32 leg *is* the measurement — a `lef_expressible`
  finding on our DEF fails the strict `ours == placed` join by
  construction, which is what makes the pinned arm honest.
- Per-design pin check (E0 rule 7): with a pin in the list,
  temporarily point its `design` field at `"gcd"` — gcd's gate
  must go red on the moved pin (`ours=0 != pin` or the
  `placed == 0` arm). Revert. Record that this red-check ran.
- `sanity_control` asserts are design-agnostic and stay live on
  picorv32 (`violated_rules_in_checked_deck`, non-pinned
  `new_rules_vs_clean` non-empty).
- The `unrouted_kinds` script fix (`script-fix.md`) is
  script-side; the gate's `j_obj` already treats `null` as a
  loud failure — no test-side change needed for card #44.

## Files

- `crates/grus-core/tests/drt.rs`
