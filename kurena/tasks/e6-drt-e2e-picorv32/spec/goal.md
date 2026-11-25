# e6-drt-e2e-picorv32: the deferred picorv32 leg

`e6-drt-e2e` merged with the picorv32 leg deferred by memo: the gcd
golden is green (all `lef_expressible` ours=0) but
`picorv32.drt.ours.reference.json` was never generated and
`test_e2e_ours_reference_picorv32` ships `#[ignore]`d — it fails
loudly as designed until this ticket lands.

picorv32 is ~7x gcd and a different cell mix — it is the only
scale-class measurement the gate has. Expect new residual classes:
the r4 emit already showed ~10x unrouted vs gcd, and review items
(`transit_extend` hull-grid scan cost, guide-region escape) are
scale-sensitive. Whatever the leg produces is measured, not
presumed.

## Scope

1. Generate `tests/designs/picorv32/picorv32.drt.ours.routed.def`
   via committed `generate_drt_ours.sh` and the full
   `check_drt_ours.sh picorv32` in-container pass — same steps as
   the gcd leg, nothing hand-built.
2. Commit `picorv32.drt.ours.reference.json` (step-E output,
   `image_id` + `generated_by` + `input_def_sha_prefix` recorded).
3. Un-ignore `test_e2e_ours_reference_picorv32` and pin the gate
   joins to the committed golden — same test structure as the gcd
   gate, including the determinism leg if the picorv32 emit time
   permits (record the choice).
4. `docs/results.md` dated entry: full comparison table vs the
   delegated routed reference, unrouted residuals named per class,
   any new `lef_expressible` findings decomposed like the gcd leg
   was (site anatomy, mechanism, fix-or-pin). A red leg is a valid
   outcome — it becomes the next fix ticket's measurement, never
   a rebaselined golden.
5. Fix the medium card-#44 issue while here: `check_drt_ours.sh`
   `unrouted_kinds` null-vs-fail fallback must match the
   scripts.md/golden.md contract.

## Files

- `tests/designs/picorv32/picorv32.drt.ours.reference.json`
- `tests/designs/picorv32/picorv32.drt.ours.routed.def` (gitignore
  exception if committed — same convention as gcd)
- `crates/grus-core/tests/drt.rs` (un-ignore + pins)
- `tests/designs/check_drt_ours.sh` (unrouted_kinds fix)
- `docs/results.md`, `docs/e6-drt.md` (claim boundary update)

## Verify

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`

Note: heavy — emit+Magic on picorv32 is hours, not minutes. The
`cargo test --workspace` verify needs `timeout_sec` headroom
(now 7200).
