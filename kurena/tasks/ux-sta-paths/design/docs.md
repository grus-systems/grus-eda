# ux-sta-paths — Documentation updates

## `docs/e9-reporting.md` (new)

The E9 reporting doc; this ticket writes its first section. Contents:

1. **`critical_paths` JSON schema** — the `api.md` §1 struct, verbatim field
   list, and the presence rule: the key is *absent* when `paths == 0` or no
   trace was produced (the `skip_serializing_if = "Vec::is_empty"` choice —
   not `[]`, not `null`).
2. **Selection semantics** — N worst endpoints per corner by the reported
   check slack (`max_slack_ns`/`min_slack_ns`), one trace each; how this
   differs from OpenSTA `-endpoint_path_count N` (which is N paths *per*
   endpoint); the multi-anchor-port approximation (ranked by the reported
   last-group check, matching the `endpoints` row).
3. **Stage semantics** — `edge` kinds (`clock`/`latch_open`/`port`/`pin`/
   `cell_arc`/`net`), that `arrival_ns` is the recorded physical pin arrival
   while `data_arrival_ns` is the reported check arrival (the two differ by
   the hold/xdomain realign shift — `overview.md` §4), and that launch stages
   stop at the register clock pin / latch enable (the clock network is not
   traced).
4. **Honesty rules** — `note` carries: un-traced predecessors (name the pin),
   ambiguous anchors, undeclared launches, `-from` segment checks, union-state
   approximations. Nothing is silently dropped.
5. **Corner model** — single-lib runs emit both corners from one state;
   `--lib-hold` merges `max` from setup + `min` from hold.

## `docs/README.md`

Add `e9-reporting.md` to the per-stage doc list in the reading order (after
`e3-sta.md` / near the end of item 3's list).

## `docs/results.md`

New dated entry `### 2026-11-23 — E9.1 — critical path traces (gcd)`:

- Reference command: `tests/designs/gcd/generate_paths_ref.sh` inside
  `ghcr.io/librelane/librelane:3.0.16` — record the exact `report_checks`
  flag line probed in step 0.
- Our command: the `sta --paths 3 --json` invocation + the test names.
- Table: per corner — endpoints traced, pin-sequence match (exact/named
  divergence), max per-stage delay residual, max arrival residual, slack/req
  residuals.
- Fail-proof record: the corrupted line, the failing test name, the observed
  red diff, and the restore.
- Named residuals if any (per AGENTS.md §2: root cause stated, no silent
  bound loosening).

## `docs/status.md`

E9 row: mark item (1) as **DONE** with the measured gcd bound (pin sequences
matched, max stage-delay residual in ps), keep items (2)–(5) OPEN under
`ux-sta-summary`/`ux-top-sdc`. Update the ticket list to reflect
`ux-sta-paths` closed.

## Not touched

- `README.md` stage table — E9 has no row there (the stage list is E0–E3
  plus roadmap); the E9 row lives in `docs/status.md`. Re-check at
  implementation time whether the README "skip summary" blurb should
  mention `--paths` — it documents the JSON surface, so add one clause if it
  reads naturally, else leave it.
- `docs/e3-sta.md` — the trace is reporting over the finished state, not an
  E3 semantics change.

## Files

- `docs/e9-reporting.md` (new)
- `docs/README.md`
- `docs/results.md`
- `docs/status.md`
