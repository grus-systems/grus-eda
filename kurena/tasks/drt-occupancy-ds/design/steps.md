# steps — implementation order, commits, verification

Four commits. Order matters: the counter census must exist on the
**old** structure before the swap, and the digest baseline must be
captured before anything moves.

## Step 0 — baseline capture at HEAD (no code change)

1. `cargo build --release -p grus-eda`.
2. `route_ms` on gcd, ≥5 reps → min/median/max (`measurement.md` §2
   commands, `--no-self-drc`, `--out-def /tmp/…`).
3. `sample $PID 10` mid-route → collapsed stacks; attribute
   `scan_wire`/`scan_rect`/`insert`/`rebuild_from`/`unmark`/
   `rasterize` + `core::…btree` frames.
4. `cargo test --release -p grus-eda-core --test drt
   test_maze_census -- --nocapture` → record `digest=` (expect
   `0x8a4768ab15bd4bd5` — verify) and `collisions=`.
5. sha256 of one `/tmp/gcd.occ.*.def` → expect
   `27f7665922c6df0212788f27b73f562d078e5bebdd47446eb1f933a469122aee`
   (the committed golden's recorded sha).

## Step 1 — `feat:` ScanStats instrumentation (commit 1)

`crates/grus-eda-core/src/drt/maze.rs` only:

- `ScanStats` struct + `Cell<u64>` fields on `Occupancy` per
  `measurement.md` §1; increments inside `scan_wire`/`scan_rect`/
  `rasterize`/`unmark`.
- Emission inside the existing `timing` (`GRUS_DRT_TIMING`) blocks:
  cumulative line per iteration (~L3959 block) + a final
  totals/averages line after the rip-up loop.
- `Cargo.toml` workspace version `0.14.0 → 0.14.1`.
- Commit: `feat: Add GRUS_DRT_TIMING occupancy scan census`.

Verify before commit: `cargo test --workspace --profile verify`
green — counters touch no `RouteDb` field; digest must stay
`0x8a4768ab15bd4bd5` (rerun `test_maze_census`). Then run
`GRUS_DRT_TIMING=1 … drt` on gcd → capture the counter census +
query-shape lines (the "before" rows). This is also the moment the
pick is *confirmed*: if `rect_lines_visited/rect_call` turns out
large (≫10), re-open `structure.md` §4 before committing to the
swap — say so in the results entry.

## Step 2 — `refactor:` dense track-indexed occupancy (commit 2)

`crates/grus-eda-core/src/drt/maze.rs` + `docs/e6-drt.md` +
`Cargo.toml` (`0.14.1 → 0.14.2`):

1. `Occupancy` fields → `Vec<Vec<Line>>` + `coords_{x,y}`
   (`structure.md` §1–2); `Mark.line` → `line_idx: u32`.
2. `rasterize` drops `tracks` param; `scan_wire` takes `ti: u32`;
   `scan_rect`/`unmark` per §3.
3. Call-site index plumbing per §5 table — `edge_cost`,
   `edge_collisions`, `scan_stub`, `patch_component`, tests.
4. `overlap_census`/`pin_overlap_census` iteration per §6.
5. `docs/e6-drt.md` §Occupancy paragraph (L560–575): "map a line
   coordinate to a `Line`" → dense `Vec<Line>` indexed by track
   position + owned coord lists (AGENTS rule 8 — docs move with
   the code).
- Commit: `refactor: Index occupancy lines by track position`.

Verify: `cargo test --workspace --profile verify` green;
`test_maze_census` digest **identical** to baseline;
`test_e2e_ours_def_deterministic_gcd` byte-identical;
`test_maze_determinism` green.

## Step 3 — `test:` differential oracle + digest pin (commit 3)

`crates/grus-eda-core/src/drt/maze.rs` (`#[cfg(test)]`) +
`crates/grus-eda-core/tests/drt.rs`:

- `RefOccupancy` verbatim-copy oracle + seeded differential harness
  (`tests.md` §1–2).
- `GCD_ROUTE_DIGEST` pin inside `maze_census` (`tests.md` §3).
- Commit: `test: Pin occupancy scan equivalence and gcd route digest`.

Verify: differential test green on both seeds; prove-it-red arms
(`tests.md` §4) executed and restored, transcript tails kept for
the results entry.

## Step 4 — after-measurement + `docs:` entry (commit 4)

- Re-run §2/§3 measurement protocol verbatim on the post-swap
  binary; picorv32 bounded arm only if it fits (`measurement.md` §4).
- `docs/results.md`: dated entry per `measurement.md` §5 skeleton —
  both tables, digest/sha identity, counter deltas, sample
  attributions, fail-proof note, exact commands.
- `docs/status.md` "Last updated" line: `v0.14.2
  drt-occupancy-ds` — one sentence naming the swap + recorded
  route_ms before→after + digest unchanged.
- Commit: `docs: Record occupancy-swap measurements`.

## Verification checklist (maps to acceptance)

- [ ] Scan share before/after in the results entry (sample +
      counters, honest about inlining).
- [ ] Differential test proves identical Hit sets; seed count +
      op count recorded.
- [ ] gcd digest `0x8a4768ab15bd4bd5` (or the step-0 measured
      value) byte-identical; `collisions=` count identical; DEF
      sha `27f76659…` identical; pin committed.
- [ ] `route_ms` improves measurably — both numbers in the entry;
      picorv32 figure named or marked not-measured.
- [ ] `cargo test --workspace --profile verify` + clippy
      `-D warnings` green.

## Files

- `crates/grus-eda-core/src/drt/maze.rs`
- `crates/grus-eda-core/tests/drt.rs`
- `docs/results.md`
- `docs/e6-drt.md`
- `docs/status.md`
- `Cargo.toml`
