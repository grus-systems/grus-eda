# async-arc-probe — Steps

Ordered. Steps 0–2 produce the measurement; step 3 is the branch; steps 4–5
land docs + version. One logical commit at the end (or two: fixture+probe,
then docs+tests — implementer's call; keep fixture artifacts in the commit
that adds them).

## Step 0 — Fixture + container run

Create `tests/designs/sta_edge/async_arcs/` per `fixture.md`: DEF (14 cells,
13 nets, 6 ports — fixture.md is authoritative), `ideal.sdc`,
`propagated.sdc`, `generate_ref.sh` (golden
JSON + `run_probe` per `probe.md`, both modes).

Run `bash tests/designs/sta_edge/async_arcs/generate_ref.sh` (Colima/Docker
route per AGENTS.md §2). Then rerun it and confirm `git status` on the
fixture dir is clean — the golden must reproduce byte-identically before it
is committed (AGENTS.md §7.1; CI regenerates every `sta_edge` script and
diffs).

## Step 1 — Our-side runs

`grus-eda sta` on the fixture per `probe.md` "Our-side runs" — both SDCs,
`--json` captured, plus one `--strict` invocation. Record endpoint list,
`recognized_check_arcs`, `skipped_endpoints` (pins/reasons/related), arc
shares, strict verdict — verbatim.

## Step 2 — `docs/results.md` entry

Write the dated entry per `docs-update.md` §4 while the probe output is at
hand. Include the census and every Q answer verbatim. This entry exists
under both branches.

## Step 3 — Branch

- **A (no propagation, expected):**
  - Endpoint sets equal? → add `test_sta_edge_async_arcs_ideal` and
    `test_sta_edge_async_arcs_propagated` next to the other
    `test_sta_edge_async_*` tests in `analysis.rs`, `Tol::STRICT`, docstring
    naming the unclocked-`CLK` discriminator and the dfbb/dfstp coverage.
  - Endpoint sets differ only by `non_seq` pins (Q4)? → still add the tests
    *iff* a compare mode can pin the shared set honestly; otherwise commit
    the fixture without a compare test and record the non_seq residual —
    do not weaken the assertion to hide the count gap.
  - Prove the test can fail (AGENTS.md §7.7): temporarily move `Clear` back
    into `unsupported_arcs` or drop a check arm and confirm red; restore.
  - `docs/roadmap.md`: R6 → Done per `docs-update.md` §3-A.
- **B (propagation):** no compare tests (our model diverges by design);
  `docs/roadmap.md` R6 gains the measured rules + sequenced plan per
  `docs-update.md` §3-B. The fixture + goldens still commit — they are the
  follow-on ticket's golden.

## Step 4 — Roadmap + README

`docs/roadmap.md` R3 and R4 → Done per `docs-update.md` §1–2 (evidence:
`test_sta_edge_hold_violation_{ideal,propagated}`,
`test_sta_edge_clock_gate_{ideal,propagated}`,
`test_gate_kind_classifies_liberty_functions`; results.md entries 2026-11-17
v0.1.61 / v0.1.66). `tests/designs/README.md` fixture line per §5.
`README.md` untouched (note stays accurate).

## Step 5 — Verify + version

- `cargo test --workspace` — green including the new tests (Branch A).
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- `cargo test --release -p grus-core sta` if the run environment offers it
  (`workflow.toml` `sta-regression` — the sta_edge tests live there).
- Version `0.2.9 -> 0.2.10` (`Cargo.toml` + `cargo check` for `Cargo.lock`),
  same commit.
- Commit: `test:`/`docs:` conventional type in English, explicit paths,
  agent trailer. Name what ran (scripts, test counts) in the body and which
  branch the measurement took.

## Acceptance cross-check

- [ ] OpenSTA Clear/Preset behaviour measured + verbatim → Step 0/2, Q1–Q2
- [ ] Falling recovery/removal vs existing lib variants → census + Q3
- [ ] Our behaviour recorded verbatim → Step 1/2
- [ ] R6 sequenced plan *or* closed as matching → Step 3
- [ ] R3/R4 marked DONE + no regressions + version bump → Steps 4–5
