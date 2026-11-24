# e6-drt-e2e — steps (r8, verify-retry; head `8d96fd7`)

**All steps landed; `5d1502b` is the implementation head.** The r8
memo adds exactly one revision step — step 0 below (`verify-timing.md`,
the background-suite start) — and re-confirms nothing else is left to
implement. `5d1502b` carries only
doc-sync and the `LineNodes`/`PaintLines` alias refactor on top of
`0d113ba`'s final hop — **no functional delta**: the committed DEF
(`635f19a9aae86a44`), the golden and every measured number below are
unchanged — over the `d8d0c6c`/`4c5e2cb`/`b372d5e`/`fba5f85`
mechanism checkpoints.

Container: image `ghcr.io/librelane/librelane:3.0.16` cached locally
(`image_id sha256:02d50f259d483979e126ef188a50f5b61e1d285b9d538312f280b087439b0302`
— recorded in the golden). Probe artifacts live in the gitignored
`tests/designs/gcd/runs/drt-ours/` workdir (`who1`–`who7.tcl`,
`region.mag`, `gcd_flat.mag`, mutation DEF).

## A. met1.2 attribution — LANDED (X3 arm fired, all 7 sites closed)

`met12-attribution.md`'s protocol ran: `who6.tcl` (site D — `_131_`
inside `_338_` `a22o_2`) and `who7.tcl` (site E — `_122_` inside
`_321_`, same cell) ran `drc why` + flattened tile dumps in-container.
The partner edge was **our own fill paint, not foreign or invisible
geometry** — every site classified **X3** (within-island notch our own
scan produced), not X1/X2:

- The `L1M1`/`M1M2_PR` met1 plates of one attach faced across a 5–15
  DBU gap; the emitted `RECT` fill stopped 20–30 DBU **short of the
  facing band** (e.g. `_131_`: fill emitted `(47436,25925)-(47451,26125)`
  while the plates faced down to y=25895).
- Root cause: `try_fill`'s mask branch decomposed the candidate gap
  against `fin − cover_rects` blockers on *every* masked layer, but
  met1 is `hull_host_open` — `rect_allowed` already admits the whole
  member-hull interior there, so the li1 pin-cover rect must not carve
  the fill. `if mask.host_open(l) { continue; }` skips the carve
  (`maze.rs`); li1 stays cover-strict.
- Post-fix `_131_` emits `(47436,25895)-(47451,26125)` — spans the
  facing band. `met1.2` 32 → 0, `notch_fills` 221 → 343, the `_099_`
  `notch_residual` closed by the same arm, `routed` 187 → 188 /
  `unrouted` 82 → 81. `PINNED_LEF_RESIDUALS` is **empty** — no pin was
  needed (the X1/X2 pin arm never fired).

## B. Golden regeneration — LANDED

One fresh full `check_drt_ours.sh gcd` run (steps A–E, no assemble-only
arm exists) emitted the committed `gcd.drt.ours.reference.json` on the
regenerated committed DEF — `input_def_sha_prefix=635f19a9aae86a44`
matches `sha256(gcd.drt.ours.routed.def)`. It supersedes the 20:07
`fba5f85` transcripts whose step-E output was lost to the driver
timeout (recorded in `docs/results.md`). Never hand-edited, never
rebaselined.

## C. Test work — LANDED (`crates/grus-core/tests/drt.rs` + `maze.rs::tests`)

- `load_ours_ref`/`load_drc_ref` helpers (`require`'d — loud failure,
  never skip) + `magic_rule_id`/`j_*` extractors.
- `ours_reference_gate(name)` — the shared join:
  - `lef_expressible` key set == the `drc.reference.json`
    `classification` `lef_expressible` subset (50 rules, derived not
    constant); per rule strict `ours == placed`, or the
    `PINNED_LEF_RESIDUALS` arm (`ours == pin && placed == 0`) —
    currently empty; `inherited == (ours == placed)` asserted.
  - `magic_only` every row `inherited`; `unclassified_fired` ⊆ the
    two-name `ALLOWED_UNCLASSIFIED`; `sanity_control`
    `violated_rules_in_checked_deck` + non-pinned `new_rules_vs_clean`
    non-empty (vacuous-control guard).
  - `ours_router`: `hull_masked > 0`; `notch_residuals` /
    `hole_residuals` == their `cli_lines` evidence-line counts;
    `unrouted` == its `cli_lines` set == `unrouted_count`;
    `unrouted_kinds` sum ≥ `unrouted_count` (per-pin failures);
    `unrouted_blockers` all-positive; `stub_outside_cover` lines ≥
    `Unattachable` (records accumulate across iterations — `_165_`
    re-attached, so ⊆-unrouted is wrong by construction).
  - `openroad.drt_report_source == "not_applicable_native_router"`;
    viability floor `signal_nets_routed == routed == in_def − unrouted`.
- `test_e2e_ours_reference_gcd` — runs the gate.
- `test_e2e_ours_def_deterministic_gcd` — fresh `emit_leg` +
  `write_routed_def` byte-equals the committed DEF.
- `test_emit_roundtrip_gcd` — re-pinned: `report.unrouted` (BTreeSet)
  == golden `ours_router.unrouted` (the 82→81 drift names nets in the
  failure, never a re-baselined count).
- `test_emit_selfdrc_gcd` — strict: `findings` + `area_residuals` +
  `notch_residuals` + `hole_residuals` all empty.
- `test_closure_reject_droppable` / `test_closure_notch_residual` —
  the two previously-uncovered closure arms (Reject on a droppable
  `ProjStub` prong; named `NotchResidual` on undroppable
  `Tree`/`PinShape` prongs over a walled gap).
- `test_e2e_ours_reference_picorv32` — `#[ignore]`d, same joins.

## D. Golden + DEF commit — LANDED

`gcd.drt.ours.routed.def` regenerated post-fix and committed;
`gcd.drt.ours.reference.json` committed with every `ours` count at 0
or inherited — no pins needed, no unexplained counts.

## E. picorv32 leg — DEFERRED (memo), honestly open

Un-run by design. No `picorv32.drt.ours.routed.def`, no golden;
`docs/results.md` states the deferral; the `#[ignore]`d test fails
loudly on `load_ours_ref` under `--ignored`. The acceptance's "both
designs" item remains open — flagged on the card, not silently green.

## F. Docs + version — LANDED

- `docs/results.md` — dated 2026-11-24 entry: trajectory
  ~3025 → 847 → 514 → 317 → 305/0, the X3 attribution record with
  `who6/who7` probe names, gate table, connectivity record (81
  unrouted named via the golden join, `stub_outside_cover` semantics
  corrected — accumulated across iterations), comparison table with
  the wirelength-rise explanation (fills unlocked → occupancy shift →
  +1 routed net; fills are `RECT` paint, not wire), picorv32
  deferral, full what-ran per E0 rule 9.
- `docs/e6-drt.md` — new §9 e2e section (script → schema → gcd
  outcome → gate tests) + §8 self-DRC scope correction (island-merge
  blind spot = the measured met1.2 class).
- `docs/status.md`, `docs/roadmap.md` — honest E6 line (unrouted=81
  carried, no "fully routed" claim, deferral named).
- `Cargo.toml` `0.12.6 → 0.12.7` (patch — bug fix) in the same commit.

## Verify / diffreview checklist (what remains)

0. **T0 — start `cargo test --workspace` detached** per
   `verify-timing.md` (nohup → `tests/designs/gcd/runs/
   cargo-test-workspace.log`, pid recorded). The r7 verify died on
   the driver's 1800 s window at ~112 CPU-min, not on a test
   failure; the background run gets the suite's full ~2h. Its
   collected log (`test result: ok.` lines + exit status) is the
   `cargo test --workspace` evidence — an in-window verify
   invocation that still times out is the known-infra limit, and
   the `--release`/`workflow.toml` alternative is flagged for the
   human, never edited by us.
1. `cargo test --workspace` — expect green; the picorv32 leg stays
   `#[ignore]`d (a `--ignored` run fails loudly on the missing
   golden — that is the designed state, not a bug).
2. `cargo clippy --workspace --all-targets -- -D warnings` — clean
   (in-window; compile-only, it fits).
3. Golden-vs-DEF join: `sha256(gcd.drt.ours.routed.def)[:16]` ==
   golden `input_def_sha_prefix` (`635f19a9aae86a44`) — verified in
   planning; re-check in verify.
4. Diff audit vs this design (`dev...5d1502b`): `maze.rs` delta is
   the `host_open` carve skip + `stub_outside_cover`/`stub_blocked`
   comment corrections + `ClosureResult` `Debug` + two fixtures +
   the `5d1502b` pure refactor (`LineNodes`/`PaintLines` aliases,
   `#[allow(clippy::too_many_arguments)]` on `access_vias`) —
   nothing else moved on top of the documented mechanism files
   (`graph.rs` hull mask/`PinCover`, `emit.rs` `RECT` emission,
   `union.rs` `rect_minus`/`enclosed_voids`, `main.rs` census
   lines); `drt.rs` delta is additive gate/tests plus the two
   re-pins; no mask/spacing/hard-block relaxation anywhere (the
   non-goals audit).
5. Golden audit vs `golden.md` schema: no absolute paths in
   `cli_lines`, no wall-clock fields, `image_id` recorded not gated,
   `count_flat` never claimed as a signal.
6. `docs/results.md` numbers must equal the golden's (they were
   transcribed from the same file) — spot-check the comparison table
   against `delta.vs_*`.

## Files (as actually touched on the branch, head `5d1502b`; r8 adds no committed paths)

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/tests/drt.rs`
- `tests/designs/gcd/gcd.drt.ours.reference.json`
- `tests/designs/gcd/gcd.drt.ours.routed.def`
- `docs/results.md`, `docs/e6-drt.md`, `docs/status.md`, `docs/roadmap.md`
- `Cargo.toml`, `Cargo.lock`
- `tests/designs/picorv32/picorv32.drt.ours.reference.json` (deferred — intentionally absent)
- `tests/designs/gcd/runs/` (r8, gitignored — `cargo-test-workspace.log`, `cargo-test-workspace.pid`; never committed)
