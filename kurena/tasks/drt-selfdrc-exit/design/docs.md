# drt-selfdrc-exit — documentation edits

## `docs/e6-drt.md`

### §8 CLI synopsis (~L793-797)

Update the synopsis line:

```
grus-eda drt --tlef <t> --lef <l> --def <placed.def> \
    --guide <d>.guide --out-def <routed.def> \
    [--reference <d>.drt.reference.json] [--self-drc | --no-self-drc] \
    [--self-drc-tolerance N] [--occ enabled|disabled] [--max-iters N]
```

and the prose that follows: self-DRC is default-on; `--no-self-drc`
is the explicit opt-out; the `self_drc_verdict` token list.

### §8 contract paragraph (~L830-832)

Replace "The CLI reports and never exits nonzero on findings; the
gate is `tests/drt.rs`." with the new contract:

- `routed=` is a **connectivity count** — all member pins joined —
  never a DRC claim; the DRC verdict of the same run is the
  `self_drc_verdict` line printed directly beneath it.
- Exit code: 1 when the emitted DEF's self-DRC `findings_total`
  exceeds `--self-drc-tolerance` (default 0); the DEF is still
  written. `TOLERATED`/`SKIPPED`/`CLEAN` exit 0. `unrouted>0` does
  not gate (named census, not a violation gate).
- `--no-self-drc` skips the check deliberately; `SKIPPED` marks the
  census so `routed=` cannot be misread as clean.
- `--self-drc-tolerance` exists for the named picorv32 residual;
  `generate_drt_ours.sh` pins it per design.

### §9 picorv32 outcome paragraph

Append one clause where the emit-time self-DRC (sees 3 of the 21
`met1.2` sites) is described: generation now runs with
`--self-drc-tolerance 3` pinned in `generate_drt_ours.sh` — the
subset-visible residual budget; the per-rule strictness stays with
`PINNED_LEF_RESIDUALS`/`test_e2e_ours_reference_picorv32`.

### §4 "is not" bullet (optional, one clause)

The `DRC-clean` bullet already names the gap; add that the emitted
DEF now carries a CLI verdict (`self_drc_verdict`) rather than only a
print — keeps §4 consistent with §8.

## `docs/results.md` — dated entry (2026-11-27)

Content:

- Change: `drt` self-DRC default-on, `self_drc_verdict` line moved
  ahead of the unrouted census, exit 1 on
  `findings_total > --self-drc-tolerance`; `--no-self-drc` /
  `--self-drc-tolerance` added; version bump.
- What ran: `cargo test --workspace --profile verify`,
  `cargo clippy --workspace --all-targets -- -D warnings`; the new
  `cli_drt_selfdrc_gates_exit` arms with observed outcomes (clean →
  Ok, `--occ disabled` → Err with parsed `findings_total`,
  tolerated/skipped → Ok); the prove-it-red run.
- Known follow-up: committed `*.drt.ours.reference.json`
  `ours_router.cli_lines` go stale (new `self_drc_verdict` line +
  moved `self_drc` block) until the next `check_drt_ours.sh`
  container regeneration; census numbers unchanged.

## `docs/status.md`

One-line prepend to the `Last updated:` chain (same style as the
existing `v0.12.x` entries): `v0.14.0` `drt-selfdrc-exit` — emitted
DEF self-DRC is default-on and gates the exit code;
`self_drc_verdict` line; picorv32 generation pinned at
`--self-drc-tolerance 3`.
