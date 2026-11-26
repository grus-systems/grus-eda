# drt-selfdrc-exit — implementation steps

One commit; ~150 lines of diff, all in `main.rs` except script/docs.

1. **`DrtArgs` flags** — add `no_self_drc` (`overrides_with`) and
   `self_drc_tolerance` (`conflicts_with`); update `self_drc`'s help
   to `(default) …`. Update `cli_drt_parses` for the new surface.
2. **Reorder + verdict** in the `Drt` arm: compute the self-DRC
   subset right after `write_routed_def`; print `self_drc_verdict`
   after `guided=`; move the `self_drc`/`preemit_check` print blocks
   ahead of `iter_progress`; rewrite the `preemit_check` scope line;
   keep `wrote routed DEF` last; `bail!` on `findings_total >
   tolerance` with the message from `cli.md`.
3. **`cli_drt_selfdrc_gates_exit`** — new test per `tests.md`
   (clean/injected/tolerated/skipped arms).
4. **Prove-it-red** — disable the `bail!`, watch the injected arm go
   red, restore. Note the run for `docs/results.md`.
5. **`generate_drt_ours.sh`** — per-design tolerance:
   `case "$DESIGN" in picorv32) TOL=3 ;; *) TOL=0 ;; esac`, pass
   `--self-drc-tolerance "$TOL"` on both invocations, comment naming
   the pinned `met1.2` residual (`docs/e6-drt.md` §9). Header comment
   updated (self-DRC default-on).
6. **Docs** — `docs/e6-drt.md` edits per `docs.md`; `docs/results.md`
   dated entry; `docs/status.md` line.
7. **Version** — `[workspace.package] version` 0.13.1 → 0.14.0
   (breaking CLI behavior change).
8. **Verify** — `cargo test --workspace --profile verify` and
   `cargo clippy --workspace --all-targets -- -D warnings`; focused
   `cargo test -p grus-eda --bin grus-eda cli_drt_selfdrc` for
   iteration.

## Out of scope / follow-ups

- `check_drt_ours.sh` keep-list needs no edit (`self_drc*` prefix
  catches the verdict line); committed `cli_lines` goldens refresh on
  the next scheduled container regeneration — not part of this
  commit.
- Gating `unrouted>0` or `preemit_check` on the exit code —
  deliberately rejected, separate ticket.
- Per-rule tolerance budgets — rejected; the strict per-rule gate is
  `PINNED_LEF_RESIDUALS` in `tests/drt.rs`, the CLI budget is a count.
