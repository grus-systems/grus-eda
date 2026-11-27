# Implementation steps

One commit: `test: Share route_nets legs across drt test assertions`
(or `refactor:` — the repo's `test:` prefix fits; check `git log`
convention). All edits in `crates/grus-eda-core/tests/drt.rs`, plus
`Cargo.toml` version bump and the `docs/results.md` entry.

## 0. Baseline capture (before any edit)

```sh
rg -c 'assert' crates/grus-eda-core/tests/drt.rs
GRUS_DRT_TIMING=1 cargo test -p grus-eda-core --test drt -- --nocapture 2>&1 \
  | rg -c 'drt: premark_obs'
time cargo test -p grus-eda-core --test drt
```

Record all three numbers for the results.md entry.

## 1. Add the shared-leg machinery

In the "Maze router" section, next to `load_routed_inputs`:

- `use std::sync::OnceLock;` (file already qualifies `std::sync::Mutex`
  inline — follow local style).
- `struct RoutedLeg`, the three `static` cells, `shared_leg`,
  `gcd_leg`, `gcd_occ_off_leg`, `leg_for`, `route_fresh` — per
  `mechanism.md`.
- The `const _` `Sync + Send` compile assertion (graph.rs pattern).

`RoutedLeg` init loads via `load` + `load_guide` + `via_table` —
no `build_route_graphs_ext`.

## 2. Reshape `emit_leg` and `emit_gate`

- `emit_leg(leg: &'static RoutedLeg) -> (DesignDb, EmitReport)` —
  clone `leg.db`, `apply_routes` with `leg.route`/`leg.vias`.
- `emit_gate(leg: &'static RoutedLeg) -> &'static RouteDb` —
  callers assert on `route_db.area_patches`/residuals; returning
  `&leg.route` avoids a clone.
- Update all ten `emit_leg`/`emit_gate` call sites per
  `callsites.md`.

## 3. Re-point maze-side tests

- `maze_census`: signature stays `maze_census(name: &str)` but body
  opens `let leg = leg_for(name);` — replace `db`/`tracks`/`join`/
  `vias`/`r`/`ms` uses with `leg.*`; `if name == "gcd"` →
  `if leg.name == "gcd"`. Every `assert` and `println` stays.
- `test_maze_occupancy`: `on`/`off` from the two legs;
  `overlap_census` uses `gcd_leg()`'s `tech`/`tracks`/`vias` for
  both (identical inputs).
- `maze_determinism`: `let leg = leg_for(name); let r2 =
  route_fresh(leg);` — asserts become `leg.route` vs `r2`.

## 4. Re-point rayon tests' inputs (top-level only)

- `test_rayon_route_determinism`: `let leg = gcd_leg()` before any
  `with_rayon_threads`; the three closure bodies call `route_nets`
  on `&leg.*` unchanged.
- `test_rayon_emit_serial_identity`: `let leg = gcd_leg()`; drop its
  `route_nets` + `load_routed_inputs`; use `leg.route` and two
  `leg.db.clone()`s for the apply arms.
- `test_net_graph_rebuild_identity`: unchanged (own bulk build +
  forced-thread streamed runs).

## 5. Fix the stale package name (drive-by, same file)

`test_maze_census_picorv32`'s doc comment says
`cargo test --release -p grus-core` — the package is
`grus-eda-core`. Fix the command while the file is being edited
(docs-in-step-with-code rule, AGENTS §7.8).

## 6. Local check loop

```sh
cargo test -p grus-eda-core --test drt --profile verify
rg -c 'assert' crates/grus-eda-core/tests/drt.rs   # == baseline
GRUS_DRT_TIMING=1 cargo test -p grus-eda-core --test drt --profile verify \
  -- --nocapture 2>&1 | rg -c 'drt: premark_obs'
time cargo test -p grus-eda-core --test drt        # dev-profile wall
cargo clippy --workspace --all-targets -- -D warnings
```

## 7. Red-check

Perturb `GCD_ROUTE_DIGEST` (`^ 1`) → `cargo test -p grus-eda-core
--test drt test_maze_census` must fail at the digest assert →
restore → passes. Record both lines.

## 8. Results + commit

- Append the `docs/results.md` entry (`measurement.md` template).
- Bump `[workspace.package] version` → `0.15.2` in root
  `Cargo.toml` (same commit).
- Commit `crates/grus-eda-core/tests/drt.rs`, `Cargo.toml`,
  `docs/results.md` — explicit paths, never `git add -A`.

## Verify

```sh
cargo test --workspace --profile verify
cargo clippy --workspace --all-targets -- -D warnings
```

## Risks

- **`&'static` borrow shape**: every shared consumer only needs
  `&leg.field`; the two mutation sites go through `emit_leg`'s
  owned clone — no `&mut` of shared state is expressible.
- **assert drift**: mechanical ref swaps (`r.` → `leg.route.`) —
  the `rg -c assert` equality check catches accidental drops.
- **Wall-time variance**: dev-profile suite is ~tens of minutes;
  single-rep timing is acceptable if the entry says so.
