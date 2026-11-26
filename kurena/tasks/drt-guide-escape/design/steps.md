# Steps — revision implementation order, verification, commits

The mechanism, fixture arms A–E (off / halo / bound / masked /
landing), emit suffix, and parser already landed (`94cc1ea`). This
revision closes the review memo: contract corrections + the three
lows + the occupancy arm. Two commits: (1) code + tests `fix:`,
(2) `docs:` corrections.

## Step 0 — re-measure the baseline (evidence only, no commit)

- `cargo test --release -p grus-core --test drt test_pin_attachment
  -- --nocapture` on gcd + picorv32 (graph leg): capture the current
  escape-kind census — gcd {halo: 7, halo_empty: 10}, picorv32
  {halo: 425, corridor_masked: 9, halo_empty: 158} — so the revision
  can name every delta (kind reclassifications, `pairs_added`
  corrections, graph census drift from endpoint tie-breaks).
- Record pre/post in the `results.md` revision paragraph.

## Step 1 — graph.rs fixes (lands inside commit 1)

1. `EscapeKind::NoAnchor` + `token()` arm (`escape=no_anchor`) +
   doc comments (`mechanism.md` §5 table is the contract).
2. `escape_span` rewrite — windowed enumeration with `cap` and the
   halo-bbox prune (`mechanism.md` §4); signature drops `nx`/`ny`
   for `cap`; no `nx*ny` allocation anywhere.
3. Candidate probe `cap = corridor_lines`; refusal path probes
   uncapped per escape layer → `NoAnchor` when every layer is
   anchorless, else `CorridorBound` with the exact min span.
4. `pairs_added` = distinct inserted pairs (`mechanism.md` §5):
   `extra.len() + (halo_total − |halo[landed]|)` or an equivalent
   union count — never `halo_total + extra.len()`.

## Step 2 — test updates (same commit)

5. `test_region_escape_fixture` empty-guide arm: `CorridorBound` →
   `NoAnchor` + `corridor_span == 0` + `token() == "no_anchor"`
   (fixture.md arm G).
6. `test_maze_guide_escape` landing arm: add `pairs_added == 3`
   (fixture.md arm E — the double-count regression pin).
7. `test_pin_attachment`: add `NoAnchor` to the refused-kinds match
   arm (empty `pin_nodes`).
8. `test_maze_guide_escape` arm F (occupancy, fixture.md):
   `ESC_DEF_TAIL_OCC` + `maze_db_custom` rebuild; same wall comps +
   `3000 …` guide + `escape: Some(DEFAULT)`; assert `escapes ==
   [Halo]`, then `route_nets` twice — `OccMode::Enabled` ⇒
   `failed(u0/A) == [NoPath]`, `OccMode::Disabled` ⇒ routed with 2
   `connected`.
9. Red→green evidence for arm F: the enabled run is the red side of
   the contrast; capture both runs' unrouted sets for `results.md`.
   Break-it check: delete io4 → the *enabled* run routes (the band,
   not the wall, was the blocker); restore.
10. `cargo test --workspace`; `cargo clippy --workspace
    --all-targets -- -D warnings`.

Commit 1: `fix: Correct escape record accounting and restore the
occupancy contrast` — body: `NoAnchor` for the no-target case
(sentinel span removed), `pairs_added` union count, `escape_span`
bounded enumeration replacing the `nx×ny` BFS, arm F restored with
the `Pin`-mark-only constraint named (`hit_verdict` blocks `Obs`/
`ObsPin` before the `Disabled` check), the `NoPath` contract noted.
No version bump — `0.13.0` is unreleased on this branch and the
added enum variant rides the same breaking change.

## Step 3 — regression surface (same verification, not a commit)

11. `test_maze_guide_escape` all arms green.
12. `test_pin_attachment` kind census re-print — name any
    `corridor_bound → no_anchor` reclassification and every
    `pairs_added` delta in `results.md`.
13. `test_graph_census` / `test_via_edge_census` /
    `test_graph_determinism`: the probe is span-identical; only
    corridor-endpoint tie-breaks could shift painted pairs — if a
    pin moves, print-then-pin, never hand-derive (E0 §2).
14. `test_maze_census` + `test_maze_determinism` +
    `test_e2e_ours_def_deterministic_gcd`: routes should not move;
    if the DEF/digest moves, regenerate per `reporting.md` §3 and
    record old/new sha.
15. picorv32 graph census only (`--ignored` state unchanged) — the
    emit leg stays deferred as before.

## Step 4 — docs (commit 2)

16. `docs/e6-drt.md` edits — `docs.md`.
17. `docs/results.md` — amend the fixture line + append the
    revision paragraph (`docs.md`).
18. `docs/status.md` — token list (`no_anchor`).

Commit 2: `docs: Correct the guide-escape contract — NoPath
semantics, occupancy arm, record fixes`.

## §5 — Acceptance cross-check

- Escape mechanism for out-of-region pins: landed (arm B) —
  unchanged.
- Escaped geometry occupancy-checked: arm F enabled/disabled
  contrast (restored by this revision).
- Fixture red before mechanism: arm A (bit-identical baseline) +
  arm F's built-in red side.
- Named failures: `fail` line `escape=<token>` incl. `no_anchor` —
  no sentinel renders.
- gcd unrouted delta: in `docs/results.md` (80 vs the 81 baseline;
  the revision paragraph records any drift).
- `cargo test --workspace` + clippy `-D warnings` green.

## §6 — Contingencies

- **Corridor-endpoint tie-breaks move a route** (probe argmin order
  differs from the retired BFS): spans are identical; if
  `test_e2e_ours_def_deterministic_gcd` or the graph census pins
  move, regenerate/re-pin per `reporting.md` §3 and record — do not
  force the old tie-break to match.
- **A real design records `NoAnchor`** (was `CorridorBound` +
  `u32::MAX`): honest reclassification — the `results.md` paragraph
  names the pins and the token change.
- **io4's lone-port net trips the DEF reader**: add `io5` on `n2`
  in a corner — same band contract (fixture.md).
- **Arm F soft-price surprises** (the enabled run routes early with
  a recorded `pin_collisions` provisional instead of going
  `NoPath`): the re-queue + hard-iteration machinery should still
  end `NoPath`; if it ends routed-with-collision, the honest
  assertion is the provisional-collision record vs none under
  `Disabled` — decide from the measured outcome and record it, do
  not force `NoPath` if the router legitimately settles elsewhere.
