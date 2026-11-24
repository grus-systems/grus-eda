# e6-drt-e2e — steps (r5, from checkpoint fba5f85)

Code landed, routed DEF committed (`sha256 1534c323855a6029`), the
post-closure measurement ran at 20:07 — **but the golden JSON was
never persisted**: the driver timed out between the Magic runs and
step-E assembly, and `tests/designs/gcd/gcd.drt.ours.reference.json`
in the worktree is byte-identical to the stale committed golden
(`count_total=514`, `met1.2=740`, `met2.2=56`,
`input_def_sha_prefix=6671d184`, `unrouted=73` — measured on a DEF
that no longer exists). **Do not commit the file as it sits.** The
317/`met1.2=32` numbers live only in the gitignored run logs
(`ours_magic.log`, `placed_magic.log`, `broken_magic.log`,
`antenna.log`, `mutation.json` — all 2026-11-24 20:07). This
revision is finish-the-record in the memo order: attribute the 32,
regenerate the golden via one fresh script run, close the test gap,
document, commit.

Container: image `ghcr.io/librelane/librelane:3.0.16` is cached
locally; start `colima start --memory 12 --cpu 8` if the daemon is
down. Probe artifacts live in the gitignored
`tests/designs/gcd/runs/drt-ours/` dir (`who1`–`who5.tcl`,
`region.mag`, `gcd_flat.mag` extraction from r3 — reuse the workdir).

## A. met1.2 attribution (`met12-attribution.md`)

1. Site table: **already decomposed** — 32 fragments → 7 sites
   (`_099_` named + 6 fill-adjacent sites on `resp_val`, `_084_`,
   `_131_`, `_122_`, `_070_`, `_066_`); per-site DEF anatomy is in
   `met12-attribution.md` §1–§2. No re-parse needed.
2. In-container `drc why` + flattened tile dump on **≥2 sites**
   (D `_131_` — the D/F repeated y-band pattern — and E `_122_` —
   different fragment shape): name the partner edge's owner + class
   (X1–X4). `who6.tcl`+ convention; name the files in results.md.
3. Decision per `met12-attribution.md` §4:
   - **X3/X4 (closure gap)** → bounded fix (≤2 iterations): extend
     the scan, add a unit fixture reproducing the site pattern,
     re-emit, re-run `check_drt_ours.sh gcd`, regenerate the golden.
     The 317 measurement is then superseded — record its numbers in
     the trajectory, commit only the final measurement.
   - **X1/X2 (invisible geometry)** → pin `("met1.2", measured)` in
     `PINNED_LEF_RESIDUALS` with every site named; the §B golden
     lands as measured.
   - **Mixed** → fix first, then pin survivors.

## B. Golden regeneration

4. **Regenerate**: run `tests/designs/check_drt_ours.sh gcd` fresh in
   the pinned image (steps A–E on the committed DEF — there is no
   assemble-only arm, and a full run re-verifies the placed baseline
   and the mutation control in the same pass). The committed script
   is the only emitter of the golden — never hand-write the JSON.
   If the rerun diffs from the 20:07 measurement (317 total,
   `met1.2=32`, `routed=187 unrouted=82`) on the same DEF + image,
   that diff is a finding to record in `docs/results.md`, not to
   silently ship. If step E aborts, read its message — the
   assembler's strict expectations name the real mismatch; fix the
   flagged issue in the script, don't rewrite it.
   This precedes the test work because §C's gate tests join against
   the committed golden — running them against the stale file would
   fail on the sha/count joins for the wrong reason.

## C. Test work (`gate.md`)

5. Two maze.rs fixtures: `test_closure_reject_droppable`,
   `test_closure_notch_residual` (the two uncovered arms).
6. `drt.rs`: `load_ours_ref` helper + `PINNED_LEF_RESIDUALS`;
   `test_e2e_ours_reference_gcd`; `test_e2e_ours_def_deterministic_gcd`
   (byte-equal vs committed DEF); `test_emit_roundtrip_gcd` unrouted
   → golden-join re-pin; `#[ignore]`d
   `test_e2e_ours_reference_picorv32`.
7. `cargo test --workspace` between legs; `generate_drt_ours.sh gcd
   --verify` covers byte-stability script-side.

## D. Golden commit

8. Commit `gcd.drt.ours.reference.json` iff every nonzero `ours`
   maps to a named site **and** `input_def_sha_prefix` matches the
   committed DEF (`1534c323855a6029` — re-verify if a fix re-emits).
   Routed DEF is already committed at `fba5f85`; recommit only if
   step 3's fix changes it. Never rebaseline.

## E. picorv32 leg — deferred (memo)

9. `generate_drt_ours.sh picorv32` / `check_drt_ours.sh picorv32`
   stay un-run; `docs/results.md` states the deferral explicitly
   (E0 rule 9). The `#[ignore]`d gate test keeps the joins ready;
   `picorv32.drt.ours.reference.json` is *not* committed — the
   acceptance's "both designs" item is honestly open, flagged on
   the card.

## F. Docs + verify

10. `docs/results.md` dated r3 entry per `docs.md` (trajectory
    ~3025 → 847 → 514 → 317; 7-site table + probe record; 82 unrouted
    named via the golden join; emit census verbatim; delta tables;
    picorv32 deferral; what-ran per E0 rule 9).
    `docs/e6-drt.md` e2e section + self-DRC scope correction;
    `docs/status.md` + `docs/roadmap.md` honest E6 line.
11. `[workspace.package] version` 0.12.6 → **patch bump** in root
    `Cargo.toml` in the same commit as the functional change (any
    X3/X4 fix or the gate/test work).
12. `cargo test --workspace` + `cargo clippy --workspace
    --all-targets -- -D warnings` clean before the card — expect the
    baseline to be red first (`test_emit_roundtrip_gcd` asserts
    `unrouted.is_empty()` against the measured 82; `gate.md` §4's
    re-pin is the fix).

## Files

- `crates/grus-core/src/drt/maze.rs` (two fixtures; closure
  extension only if X3/X4)
- `crates/grus-core/tests/drt.rs`
- `tests/designs/gcd/gcd.drt.ours.reference.json`
- `tests/designs/gcd/gcd.drt.ours.routed.def` (only if a fix re-emits)
- `tests/designs/picorv32/picorv32.drt.ours.reference.json` (deferred)
- `docs/results.md`, `docs/e6-drt.md`, `docs/status.md`, `docs/roadmap.md`
- `Cargo.toml`
