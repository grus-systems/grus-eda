# drt-selfdrc-exit — overview

External review S2-1: `routed=N` + exit 0 can be read as "the emit is
clean", but self-DRC findings never reach the exit code. Today the
`drt` arm of `crates/grus-eda/src/main.rs` prints
`self_drc findings_total=N` (when asked) and still returns `Ok(())`.

## Spec-vs-code discrepancy (recorded, resolved)

`spec/goal.md` claims "current behavior runs self-DRC by default".
The code disagrees: `DrtArgs.self_drc` is an opt-in `bool` flag
(`main.rs` ~L716-720); only `tests/designs/generate_drt_ours.sh`
passes `--self-drc`. A bare `grus-eda drt` run today performs no
subset check at all. Leaving it opt-in would not fix S2-1 — a caller
that never passes the flag still reads `routed=N` + exit 0 on a dirty
emit. **Decision: self-DRC becomes default-on** (see `cli.md`).

## Decisions (details in sibling docs)

| # | Decision | Rationale |
|---|----------|-----------|
| D1 | Self-DRC runs by default; `--no-self-drc` opts out. `--self-drc` kept as the explicit-on spelling (clap `overrides_with` pair, last flag wins). | Only a default-on check fixes the misleading exit-0. The pair keeps existing invocations (`generate_drt_ours.sh` passes `--self-drc`) valid without a shim. |
| D2 | Nonzero exit = `anyhow::bail!` at the end of the `drt` arm → process exit 1 via `main`'s `Result` propagation. | Same mechanism as `sta --strict` failures; testable in-process through `run(Cli)` — no binary spawn needed. |
| D3 | `--self-drc-tolerance N` (default 0) **is** warranted: picorv32's pinned residual is `findings_total=3` (emit-visible `met1.2` sites — 3 of the 21 Magic sites, the rest intra-island; `docs/e6-drt.md` §9). `generate_drt_ours.sh` sets a per-design budget (picorv32=3, else 0). | `set -euo pipefail` in the script would otherwise abort every picorv32 emit run. The strict per-rule gate stays in `test_e2e_ours_reference_picorv32` (`PINNED_LEF_RESIDUALS`); the CLI budget is only a pipeline unblock, so a plain count suffices. |
| D4 | Census ordering: `self_drc_verdict <TOKEN>` printed immediately after the `guided=` census line, then the `self_drc` findings block — all before `iter_progress=` and the unrouted listing (spec option A). | Findings adjacent to `routed=` cannot be misread or buried. |
| D5 | `RouteOutcome::RoutedWithFindings` enum **rejected**: `route_nets`/`apply_routes` predate the re-parse; findings are a property of the emitted text, composed at the CLI layer. The honest representation is the verdict line + exit code, not engine state. | Adding a `findings` field to `EmitReport` would lie about provenance (the engine never runs the re-parse). |
| D6 | `unrouted > 0` stays exit 0; `preemit_check` stays report-only. | Both are loudly named already; gating connectivity is a different ticket. The `preemit_check` scope line's "until drt-selfdrc-exit lands" clause gets rewritten. |
| D7 | `--no-self-drc` keeps exit 0 and prints `self_drc_verdict SKIPPED`. | Opt-out must not be misread as clean either. |
| D8 | Workspace version 0.13.1 → 0.14.0. | Default-on check + exit gating is a breaking CLI behavior change (0.x rule: breaking → minor). |

## Exit-code contract (new)

`grus-eda drt` exits 1 when the self-DRC subset checker reports
`findings_total > --self-drc-tolerance` (default tolerance 0). The
routed DEF is still written and `wrote routed DEF to …` still prints —
the artifact is preserved; only the verdict fails. Exit 0 in the
CLEAN, TOLERATED, and SKIPPED verdicts.

## Blast radius

- `tests/designs/generate_drt_ours.sh`: gains a per-design
  `--self-drc-tolerance` (picorv32=3; the pinned `met1.2` residual).
  Without it picorv32 generation aborts under `pipefail`.
- `tests/designs/check_drt_ours.sh`: **no edit needed.** Its step-E
  keep-list already matches `self_drc*` line prefixes, so the new
  `self_drc_verdict` line lands in `cli_lines` automatically. The
  committed `ours_router.cli_lines` go stale (line order + new line)
  until the next container regeneration run — recorded in
  `docs/results.md`; no census numbers change.
- No `RouteDb`/`EmitReport`/`SubsetReport` changes.

## Files

- `crates/grus-eda/src/main.rs`
- `tests/designs/generate_drt_ours.sh`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `Cargo.toml`

`crates/grus-eda-core/tests/drt.rs` is **not** touched: the exit path
lives in the binary crate and is exercised through `run(Cli)` in
`main.rs`'s `#[cfg(test)]` module; the library-side proof that the
injection produces findings already exists
(`test_emit_fail_proof` Arm B, `occ=disabled` → `findings_total >= 1`).
