# Steps — implementation order, verification, commits

Two commits: (1) mechanism + fixture + emit suffix + script parser
(one coupled functional change), (2) docs + `results.md`. The
red→green protocol inside (1) is evidence, not a separate commit.

## Step 0 — measure the remaining class (evidence only, no commit)

- `cargo test --release -p grus-core --test drt test_pin_attachment
  -- --nocapture` and `test_maze_census` on gcd: capture the
  baseline — `region_escape` names, `unrouted=81` split
  `NoPath/Unattachable`, `stub_outside_cover` count, digest.
- `DRT_DEBUG_NET=_134_` / `_138_` route probe: name exactly which
  layer lacks nodes for `_301_/B1` / `_307_/B1` (expected: li1 pin
  rect + met1 hull descent vs met2 transit interior). This decides
  whether the contiguous `lo..=first_unmasked` layer rule
  (mechanism §3) is the right shape or needs narrowing.
- Record all of it in the `results.md` entry.

## Step 1 — types + plumbing + fixture (lands inside commit 1)

1. `EscapeKind`/`EscapeRecord`/`RouteGraphs.escapes`,
   `GraphOpts`/`EscapeCfg` + `build_route_graphs_ext` signature,
   call-site updates (`mechanism.md` §5–6).
2. `escape_extend` skeleton: strict-escape pre-pass + shared
   `strict_attached` helper + `region_escape` push-condition change
   (§2). `escapes` records emitted, no pairs added yet (or gate the
   mutation behind the cfg) — behavior with `escape: None` is
   bit-identical.
3. `test_maze_guide_escape` fixture (fixture.md): arm A-red green
   immediately; arm A-green **red** — capture for `results.md`.
4. `test_pin_attachment` escape-arm update (reporting.md §4) — red
   until §3 lands.

## Step 2 — the mechanism (rest of commit 1)

5. Halo pair collection + `point_allowed` filtering (§3).
6. Span search + bounded L-corridor (§4) + `EscapeKind` outcomes.
7. Emit-side `escape=` suffix join (reporting.md §1) +
   `recount_unrouted.py` parser (§2).
8. Fixture: all arms green, no assertion edits. Break-it check:
   drop `point_allowed` on halo pairs → arm D red; restore.
9. `cargo test --workspace`; `cargo clippy --workspace -- -D warnings`.

Commit 1: `feat: Add bounded pin-halo escape for guide-region
escapes in the DRT graph` — body: mechanism + bounds rationale, the
pre-halo `region_escape` semantics decision, the red→green protocol,
rejected route-time retry, version bump
(`workspace.package` 0.12.12 → 0.12.13, patch).

## Step 3 — regression surface (same verification, not a commit)

10. `test_maze_census` (gcd): unrouted delta vs 81 baseline;
    `iter_progress`/`iter_prov`; digest old→new; `projected` delta
    (halo strict attaches may convert former projections). Any
    unrouted delta is a named win or a named explanation.
11. Graph census re-pins: `total_nodes`/`total_edges`/
    `via_edges_per_pair`/`total_via_edges` — print-then-pin
    (never hand-derive, E0 §2).
12. `test_maze_determinism`, `test_maze_occupancy`,
    `test_emit_roundtrip_gcd`, `test_emit_selfdrc_gcd` — escape
    metal must not introduce self-DRC findings; if it does, name
    the residual, don't absorb it silently.
13. `test_e2e_ours_def_deterministic_gcd` — expected red if any gcd
    route moves → regenerate `tests/designs/gcd/
    gcd.drt.ours.routed.def` via `generate_drt_ours.sh gcd`
    (release binary, host — see §6), record old/new sha.
14. picorv32 (`--ignored`): `test_pin_attachment`-level graph census
    only (escape records by kind + pair counts) — the full emit leg
    (~2 h) is deferred unless the gcd leg raises a question only
    picorv32 can answer; decision recorded in `results.md`.

## Step 4 — docs (commit 2)

15. `docs/e6-drt.md` edits — `docs.md`.
16. `docs/results.md` dated entry — `docs.md` template.
17. `docs/status.md` — only if a named number moves.

Commit 2: `docs: Record the guide-escape halo mechanism + gcd
escape census`.

## Step 5 — verify

- `cargo test --workspace` green; clippy `-D warnings` clean.
- Acceptance: mechanism chosen + cost recorded (`escapes` census +
  results.md); escaped geometry occupancy-checked (fixture arm C);
  fixture red-before (steps 1.3); named failures (`fail` suffix +
  `EscapeKind`); gcd delta in `docs/results.md`; semver bump.

## §6 — Contingencies

- **gcd routes move** (any net's wave picks a different equal-cost
  path through new halo nodes): `gcd.drt.ours.routed.def`
  regeneration is the host leg (`generate_drt_ours.sh gcd`), not the
  container. `gcd.drt.ours.reference.json` staleness is named in
  `results.md`; regenerating it is the `check_drt_ours.sh` container
  leg (Colima + LibreLane — **ask first**, billable). The test gate
  reads the golden internally and stays green either way.
- **`projected`/strict mix shift on gcd**: halo attaches some former
  projections as strict — legitimate improvement, recorded, not
  masked.
- **`_134_`/`_138_` still unrouted after halo** (e.g. corridor
  masked or bound): that's a valid outcome — `escape=<kind>` names
  it; loosening bounds without a measured reason is out of scope.
- **picorv32 escape count × halo cost** (592 × bounded pairs) grows
  graphs measurably: record `escapes` pair counts; if growth is
  outsized, per-layer halo caps are the honest knob — do not shrink
  silently.
- **`EscapeKind::CorridorMasked` unreachable** by construction
  (unmasked corridor always survives): drop the variant or keep +
  document — see fixture.md arm D.
