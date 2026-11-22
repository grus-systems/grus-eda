# Implementation Steps

Doc-only ticket. No code, no test, no golden changes. Version bump skipped
(doc-only changes do not bump `[workspace.package] version`).

1. **P-GR probe** — run the `probes.md` P-GR script in the pinned container
   on `gcd.placed.def`. Record: guide file head, `estimate_parasitics
   -global_routing` availability and its SPEF's per-net capacitance spread
   vs `gcd.routed.spef`/`gcd.rcx.spef`, and the `*guide*`/`read_guides`
   command surface. Decide E4 criterion tolerances and whether the
   "our-guides → reference's own parasitic estimator" comparison is usable.
2. **P-CTS probe** — enumerate `clock_tree_synthesis`/`report_clock_*`
   commands, run CTS on `gcd.placed.def`, record flag surface, inserted
   buffer count, insertion delay, setup/hold skew. Tighten E5 criteria 1–2.
3. **P-IRD probe** — check `analyze_power_grid`/`check_power_grid`/psm
   presence; if present, run on `gcd.placed.def` and record the worst-drop
   metric surface. If absent, E8 is documented as deferred for lack of a
   reference — say so plainly.
4. **P-DRC probe** — check `magic`/`klayout` in the image and whether magic
   `def read` + `drc count` works on `gcd.routed.def`. Decides whether E7's
   standalone scoping keeps an independent reference or collapses to
   paired-with-E6.
5. **Finalize the design docs** — fold every probe result into
   `done-when.md` (replace provisional tolerances with measured anchors),
   `inventory.md` (mark confirmed/absent commands), and `order.md` (flip any
   stage the probes gate out).
6. **`docs/results.md`** — one dated `next-stage-scoping` section: each
   probe's exact command, its output excerpt, and the decision it drove.
   Keep failed probes in the log.
7. **`docs/roadmap.md`** — add the new stage list (E4–E8) in the existing
   item style: per stage, inputs/outputs in one line, the measured
   done-when, the ticket sketch, and the order/gating note from `order.md`.
   Flag the README "out of scope" tension explicitly for the user's later
   decision; do not edit `README.md` (out of this ticket's file set).
8. **Verify** — `cargo test --workspace` and `cargo clippy --workspace
   --all-targets -- -D warnings` pass unchanged; `git status` shows only
   `kurena/tasks/next-stage-scoping/` + `docs/{roadmap,results}.md`;
   `git diff` on `tests/` is empty.

## Files

- `kurena/tasks/next-stage-scoping/design/`
- `docs/roadmap.md`
- `docs/results.md`
