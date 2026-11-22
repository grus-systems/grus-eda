# e4-guides-emit — Steps

One ticket, but three logical commits is the natural split (repo rule:
commit as soon as a unit is done). Version bump `0.3.8` → `0.3.9` (patch —
new feature, no breaking surface change outside the workspace) rides in
commit 1.

## Step 0 — Paths return + guide writer + CLI (commit 1, `feat:`)

1. `rsmt.rs`: add `decompose_design_nets_indexed` (segments +
   `net_seg_offsets` CSR); `decompose_design_nets` delegates.
2. `route.rs`: `route_segments` returns `RouteOutput{report,
   segment_paths}`; add `route_design`; `estimate_congestion` delegates;
   fix the two internal test call sites.
3. `design_db.rs` + `readers/def.rs`: `pin_io_layer` field populated from
   the `PINS` `+ LAYER` token.
4. `congestion/guide.rs` + `mod.rs`: `GuideBox`/`NetGuide`/`build_guides`/
   `write_guides`/`parse_guides` per `api.md` + `semantics.md`.
5. `main.rs`: `route` subcommand per `api.md` (shared load helper with the
   `congestion` arm).
6. Unit tests (`tests.md` §"Unit tests").
7. `Cargo.toml` `0.3.8` → `0.3.9`; `cargo check` refreshes `Cargo.lock`.
8. Gates: `cargo test --workspace` green; `clippy -D warnings` clean.
9. Commit (`feat:` + `Co-Authored-By` trailer).

## Step 1 — References + comparison tests (commit 2, `test:`/`feat:`)

1. Append step 5c to both `generate_ref.sh` (see `reference.md`); update
   the script header comments to list `<d>.guide`.
2. Container up per `AGENTS.md` §2; run **both** `generate_ref.sh` whole
   inside `ghcr.io/librelane/librelane:3.0.16` (picorv32 ~8 min). `git
   diff` on the directories must show only the new `.guide` files and the
   script edits — any other regenerated-file drift is investigated, not
   committed blind.
3. `git add tests/designs/{gcd,picorv32}/<d>.guide`; `README.md` artifact
   lists updated.
4. `guide.rs` integration tests: gcd + picorv32 metric gates, determinism
   test.
5. Run `grus-eda route` on both placed DEFs; `check_guide_readback.sh`
   against the gcd output in the container (`read_guides` + exercised
   `estimate_parasitics -global_routing`).
6. Fail-proof mutation + restore (`tests.md`).
7. `docs/results.md` dated `e4-guides-emit` entry per `metrics.md`.
8. Gates as step 0 plus the integration tests green. Commit
   (`feat:`/`test:` + trailer).

## Step 2 — Docs (commit 3, `docs:`)

1. `docs/e4-guides.md`: the emission rule (`semantics.md` content adapted
   to stage-doc style), criteria table, status pointer to `results.md`.
2. `docs/roadmap.md`: E4 entry — mark writer + topology criteria landed
   with the measured numbers; parasitics/STA criteria stay open for the
   next tickets.
3. `docs/status.md`: add/advance the E4 row if the doc structure calls for
   it (follow existing row shape).
4. Commit (`docs:` + trailer).

## Risks / notes for the implementer

- The 0.70 overlap bound is provisional by design — the likeliest
  shortfall is strip-vs-per-gcell box granularity or stub-layer
  differences, both visible in the per-net share distribution before any
  bound is touched.
- `read_guides` may warn `GRT-0008` — it is stale (P-GR), not an error;
  only non-zero exit / missing nets fail the check.
- Keep `route` printing the same congestion summary as `congestion` — the
  shared helper should make the two arms differ only in the guide tail.
- No SDC is loaded; the clock-band note in `semantics.md` is the honest
  approximation statement for `results.md`.
