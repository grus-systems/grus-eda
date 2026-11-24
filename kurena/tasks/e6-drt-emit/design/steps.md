# e6-drt-emit — Close-out steps (post-rejection revision)

Emission landed; the detached picorv32 gate measured 42 min-area
findings (`li.6`×3, `met3.6`×39 — committed in `results.md`). This
revision lands the `minarea.md` patch, proves it on toy fixtures,
then re-runs the picorv32 leg **once** and records the delta.

## 1. Implement the patch (maze.rs)

- `AreaResidual` + `RouteDb.area_patches`/`area_residuals` fields +
  `drt.rs` re-export.
- `patch_net_min_area` per `minarea.md` — component model via
  `LayerShapes::from_rects` + `union::components` + `union_area`
  (the checker's own primitives), preferred-axis node-walk under
  occupancy-strict `scan_wire`, post-loop hook in `route_nets`
  before `routes` flattens.
- Census lines in `emit_gate` and the `drt` CLI print block
  (`area_patches`/`area_residuals` + per-residual lines).
- `route.wires.sort_unstable()` after patching.

## 2. Unit fixtures (maze.rs `mod tests`)

The five fixtures from `tests.md` — `toy_tech` gains the
`area_dbu2` bound parameter with existing callers at `0`. Run:

```
cargo test -p grus-core drt::maze::tests -- --nocapture
```

Every fixture must show the sub-bound component first (non-vacuity
assert inside each test, AGENTS §7.7).

## 3. In-suite legs

```
cargo test --release -p grus-core --test drt emit -- --nocapture \
    2>&1 | tee /tmp/emit-gcd-patch.log
cargo test -p grus-core --lib drt
```

Expected: gcd gate stays `findings_total=0`; `area_patches`/`residual`
census prints (gcd may legitimately be `0/0` — record what it
measures); digest stays `0x48593d441074229b` if the patch is a
no-op on gcd — a shift is investigated first (it would mean gcd
has sub-bound components the gate didn't flag, i.e. foreign-metal
merging carried them — plausible but must be *seen*, not assumed).

## 4. Launch the picorv32 gate re-run — detached, once

```
cargo test --release -p grus-core --test drt \
    test_emit_selfdrc_picorv32 -- --ignored --nocapture \
    2>&1 | tee /tmp/emit-picorv32-patch.log
```

Detached (nohup / separate shell) — budget ≈ route (~135 min clean;
the last leg measured 30,658 s under ambient load) + emit + parse +
`run_all`. Captures for `results.md`: routed/unrouted, the
`area_patches`/`area_residuals` census lines, emit census deltas,
the `li.6`/`met3.6` findings delta vs the committed 42 (target: 0
findings; each surviving `*.6` must map to a recorded
`AreaResidual`), wall clock.

## 5. Docs (docs.md), drafted while step 4 runs

`docs/e6-drt.md` §7 gains the min-area patch paragraph (post-loop
pass, component model, occupancy-strict extension, residual naming);
`docs/results.md` appends the re-run outcome to the dated entry
(baseline 42 → fresh count, patch census, wall clock); `docs/
status.md` and `docs/roadmap.md` one-line updates.

## 6. Version + verify + commit

- `[workspace.package] version` `0.12.5 → 0.12.6` (bugfix → patch
  on 0.x), same commit.
- `cargo test --workspace` — default set only (the picorv32 leg is
  `#[ignore]`d and never enters it).
- `cargo clippy --workspace --all-targets -- -D warnings`.
- Commit: `fix: Post-route min-area patch for stub components`
  (Conventional, English, agent trailer). Body: the measured class,
  the model, the occupancy-strict/region rules, fixture list, the
  re-run delta (or "in flight" note per below).

## Contingencies

- **picorv32 leg still running at verify time** — same rule as last
  round: the kurena verify never runs it; commit the unit-fixture +
  gcd evidence with the leg recorded as "running detached — command
  + start time", and land the delta numbers in the card's follow-up
  or the e2e ticket. Do **not** gate-verify an unfinished claim.
- **Residual `*.6` findings on the re-run** — each must correspond
  to an `AreaResidual` line; a finding without one is a component-
  model divergence — investigate, never loosen the assert. If the
  residuals all share a mechanism (e.g., a preferred axis walled by
  foreign metal), name it in `results.md` — the acceptance's named
  path.
- **New spacing/enclosure findings** — the patch is
  occupancy-strict by construction; a new class means the scan
  semantics diverged from the checker's — debug against the
  specific finding before touching thresholds.
- **gcd digest/count shift** — investigate (see step 3); re-pin
  only after the shifted number is explained in `results.md`.
- **Fixture shows the walk can't reach bound on a real class** —
  the preferred-axis-only domain is decision 9; if a measured
  finding class needs a perpendicular fallback, that is a design
  change for the next card, not a silent extension.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-core/tests/drt.rs`
- `crates/grus-eda/src/main.rs`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
