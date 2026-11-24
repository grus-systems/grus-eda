# e6-drt-e2e — steps (r3, from checkpoint 4c5e2cb)

Scripts landed, mask model landed, WIP measurement committed. Order
is the memo's: fix emit geometry until Magic reads clean, then
recover connectivity, then finish the pipeline. Each leg re-runs
the container check and records the delta — a step is done when its
measured number moves, not when code compiles.

Container: image `ghcr.io/librelane/librelane:3.0.16` is cached
locally; start `colima start --memory 12 --cpu 8` if the daemon is
down. Probe artifacts from the root-cause work (`who*.tcl`,
`region.mag`, `gcd.whofix.def` in `tests/designs/gcd/runs/drt-ours/`)
are in the gitignored runs dir — keep for reference, they carry no
commit obligation; note their existence in `docs/results.md` so the
evidence trail is reproducible.

## A. Same-net self-spacing closure + met1 confinement (`self-spacing.md`)

1. Union-exact pin cover in `graph.rs`: merge each cell's cover
   rects into components (overlap-chain); `rect_allowed`/`seg_allowed`
   covered-part test = residual-empty after `rect_minus` over the
   cell's cover union.
2. Closure pass in `route_one_net` extraction (after hlines/vlines/
   vplaced merge + pin attach, before `NetOutcome::Routed`): per
   layer, pairwise facing-edge disjointness < `s_l`; fill = gap-span
   rect (±1 into prongs) admitted iff `occ.scan_rect`+`exempt_obs`
   clean and `rect_allowed` passes; iterate to fixpoint; emit via
   new `NetRoute.fills` → `NetGeom.rects` → `RECT` DEF records;
   rasterize in `commit_route`. Fallback: reject the emitting
   access/stub element, next candidate (`self-spacing.md` Rule B).
3. Assert pin-touch paint ⊆ cover-union on masked layers (mcon +
   pin-side stub) — Rule A's cheap half; transit stays host-open
   `≥ s` (no change needed — occupancy already enforces).
4. `notch_fills=<n>` appended to the `guided=` census line; script
   regex + `ours_router` field (`scripts.md`).
5. Re-emit gcd + `check_drt_ours.sh` → target `lef_expressible`
   ours == placed on all 50 rules (i.e. met1.2/met2.2 → 0). Any
   residual site gets the `drc why` + flattened-dump treatment;
   the strict-confinement escalation (`self-spacing.md`) is the
   named next rung, not a parallel attempt.
6. `cargo test --workspace` between legs; re-pin digests only to
   measured values.

## B. Access-model expansion (`pin-access.md`)

7. Land rungs A1–A4 in order, re-emit after each, record `unrouted`,
   `unrouted_kinds`, `unrouted_blockers`, `stub_outside_cover` per
   rung in `docs/results.md`. Stop when `unrouted` is empty or the
   residual is provably unattachable (named per net + blocking class).
8. `stub_outside_cover` code comment updated to the r3 semantics
   (`golden.md`); the emit keeps printing it.

## C. Golden + DEF commits (only when clean)

9. `generate_drt_ours.sh gcd` → commit `gcd.drt.ours.routed.def` iff
   the emit census reads clean (or unrouted == named residuals only).
10. `check_drt_ours.sh gcd` in-container → `lef_expressible` all
    `inherited`, `unclassified_fired` empty-or-pinned,
    `sanity_control.new_rules_vs_clean` non-empty → commit
    `gcd.drt.ours.reference.json`. **Never rebaseline** — a RED gate
    is chased, not committed (the WIP 514 file is superseded, not
    amended).

## D. picorv32 leg

11. `generate_drt_ours.sh picorv32` + `check_drt_ours.sh picorv32`
    (the `[design]` arg path already exists; picorv32 DEF is not
    committed per the existing comment). Commit
    `picorv32.drt.ours.reference.json` iff clean or with residuals
    named in `docs/results.md` (the `gate.md` #[ignore]d test takes
    the pinned values).

## E. Gate tests + docs + verify

12. `drt.rs` additions per `gate.md`; restore `test_emit_roundtrip_gcd`.
13. `docs/results.md` full dated entry (trajectory 3025→847→514→final,
    root-cause narrative, rung-by-rung numbers, delta tables,
    what-ran per E0 rule 9 — name partial reruns);
    `docs/e6-drt.md` e2e section + §7 self-DRC scope correction;
    `docs/status.md` + `docs/roadmap.md` E7-style honest line.
14. `[workspace.package] version` bump in root `Cargo.toml` in the
    same commit as the functional change (patch — bug-fix class).
15. `cargo test --workspace` + `cargo clippy --workspace -- -D warnings`
    clean before the card.

## Files

- `crates/grus-core/src/drt/graph.rs`
- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt/emit.rs`
- `crates/grus-eda/src/main.rs`
- `crates/grus-core/tests/drt.rs`
- `tests/designs/check_drt_ours.sh`
- `tests/designs/generate_drt_ours.sh`
- `tests/designs/gcd/gcd.drt.ours.routed.def`
- `tests/designs/gcd/gcd.drt.ours.reference.json`
- `tests/designs/picorv32/picorv32.drt.ours.reference.json`
- `docs/results.md`, `docs/e6-drt.md`, `docs/status.md`,
  `docs/roadmap.md`
- `Cargo.toml`
