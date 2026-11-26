# e5-cts-e2e — Steps

Three commits, each self-contained. Per AGENTS.md §5: no `git add -A`,
explicit paths only; version bump rides the functional commit.

## Commit 1 — `test:` ours-side CTS artifacts + generator

1. Write `tests/designs/generate_cts_ours.sh` (`artifacts.md`; `chmod +x`).
2. Run it for `gcd` and `picorv32`; confirm `<d>.cts.ours.def` +
   `<d>.cts.ours.report.json` emit; note sizes and sha256.
3. `.gitignore`: add `tests/designs/picorv32/picorv32.cts.ours.def` +
   `.report.json` under the existing picorv32 block.
4. `tests/designs/README.md`: artifact rows + regenerated-by note.
5. Regenerate `picorv32.cts.def` via `generate_cts_ref.sh picorv32`
   (LibreLane container); `git diff` the two committed picorv32
   `*.json` goldens — empty expected; drift is a finding to record,
   not commit (E0 lesson 1).
6. `.github/workflows/ci.yml`: the `generate` step + upload line for
   `picorv32.cts.def` (`artifacts.md` §CI).
7. Re-run `generate_cts_ours.sh gcd`; `git diff` on the committed
   pair must be empty. Commit: script, `.gitignore`, `README.md`,
   `gcd.cts.ours.def`, `gcd.cts.ours.report.json`, `ci.yml`.

## Commit 2 — `test:` e2e gates + decomposition + version bump

8. `cts/mod.rs` `mod tests`: factor `cts_sta_run`; add `CtsRefJson`
   (serde Deserialize), `CtsTol` (+`GCD_CTS`/`PICORV32_CTS` with
   cause-naming doc comments), `within_tolerance`, endpoint/clock-pin
   compare helpers, census print.
9. Add tests (`tests.md`): `test_gcd_cts_e2e`,
   `test_picorv32_cts_e2e`, `test_<d>_cts_def_deterministic`,
   `test_<d>_cts_decomposition`.
10. Run `cargo test -p grus-core cts -- --nocapture`; collect the
    measured numbers.
11. Outside-bound handling: run the decomposition, assign causes per
    `decomposition.md`, hold measured values in documented
    `PICORV32_CTS_*`/`GCD_CTS_*` constants. **Do not** silently widen.
    If a failure is diagnosed as the polarity-tie case, apply the
    `analysis.rs` `pub(crate)` escalation (`tests.md` §Contingency)
    and record it.
12. Fail-proof: the no-`run_cts` mutation (and the latency_min swap)
    → confirm red → restore. Record which assertions fired.
13. `Cargo.toml` `0.5.0 → 0.5.1` (patch — additive tests/artifacts/
    docs; choice recorded in the commit body).
14. `cargo test --workspace`, `cargo clippy --workspace --all-targets
    -- -D warnings`. Commit `cts/mod.rs` + `Cargo.toml`
    (+ `analysis.rs` only if the contingency fired).

## Commit 3 — `docs:` verdict records

15. `docs/results.md`: the e2e block (`docs.md`) — measured values
    only, commands, hashes, fail-proof record, decomposition table.
16. `docs/status.md` E5 row: per-criterion verdicts + floors + gaps.
17. `docs/e5-cts.md`: §1 row, new e2e section, §6 → `0.5.1`.
18. `docs/roadmap.md`: E5 line. Commit the four docs.

## Ordering notes

- `picorv32.cts.def` (step 5) must exist before step 10 or the
  picorv32 decomposition test panics via `require` — the panic names
  the generator.
- The determinism test's committed-DEF arm makes commit 1 a
  prerequisite of commit 2 — the order is load-bearing.
- Runtime: picorv32 adds ~2 STA runs + 2 `run_cts` per test over the
  existing suite (~seconds each); the gate + decomposition both run
  in the default `cargo test` suite (no `#[ignore]`).
