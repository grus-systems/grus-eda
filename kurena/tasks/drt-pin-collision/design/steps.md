# Steps — implementation order, verification, commits

Two commits: (1) the mechanism + fixture (one coupled functional
change), (2) docs + `results.md`. The red-proof run between the
sub-steps of (1) is part of the evidence, not a separate commit.

## Step 1 — census + fixture skeleton (lands inside commit 1; red run)

1. `pin_overlap_census` + `RouteDb.pin_collisions` + `route_digest`
   coverage (`mechanism.md` §6–7).
2. `test_maze_pin_collision` (fixture.md arms A/B) +
   `test_route_digest_covers_pin_collisions`.
3. `cargo test --release -p grus-core --test drt test_maze_pin_collision
   -- --nocapture` → **expected red** on the unfixed verdict:
   arm B reports `pin_collisions == 1` and routes instead of
   failing. Save the output for `results.md`.
   Also run `test_maze_census` + a one-off `pin_overlap_census`
   print on gcd → record the committed-route count (expected 0 —
   `met3.2` ours=0 already).

## Step 2 — the fix (rest of commit 1)

4. `hit_verdict` merge-arms change + `Occupant::Pin`/doc-comment
   rewrite (`mechanism.md` §1).
5. `edge_collisions` directed second sink + `NetOutcome::Routed`
   extra vec (§2).
6. `route_nets`: `prov_pins` collection, wire-only re-queue,
   `IterStat.prov_collisions` count + `main.rs` conditional print
   (§3, §8).
7. Postmortem `or_else` arm gains `Pin` (§4).
8. `project_attach` + `try_pin_paint` Proj stub scans (§5).
9. Re-run the fixture → green on both arms (no assertion edits).
10. `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`.

Commit 1: `fix: Record wire×foreign-pin collisions and hard-block
them on the final DRT iteration` — body enumerates the five sites,
the directed-record rationale (wire is the only movable party), the
red→green protocol, and the version bump (`Cargo.toml`
`workspace.package` 0.12.12 → 0.12.13, patch).

## Step 3 — regression surface (same verification, not a commit)

11. `test_maze_census` (gcd): compare `iter_progress`/`iter_prov`,
    `collisions`, `unrouted`, digest vs the 2026-11-26 baseline
    (`guided=269 routed=188 unrouted=81 collisions=0 iterations=8
    digest=0xa6d8b74a740a7489`). `pin_collisions` must be 0.
    Divergence is not auto-accepted — see §6.
12. `test_maze_occupancy`, `test_maze_determinism`,
    `test_emit_roundtrip_gcd`, `test_emit_selfdrc_gcd` — the
    occupancy/determinism/emit gates must stay green.
13. picorv32 (`--ignored`): `test_maze_census_picorv32` only if the
    gcd leg shows a delta worth explaining — ~2 h release; decision
    recorded either way.

## Step 4 — docs (commit 2)

14. `docs/e6-drt.md` edits — `docs.md`.
15. `docs/results.md` dated entry — template in `docs.md`.
16. `docs/status.md` — only if a named number moved.

Commit 2: `docs: Update e6-drt pin-mark semantics + results entry
for pin collisions`.

## Step 5 — verify

- `cargo test --workspace` green; `cargo clippy --workspace -- -D
  warnings` clean.
- Acceptance checklist: mechanism chosen+rationale (commit body +
  `docs/e6-drt.md`), fixture red-before-fix (results.md), survivors
  named (`pin_collisions`), dated `results.md` entry.

## §6 — Contingencies

- **`iter_prov`/`iter_progress` moved on gcd** (pin hits now
  recorded → re-queue counts change): legitimate semantic change —
  regenerate `gcd.drt.ours.reference.json` via the committed
  `check_drt_ours.sh gcd` + `generate_drt_ours.sh` leg (Colima +
  LibreLane container — **ask first**, billable/Docker). If the
  container leg cannot run this ticket, fall back to keeping
  `prov_collisions` counting `prov` only and document that choice
  instead.
- **`collisions`/`unrouted`/digest moved on gcd**: the verdict
  change altered real routes — inspect before accepting; a routed
  net lost to a `Pin` block is a named `unrouted` entry (acceptable
  iff `blockers` attributes it and results.md names it).
- **`pin_collisions` > 0 on gcd/picorv32 committed routes**: a real
  surviving residual — name each site (net pair, layer, line) in
  `results.md`; acceptance explicitly allows named survivors.
- **`drt-preemit-spacing` lands first**: this ticket stands —
  emit-side detection complements, does not replace, router-side
  prevention (spec note).
