# drt-unrouted-census — Steps

Print-path ticket; no routing behavior changes. `e6-drt-e2e` has
landed (v0.12.7/v0.12.9) — the pin-access rewrite the spec warned
about is already the code state this design is written against.

## 1. `main.rs` — the census block (~2053-2116)

- Add a file-scope `fn pin_label(db: &DesignDb, pin:
  grus_core::interner::PinId) -> String` — `<inst>/<pin>` /
  bare port name; reuse it in the existing `stub_outside_cover`
  print (which inlines the same logic today).
- Before the `unrouted` loop: build
  `let escapes: std::collections::BTreeSet<(u32, u32)> =
  graphs.region_escape.iter().map(|e| (e.net.0, e.pin.0))
  .collect();`
- Replace `for name in &report.unrouted { println!("  unrouted
  {name}"); }` with an iteration over `route_db.unrouted`:
  `println!("  unrouted {}", guide_net_name(&design_db, u.net))`
  then per `&(p, k) in &u.failed` a
  `println!("    fail {} {k:?}{esc}", pin_label(&design_db, p))`
  where `esc = " region_escape"` iff `(u.net.0, p.0) ∈ escapes`.
- `report.unrouted` remains used for the `unrouted=` count in the
  summary line — no dead binding.
- Style: this block already writes fully-qualified paths
  (`grus_core::drt::maze::FailKind`, `std::collections::BTreeMap`)
  — match it.

## 2. `maze.rs` — doc comment only

Extend `UnroutedNet.failed`'s comment with the final-iteration /
`pin_nodes`-order semantics and the `stub_outside_cover`
ever-failed distinction (`semantics.md` §5). No code change.

## 3. Consumers

- `check_drt_ours.sh`: add the `fail ` keep-list arm
  (`consumers.md` §1). Nothing else — no schema, no regex change.
- `recount_unrouted.py`: Semantics docstring update + the
  tolerant `fail`-line join (`consumers.md` §2).
- `docs/e6-drt.md` §8: one sentence on `fail` lines +
  final-vs-ever-failed.

## 4. Verify

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p grus-eda
```

Then the honest-evidence legs (record each in `docs/results.md`):

- **gcd emit run** — `target/release/grus-eda drt --tlef … --lef …
  --def tests/designs/gcd/gcd.placed.def --guide
  tests/designs/gcd/gcd.guide --out-def /tmp/gcd.census.def`
  (~2 s route). Capture: the `fail`-line block; assert
  `#fail lines == NoPath + Unattachable` (98 on the recorded
  census); count `region_escape` flags and compare against gcd's
  17 pinned escapes (if zero, say so — the flag is then vacuous on
  gcd, honest state).
- **Digest arm** — `cargo test --release -p grus-core --test drt
  test_maze_census -- --nocapture`: the printed `digest=` must equal
  the pre-change value on this branch (record both; the change is
  print-only so a shift is a bug — but verify, don't assert from
  reasoning). `test_maze_determinism` in the workspace suite is the
  in-run arm.
- **Fail-proof (AGENTS §7.7)** — the format itself: break the
  `unrouted` line into 3 tokens on a scratch edit → confirm the
  step-E regex/recount classification would fail (a reasoning
  exercise already settled by `format.md`; the live check is the
  `#fail lines == kinds sum` self-check above).

## 5. Docs + version + commit

- `docs/results.md`: dated entry — sample output block (gcd),
  `region_escape` flag count measured, digest before/after,
  semantics resolution one-liner (`u.failed` = final;
  `stub_outside_cover` = ever-failed).
- `docs/status.md`: one-line update.
- `Cargo.toml`: `[workspace.package] version` `0.12.9 → 0.12.10`
  (feature addition → patch on 0.x), same commit.
- Commit: `feat: Per-pin unrouted failure census in drt emit`
  (Conventional, English, agent trailer).

## Contingencies

- **gcd shows zero `region_escape` flags** — record it verbatim;
  do not manufacture coverage. The picorv32 emit leg (592 escapes)
  is the real exercise and stays deferred like every ~2.3 h picorv32
  leg; note it as the pending arm.
- **Escaped-slash inst names tripping the step-E path guard** —
  only reachable on regenerated goldens; document in the results
  entry if it fires (`consumers.md` §1).
- **`fail` prefix colliding with a future emit line** — prefix is
  owned by this census now; a collision surfaces as a malformed
  `cli_lines` entry in step-E, which fails loudly by design.

## Files

- `crates/grus-eda/src/main.rs`
- `crates/grus-core/src/drt/maze.rs`
- `tests/designs/check_drt_ours.sh`
- `tests/designs/recount_unrouted.py`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `Cargo.toml`
