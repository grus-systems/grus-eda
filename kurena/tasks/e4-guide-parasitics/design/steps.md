# e4-guide-parasitics — Steps

One ticket, four logical commits (repo rule: commit as soon as a unit is
done). Version bump `0.3.9` → `0.3.10` (patch — new feature, no breaking
surface) rides in commit 2.

## Step 0 — Probe (commit 1, `docs:`)

1. Container up per `AGENTS.md` §2; run the `probe.md` setup Tcl (the
   two `set_wire_rc` variants, the `_000_` 1-net case, and our
   `gcd.ours.guide` read-back for the uncovered-pin question). Scratch
   under `tests/designs/gcd/runs/probes/` (gitignored).
2. Answer Q1–Q9 from the dumps; write the probe block into
   `docs/results.md` and the resolved model rule (all knobs fixed) into
   a new parasitics section of `docs/e4-guides.md` — the rule in `docs/`
   before the code (§7.6). If the arithmetic stays ambiguous, read the
   pinned OpenROAD `est/` source to explain it and say so explicitly.
3. Commit (`docs:` + `Co-Authored-By` trailer).

## Step 1 — Builder + CLI (commit 2, `feat:`)

1. `congestion/guide.rs`: `resolve_guide_nets` + unit test.
2. `sta/parasitics.rs`: `estimate_guide_parasitics` per the resolved
   `model.md`; `sta/mod.rs` re-export.
3. `main.rs`: `ParasiticsMode::Guide`, `StaArgs::guide`, the
   `"guide RC"` mode arm and the `--guide` misuse bail; the no-`--guide`
   path routes internally via `route_design` + `build_guides`.
4. Unit tests per `tests.md` (builder fixture, resolver, SPEF
   round-trip).
5. `Cargo.toml` `0.3.9` → `0.3.10`; `cargo check` refreshes `Cargo.lock`.
6. Gates: `cargo test --workspace` green; `clippy -D warnings` clean.
7. Commit (`feat:` + trailer).

## Step 2 — References + comparison tests + direction (b) (commit 3)

1. `tests/designs/guide_caps.py` (extract + diff modes).
2. Append step 5d to both `generate_ref.sh`; update header comments.
3. Run **both** scripts whole in the pinned image (picorv32 ~8 min).
   `git diff` must show only the new `.guide.caps.txt` files and the
   script edits — any other regenerated-file drift is investigated, not
   committed blind (E0 lesson 1).
4. `picorv32/check_guide_readback.sh` (gcd script mirrored).
5. `tests/designs/README.md` artifact lists.
6. Integration tests (`tests.md`): both designs' (a) gate, determinism.
   Run `--nocapture` and capture the stats tables for results.
7. Direction (b): `route --guide-out` on both placed DEFs, both
   `check_guide_readback.sh` runs, `guide_caps.py diff` each — capture
   tables.
8. Fail-proof mutation + restore (`tests.md`).
9. Gates as step 1 plus the new integration tests green.
10. Commit (`feat:`/`test:` + trailer).

## Step 3 — Comparison record + docs (commit 4, `docs:`)

1. `docs/results.md`: dated `e4-guide-parasitics` entry per
   `metrics.md` — probe reference (or fold step 0's block in here if it
   fits one entry), (a) and (b) tables per design, fraction inside
   `max(5%, 1 fF)`, worst offenders with root cause, gate verdict,
   runtime, the fail-proof record, "what was run".
2. `docs/e4-guides.md`: ensure the parasitics section and the criteria
   table reflect the landed model and the measured verdict (keep in step
   with code, §7.8).
3. `docs/roadmap.md`: E4 progress line — criterion 3 measured; criterion
   4 remains.
4. `docs/status.md`: E4 row — update tolerances/status/known gaps in the
   existing row shape.
5. Commit (`docs:` + trailer).

## Risks / notes for the implementer

- The likeliest probe surprise: OpenROAD charges pin access/stub
  capacitance or via resistance the default model omits — that is what
  knobs N3–N6 are for; fix the model, not the bound.
- `picorv32.guide.caps.txt` regenerates deterministically but depends on
  gitignored `picorv32.placed.def`; document that dependency in the
  results entry (the worktree seed covers tests).
- Direction (b) on picorv32 runs `read_guides` on a ~63 k-box file —
  expect `GRT-0008` (stale warning) and possibly `EST-0026` pin
  warnings; warnings are not failures, but count them in the record.
- Keep the shared `ParasiticsMode` change invisible to `power`: its
  catch-all bail already rejects `Guide`; confirm the message reads
  sanely rather than pattern-matching weirdly.
