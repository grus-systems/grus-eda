# e6-drt-e2e — implementation steps

Ordered; container steps need the pinned LibreLane image via
Colima (`AGENTS.md` §2).

1. **Script fixes** (`tests/designs/check_drt_ours.sh`): the
   unclassified allowlist + abort, the `wrote routed DEF` drop +
   absolute-path guard (`scripts.md`). Fix before generating
   goldens so the first committed JSON already carries them.
2. **Hull mask** (`maze.rs`/`graph.rs`): hull rects +
   `cover(N)` map, node filtering in `build_route_graphs_ext`,
   `pin_obs_rects` into `premark_obs`, `hull_masked` census,
   stub-confinement check (`hull-model.md`).
3. **Local iteration**: `cargo test -p grus-core drt` — maze/emit
   tests, census prints, unrouted re-pins. Decide the ladder
   variant from measured starvation (strict → L1 → L2 per
   `hull-model.md`); record the decision + counts.
4. **Regenerate + score gcd**: `generate_drt_ours.sh gcd --verify`
   (`OURS-DETERMINISTIC <sha>` printed), then `check_drt_ours.sh
   gcd` in the container — inspect `lef_expressible`: iterate the
   ladder until `ours == 0` on all 50 (and `unclassified` ours
   counts resolved or named). Commit `gcd.drt.ours.routed.def` +
   `gcd.drt.ours.reference.json`.
5. **picorv32**: same emit + check; commit
   `picorv32.drt.ours.reference.json` (its routed DEF stays
   gitignored).
6. **Gate tests** (`tests/drt.rs`): the three tests + assertion
   re-pins from `gate.md`; run the ignored picorv32 leg manually.
   Fail-proof arms verified (sanity-control asserts, `hull_masked
   > 0`).
7. **Docs**: `e6-drt.md` §7 + e2e section + claim boundary,
   `results.md` dated entry, `status.md`/`roadmap.md` lines
   (`docs.md`).
8. **Version bump** (`Cargo.toml` workspace `patch` — behavior
   fix, `0.x` rules) in the same commit as the functional change.
9. **Final**: `cargo test --workspace`, clippy `-D warnings`,
   `git diff` the committed goldens vs fresh regeneration (E0
   lesson 1 — rerun the exact committed scripts).

## Files

- `crates/grus-core/src/drt/maze.rs` (+ `graph.rs` if the mask
  lands there, `emit.rs` only if residuals demand)
- `tests/designs/check_drt_ours.sh`
- `tests/designs/gcd/gcd.drt.ours.routed.def`
- `tests/designs/{gcd,picorv32}/<d>.drt.ours.reference.json`
- `crates/grus-core/tests/drt.rs`
- `docs/e6-drt.md`, `docs/results.md`, `docs/status.md`,
  `docs/roadmap.md`, `Cargo.toml`
