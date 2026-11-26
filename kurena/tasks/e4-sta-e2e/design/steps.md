# e4-sta-e2e — Steps

One ticket, three logical commits (repo rule: commit as soon as a unit
is done). Version bump `0.3.10` → `0.3.11` (patch — new test/reference
surface, no breaking change) rides in commit 2.

## Step 0 — Ours-side artifacts (commit 1, `test:`)

1. `tests/designs/generate_guide_ours_spef.sh` per `reference.md` §1.
2. Run it on gcd and picorv32 (needs `picorv32.placed.def` — present via
   the worktree seed or a prior `generate_ref.sh`).
3. `.gitignore` + `tests/designs/README.md` additions per
   `reference.md`.
4. Sanity: `sta --spef gcd.guide.ours.spef` hand-run reports 53
   endpoints (record the WNS in the commit body); `git diff` clean on a
   script rerun.
5. Commit (`test:` + trailer) — script, `.gitignore`, README, and the
   committed `gcd.guide.ours.spef`.

## Step 1 — Reference JSONs + tests (commit 2, `test:`)

1. Append step 9c to both `generate_ref.sh` scripts per `reference.md`
   §2, including the loud guard on the step-1 outputs and the header
   comment update.
2. Run **both** scripts in the pinned image. Full reruns are the honest
   path (picorv32 ~8 min); if only step 9c is rerun, name exactly that
   in the commit and results entry (§7.9). `git diff` must show only
   the script edits and the new `<d>.guide.sta.reference.json` files —
   any other regenerated-file drift is investigated, not committed
   blind (E0 lesson 1).
3. `Rc::GuideOursSpef` arm + the six tests of `tests.md` in
   `sta/analysis.rs`.
4. `Cargo.toml` `0.3.10` → `0.3.11`; `cargo check` refreshes
   `Cargo.lock`.
5. Run the new tests `--nocapture`; capture the per-field max-diff and
   decomposition tables for results.
6. Fail-proof mutation + restore per `tests.md`.
7. Gates: `cargo test --workspace` green; `clippy -D warnings` clean;
   `cargo test --release -p grus-core sta` green.
8. Commit (`test:` + trailer).

## Step 2 — Comparison record + docs (commit 3, `docs:`)

1. `docs/results.md` entry per `metrics.md` — probe conditions, gate
   tables, decomposition, attribution of any outside-bound endpoint,
   `EST-0026` counts, interop note, runtime, fail-proof, "what was run".
2. `docs/e4-guides.md`: E4 e2e row + the three-block golden design note.
3. `docs/status.md` E4 row: verdict, tolerances, refreshed known gaps.
4. `docs/roadmap.md` E4 progress line.
5. Commit (`docs:` + trailer).

## Risks / notes for the implementer

- **The gate may not pass clean.** Direction (b) in e4-2 showed ~2% of
  nets up to 87–131% cap error on our guides; whether that moves an
  endpoint past `max(5 ps, 1%)` is exactly what this ticket measures.
  If endpoints land outside the bound: run the decomposition first,
  name the cause per `metrics.md`, then decide — do not reach for a
  wider `Tol` without the named cause in `docs/results.md`.
- **OpenSTA may reject our SPEF** (`guide_ours_spef` block). That is a
  `write_spef` interop bug, not a reason to drop the block — fix the
  writer (it claims OpenSTA-compatible output) and regenerate.
- **`EST-0026` asymmetry is expected**: OpenROAD leaves warned pins
  dangling; our builder attaches them to the nearest node. On nets they
  hit, the `guide_ours_spef` − `guide_ours_readback` axis carries it —
  that is the named cause, not a defect to paper over.
- **`spef_parasitics` still cannot read `<d>.guide.spef`** (island pins,
  pre-existing strictness from e4-2). Do not "fix" it silently to gain
  the missing matrix cell — it is recorded as a known gap.
- **picorv32 ordering**: step 9c needs `picorv32.guide.spef` (step 5d
  regenerates it — gitignored, so a fresh worktree must run the full
  script anyway) plus both step-1 outputs.
- **No `main.rs` change is expected.** The whole ours chain is already
  CLI-expressible; if a hook turns out to be missing, keep it inside
  `crates/grus-core/src/sta/` per the goal and say why in the commit.
