# steps — implementation order and verification

One logical unit, one commit (the tree must compile at every commit;
splitting API change from its call-site fallout leaves a red tree).
The order below is for working through the change, not for commit
boundaries.

## Step 0 — baseline (read-only)

Confirm the pins this refactor must not move:

- `cargo test -p grus-eda-core --profile verify --test drt
  test_maze_census` → digest `0xef2b3ff70271a2e9` (pinned at
  `tests/drt.rs:2366`).
- `test_e2e_ours_def_deterministic_gcd` green at HEAD — the DEF
  byte-golden is `tests/designs/gcd/gcd.drt.ours.routed.def`.

## Step 1 — `src/drt/pool.rs`

`drt_pool(threads: usize)` per `api.md`; delete the env read; rewrite
the module/fn doc.

## Step 2 — the four call files

- `maze.rs`: `MazeConfig.threads` + `Default`; `drt_pool(cfg.threads)`
  at both sites; rewrite the L4297–4299 comment.
- `graph.rs`: `build_route_graphs_ext(.., threads)`; `drt_pool(threads)`;
  `build_route_graphs` passes `0`; docstring clause.
- `emit.rs`: `apply_routes(.., threads)`, `nets_section(.., threads)`,
  `write_routed_def(.., threads)`; two `drt_pool(threads)` sites.
- `main.rs`: boundary `RAYON_NUM_THREADS` read; `MazeConfig` literal
  gains `threads`; `apply_routes`/`write_routed_def` call sites.

`cargo check -p grus-eda` must be clean before touching tests —
it isolates library/CLI errors from the test refactor.

## Step 3 — `tests/drt.rs`

Per `tests.md`: delete `THREAD_ENV_LOCK`/`with_rayon_threads` and the
stale pinning comments; rewrite the four forced arms; append `0` (or
the forced count) at the ~25 remaining call sites. Compile: the
`rg` greps in `tests.md` come back empty/clean, and
`cargo check --tests -p grus-eda-core` is green.

## Step 4 — version + docs

`Cargo.toml` `0.15.2 → 0.16.0` (same commit per AGENTS §5);
`cargo check` regenerates `Cargo.lock`. `docs/e6-drt.md` paragraph,
`docs/status.md` header line per `docs.md`.

## Step 5 — verify

- `cargo test --workspace --profile verify` — the declared verify
  command; all four forced arms pass at explicit 1/8.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- Pins: `test_maze_census` digest `0xef2b3ff70271a2e9` and
  `test_e2e_ours_def_deterministic_gcd` byte-equality — both run
  inside the suite; no golden regeneration is needed or permitted
  (a moved digest/golden means the refactor changed output — stop
  and find the cause).
- Greps (`tests.md` list) pasted into the results entry.

## Step 6 — results entry + commit

Write the `docs/results.md` dated entry (`docs.md` skeleton) naming
exactly which commands ran and their outcomes, then commit per the
`docs.md` message sketch — `refactor!:`, breaking-change body,
workspace bump in the same commit, agent trailer.
