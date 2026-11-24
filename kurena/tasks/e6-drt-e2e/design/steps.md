# e6-drt-e2e — implementation steps (r2)

Ordered from checkpoint `d8d0c6c`. Container steps need the pinned
LibreLane image via Colima (`AGENTS.md` §2). The review memo's order
is preserved: measure the mask first, fix pin access second, then
the remaining pipeline.

1. **Mask verification (the memo's key measurement)** —
   `tests/designs/check_drt_ours.sh gcd` in the container on the
   committed checkpoint DEF + existing emit log, **before** any
   script or code change touches their formats. Unrouted nets emit
   no wires, so this answers "are the wires we emit clean?" in
   isolation: `lef_expressible` ours == 0 on all 50 and
   `unclassified` ours == 0 on the two pinned names is the expected
   verdict; anything else is a DRC finding to chase *before*
   connectivity work. Record the per-rule result as a measurement
   entry in `docs/results.md` — do **not** commit the JSON: the DEF
   will change under step 3 and the golden's `input_def_sha_prefix`
   must match the final committed DEF.
2. **Diagnostic census** (`pin-access.md` step 0): `unrouted_kinds`
   emit line + `DRT_DEBUG_NET` blocking-`Occupant` attribution +
   `check_drt_ours.sh` keep-list/golden field. Lands after step 1 so
   the schema addition never meets a log that lacks the line.
3. **Pin access** (`pin-access.md` ladder): P1 multi-candidate
   relaxed vias → measure; P2 cover-scoped masked-layer extension →
   measure; P3 sibling-clearance filter only if still starved.
   Iterate locally with `cargo test -p grus-core --test drt`
   (`test_emit_roundtrip_gcd` is the unrouted→0 tripwire, currently
   red) and fresh emits; `DRT_DEBUG_NET` on remaining pockets.
   Success: unrouted back at/near 0 with residuals named.
4. **Regenerate + score gcd**: `generate_drt_ours.sh gcd --verify`
   (`OURS-DETERMINISTIC <sha>` printed), then `check_drt_ours.sh gcd`
   in the container. Commit `gcd.drt.ours.routed.def` +
   `gcd.drt.ours.reference.json` iff the gate reads clean
   (`lef_expressible` all-zero, `unclassified` ours==0, magic_only all
   `inherited`). Rerun the exact committed scripts and `git diff` the
   outputs before committing (E0 lesson 1).
5. **picorv32**: `generate_drt_ours.sh picorv32` (multi-hour detached
   leg — start it as soon as step 3's fix lands, overlap with steps
   6–8) + `check_drt_ours.sh picorv32`; commit
   `picorv32.drt.ours.reference.json` (its routed DEF stays
   gitignored). Residual unrouted/magic_only differences named.
6. **Gate tests** (`tests/drt.rs` — `gate.md`): the three new tests +
   re-pins (`test_emit_roundtrip_gcd` restored to empty or a pinned
   residual name list, `test_maze_census` classes, `hull_masked > 0`
   arm). Run the `#[ignore]`d picorv32 legs manually — record which
   arms ran.
7. **Docs** (`docs.md`): `e6-drt.md` §7 inventory update + e2e
   section + claim boundary, `results.md` dated entry (RED first
   contact → mask-verification → pin-access fix → green table →
   delta block), `status.md`/`roadmap.md` lines.
8. **Version bump** (`Cargo.toml` workspace `patch` — behavior fix)
   in the same commit as the functional change.
9. **Final**: `cargo test --workspace`, clippy `-D warnings`,
   `git diff` the committed goldens vs fresh regeneration.

## Decision points

- If step 1 reads dirty on `lef_expressible`: the hull model has a
  correctness bug — chase per-rule (sample bboxes against the DEF)
  before touching connectivity. `magic_only` non-inherited rows and
  the two unclassified names get the same treatment.
- If step 3 lands unrouted > 0: the residual list is the pinned
  gate expectation (`gate.md` §4) and the `status.md` line names the
  class — acceptable only as a *named, measured* landing.

## Files

- `crates/grus-core/src/drt/{graph,maze}.rs`, `crates/grus-eda/src/main.rs`
- `tests/designs/check_drt_ours.sh`
- `tests/designs/gcd/gcd.drt.ours.routed.def`
- `tests/designs/{gcd,picorv32}/<d>.drt.ours.reference.json`
- `crates/grus-core/tests/drt.rs`
- `docs/e6-drt.md`, `docs/results.md`, `docs/status.md`,
  `docs/roadmap.md`, `Cargo.toml`
