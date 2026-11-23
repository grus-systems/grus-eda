# e7-drc-e2e — implementation steps

Ordered; each step ends green. Commit per AGENTS §5 (proactive,
small, Conventional Commits, trailer). Version bump rides in the
same commit as the functional change that lands the CLI.

## Step 1 — `drc::report` + width report sibling

1. `width.rs`: extract the `check_width` rule loop into
   `check_width_report` (per `report.md`); `check_width` delegates
   via `flat_map`. No behavior change — existing tests must pass
   untouched.
2. `enclosure.rs`: derive `Serialize, Deserialize` on `EncMode`.
3. `drc.rs`: `pub mod report;`.
4. New `drc/report.rs`: `Family`, `Measured`, `Finding`,
   `RuleOutcome`, `SubsetReport`, `run_all`, `first_paren_id`,
   `RuleDelta`/`MagicDelta`/`magic_deltas`, deck constants,
   `claim_boundary`/`claim_boundary_from` — per `report.md`.
5. `tests/drc.rs`: add `test_subset_coverage_complete` and
   `test_subset_gate_matches_golden` per `gate.md`.
6. Fail-proof: drop one `WIDTH_RULES` row → coverage test must go
   red naming the missing `rule_name`; restore.
7. Commit: `feat: Add drc::report unified subset report`.

## Step 2 — `grus-eda drc` CLI + version bump

1. `main.rs`: `DrcArgs`, `Commands::Drc` arm, per-rule census +
   findings print, claim-boundary line, `--reference` delta table +
   unclaimed list, `--json` writer (CLI report struct per `cli.md`).
2. `tests/drc.rs`: extend `test_spacing_mutation_met2_probe` with
   the report-level parity assertions per `gate.md` §3.
3. Manual verification runs on both designs:
   `cargo run --release -p grus-eda -- drc --tlef ... --lef ...
   --def tests/designs/<d>/<d>.routed.def --reference
   tests/designs/<d>/<d>.drc.reference.json --json /tmp/<d>.drc.ours.json`
   — capture stdout for `docs/results.md` and confirm the JSON
   parses (`jq .rules | length` = 50).
4. `Cargo.toml` `[workspace.package] version` `0.11.0 → 0.12.0` +
   `Cargo.lock` (`cargo check` regenerates).
5. Commit: `feat: Add grus-eda drc subcommand for the
   lef_expressible subset` (includes bump).

## Step 3 — docs

1. `docs/e7-drc.md` §16–18 + §7 note per `docs.md`.
2. `docs/results.md` dated entry — real captured numbers only.
3. `docs/status.md` (Last updated + E7 row), `docs/roadmap.md`
   (E7 line).
4. Commit: `docs: Record e7-drc-e2e subset gate and drc CLI`.

## Verify (gate)

- `cargo test --workspace` — all green, including the two new gate
  tests and the extended mutation test.
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- `grus-eda drc` stdout shows the claim-boundary line with and
  without `--reference`; `--json` output deserializes and names 50
  rules.

## Risks / watch items

- **`run_all` cost**: spacing's pair sweep is the hot path; running
  all five families on picorv32 (~35 k cuts, dense met1/2) may take
  seconds — fine, but record the real number in `results.md`. If it
  surprises (minutes), stop and profile rather than ship silent.
- **Layer `None` rows**: rules whose layer is absent keep
  `layer: None` in `RuleOutcome`; the CLI prints `-` and the gate
  asserts the *rule_name* set, not resolvability — sky130 resolves
  all 50.
- **`magic_per_rule` join edge**: if a golden key ever names two
  implemented ids, first-token attribution assigns it once — the
  `unclaimed` bookkeeping makes a mis-attribution visible instead
  of silent.

## Files

- `crates/grus-core/src/drc/report.rs`
- `crates/grus-core/src/drc.rs`
- `crates/grus-core/src/drc/width.rs`
- `crates/grus-core/src/drc/enclosure.rs`
- `crates/grus-core/tests/drc.rs`
- `crates/grus-eda/src/main.rs`
- `docs/e7-drc.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
