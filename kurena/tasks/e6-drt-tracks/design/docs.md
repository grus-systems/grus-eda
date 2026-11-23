# e6-drt-tracks — Documentation updates

## `docs/e6-drt.md` (new)

The E6 model doc — the track-grid and access-point substrate only;
later tickets append routing sections. Sections:

1. **Scope** — what the substrate is, pointers to `e6-drt-ref` for the
   delegation reference, non-goals verbatim from the spec.
2. **Track model** — the two provenances and the merge rule:
   - LEF `TRACKS` → `RoutingLayer.tracks_{x,y}` (expanded at
     `parse_lef_bytes` end; round-then-expand convention
     `(v * dbu).round()` on `start`/`step`, then `x0 + k·s`).
   - DEF `TRACKS` → `DesignDb::tracks` (`TrackGrid`, already DBU) →
     expanded and merged by `drt::tracks::layer_tracks`, sorted+deduped.
   - **The sky130 fact**: the techlef declares zero `TRACKS`; the
     effective grid on gcd/picorv32 is entirely DEF-sourced. LEF
     grammar support is covered by synthetic fixtures. Say this
     explicitly — the acceptance's "LEF TRACKS parse into lists" would
     otherwise read as vacuous coverage.
   - Preferred axis: `RoutingLayer.dir` from LEF `DIRECTION`;
     Horizontal ↔ `tracks_y`, Vertical ↔ `tracks_x`. A layer missing
     its preferred axis is a `TrackGap` — named, never synthesized
     from `PITCH`/`OFFSET`.
   - Strictness asymmetry to note: LEF `TRACKS` errors on unknown
     layers and bare-form applies to all routing layers; DEF `TRACKS`
     drops unknown layers silently (pre-existing, untouched).
3. **Access-point model** — shape enumeration (instance pins via
   `pin_macro_pins` + `orient_rect` + translate, gated on
   `Placed|Fixed|Cover`; io `BTERM`s via `pin_io_rects`), the midpoint
   rule with the i32-truncation note, boundary-inclusive tracks,
   per-pin dedup, emission order.
4. **Findings taxonomy** — `off_track` vs `no_shapes` vs `TrackGap`:
   all three named, all gate-blocking on the test designs.
5. **Claim boundary** — what access points are *not*: no DRC pruning
   (a point may collide with macro OBS or another net's shapes — the
   maze router's problem), no track assignment (candidates, not
   choices), no comparison to TritonRoute's AP sets (different
   algorithm — our enumeration is a candidate superset, not a
   reproduction). The census gate proves coverage and determinism on
   real inputs, nothing more.

## `docs/results.md`

New dated `### 2026-11-… — e6-drt-tracks:` entry in established form:

- **Track census table**: per design, per layer — `dir`,
  `tracks_x` count, `tracks_y` count, preferred-axis status; the
  "LEF `TRACKS` = 0 records in sky130, grid is DEF-sourced" line.
- **Access census table**: per design — member pins counted,
  `points` total, `per_layer` nonzero counts, `off_track`/`no_shapes`
  counts (expected 0 — if not, name them verbatim).
- **Determinism**: double-run equality with `points.len()`.
- **Fail-proofs**: the preferred-axis drop (red → restore → green)
  and the off-track fixture — one line each, matching the existing
  `met5.1`-drop entry's terseness.
- **Checks run**: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and which
  test names ran.

## `docs/status.md`

Prepend the `Last updated:` line for the ticket (same register as the
`v0.12.0` e7-drc-e2e line): LEF `TRACKS` grammar + `RoutingLayer`
track fields, `drt::tracks` LEF∪DEF merge with named gaps,
`drt::access` candidate APs, census green on both designs with the
per-layer evidence.

## `docs/roadmap.md`

One `- Progress 2026-11-… (ticket `e6-drt-tracks`):` line directly
under the `e6-drt-ref` line in the `### E6/E7` block: track model +
AP census landed, both designs all-pins-covered, next tickets
(maze/grid graph) if known — otherwise name the substrate-only scope.

## `docs/README.md`

Add `e6-drt.md` to the reading-order list in step 3. (`e7-drc.md` is
currently absent from the same list — add it in the same edit; it is
an adjacent one-line fix, record it in the commit body.)

## Version

`[workspace.package] version` `0.12.0 → 0.12.1` in `Cargo.toml`
(feature addition ⇒ patch on 0.x), `Cargo.lock` regenerated — same
commit as the functional change.

## Files

- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `docs/README.md`
- `Cargo.toml`
- `Cargo.lock`
