# steps — order, commits, verification

Order is load-bearing: the split table must exist before any
reduction is picked (acceptance item 1), and the pre-change DEFs
must be captured at HEAD for the byte-identity gate.

## Step 0 — baseline at HEAD (no code change)

1. `cargo build --release -p grus-eda`.
2. `/usr/bin/time -l` runs per `measurement.md` §2: gcd ≥3 reps →
   median peak RSS; picorv32 one detached run (regenerate
   `picorv32.placed.def` via `tests/designs/picorv32/generate_ref.sh`
   if absent — gitignored).
3. Keep `/tmp/gcd.mem.base.def` (expect sha256
   `1422d02765f6aba99e25f1792436607c788ca31397c0bf843340bd65fb2f9482`)
   and `/tmp/picorv32.mem.base.def` — the byte-identity baselines.
4. `cargo test --release -p grus-eda-core --test drt test_maze_census
   -- --nocapture` → confirm `digest=0xef2b3ff70271a2e9`.

## Commit 1 — `feat:` `GRUS_DRT_MEM` structural census

`crates/grus-eda-core/src/drt/mem.rs` (new) + export in `drt.rs`;
capacity-walk fns in `graph.rs` (`route_graphs_bytes`) and `maze.rs`
(`net_topo_bytes`, `occupancy_bytes`, `marks_bytes`); `libc` dep in
`crates/grus-eda-core/Cargo.toml` (getrusage for `peak_rss_bytes` —
justify in the commit message); checkpoint lines in
`route_nets_with_progress` and the `Commands::Drt` arm in `main.rs`.

- Measurement-only: no `RouteDb`/emit change, digest stays
  `0xef2b3ff70271a2e9` (re-run `test_maze_census` to prove).
- Workspace version patch bump.
- Verify: `cargo test --workspace --profile verify` green; clippy
  `-D warnings` clean; ungated run emits zero `drt: mem` lines
  (fail-proof the gate once: forced on → lines appear → restore).
- Then run the §3 split on gcd and picorv32 (`GRUS_DRT_MEM=1` +
  `time -l`, picorv32 detached). Fill the §5 table → **pick per
  `options.md` §"Pick rule"**.

## Commit 2..N — `feat:` the picked reduction(s)

One logical change per commit (O1, O2, O3/O4 riders as picked).
After each:

- `cargo test --workspace --profile verify` + clippy — all green;
  every pin in `tests.md` §1 holds.
- `route_digest` unchanged unless the change is *deliberately*
  semantic (none of the options are — drift means a bug, not a
  re-pin).
- gcd: emitted DEF byte-identical to the committed golden
  (`test_e2e_ours_def_deterministic_gcd`).
- picorv32: `cmp` the fresh DEF against `/tmp/picorv32.mem.base.def`
  (detached run per picked change; it doubles as the after-RSS rep).
- Workspace version patch bump per commit.

## Final commit — `docs:` the record

`docs/results.md` dated entry per `docs.md` (split table, before/after
RSS, route_ms effect, DEF shas, what-was-run), `docs/e6-drt.md`
memory paragraph with the honest bound, `docs/status.md` line.
Land with the last functional commit's verification output quoted.
