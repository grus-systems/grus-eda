# e6-drt-e2e-picorv32 — steps (r1)

Ordered so the committed script predates the golden it emits, and
the un-ignore lands no earlier than the golden it joins. Container:
`ghcr.io/librelane/librelane:3.0.16` (cached,
`image_id sha256:02d50f259d…` recorded in the gcd golden).

## A. Prep commit — script fix + pin mechanism + version

Commit 1 (`fix:`):

1. `tests/designs/check_drt_ours.sh`: `unrouted_kinds` `mk is
   None` → `fail` inside the `emit_log_present` branch
   (`script-fix.md`). Exercise the fail arm on a doctored log
   copy; record the arm in results.
2. `crates/grus-core/tests/drt.rs`: `PINNED_LEF_RESIDUALS` →
   per-design `(design, rule, count)` triples, empty list
   (`gate.md` §2). Behavior-neutral; no test result moves.
3. `Cargo.toml` `0.12.8 → 0.12.9` + `Cargo.lock`
   (`docs.md` §4) — before the check leg so the golden records
   0.12.9.
4. `cargo test --workspace` + clippy — the only deltas are the
   fail arm (never hit on well-formed logs) and an empty const's
   new shape.

Optional within this commit or a later one: rerun
`check_drt_ours.sh gcd` — byte-identical JSON expected
(`image_id` drift aside); the inertness proof for the fix.

## B. Emit leg (detached, hours)

`emit-leg.md` §1 pre-flight; then:

```
bash tests/designs/generate_drt_ours.sh picorv32 --verify \
    > tests/designs/picorv32/runs/drt-ours/emit-driver.log 2>&1 &
```

Freeze `src/drt/*` + `main.rs` emit-path until the golden
commits. On completion: `OURS-SHA` (+ `OURS-DETERMINISTIC` if
`--verify` ran), the `guided=` census, `unrouted_kinds`,
`unrouted_blockers`, `self_drc findings_total`. If the first
emit's `route_ms` rules out a second pass, the skip decision is
recorded per `gate.md` §3 — never silently.

## C. Check leg (container, minutes)

```
bash tests/designs/check_drt_ours.sh picorv32
```

produces `picorv32.drt.ours.reference.json`. Read the verdict:

- `lef_expressible` all `ours == placed`, `magic_only` all
  inherited, `unclassified` ⊆ allowlist → **green leg**.
- Any `ours != placed` → `residuals.md` §2–§4 decomposition:
  sites via `drc listall` bboxes + `who*.tcl` probes, mechanism
  class, fix-or-pin.
- Step-E `fail` on a new unclassified name → deck change or a
  new violation class; investigate before anything commits
  (the abort is the designed loud failure).

## D. Leg commit(s) — golden + un-ignore + docs

Commit 2 (`feat:` or `test:`), or split per AGENTS commit
discipline:

1. `picorv32.drt.ours.reference.json` — fresh step-E output,
   `input_def_sha_prefix` matches `shasum` of the measured DEF.
2. `picorv32.drt.ours.routed.def` — only if `<1 MB` + the
   gitignore exception line (`emit-leg.md` §4).
3. `crates/grus-core/tests/drt.rs` — un-ignore
   `test_e2e_ours_reference_picorv32`, doc-comment update;
   picorv32 pin entries if the leg pinned residuals (named
   reason per entry); `test_e2e_ours_def_deterministic_picorv32`
   (`#[ignore]`d) only if the DEF committed.
4. `.gitignore` — the `!tests/designs/picorv32/…routed.def`
   exception iff the DEF commits (mirror the gcd comment).
5. `docs/results.md`, `docs/e6-drt.md`, `docs/status.md`
   (`docs.md`) — entry lands **with** the golden it describes.

If the leg pinned residuals: `cargo test --workspace` must be
green with the pins and the pin-scoping red-check (`gate.md` §4)
recorded. If the leg is red without a named mechanism →
`residuals.md` §5 escalation arm: commit the golden + the
measurement, keep `#[ignore]`, card it — do not ship an
un-pinned red test into the verify gate.

## Verify / diffreview checklist

0. `cargo test --workspace` — green; shape ~601+N passed /
   5–6 ignored (the picorv32 e2e gate now runs; the routing
   picorv32 legs stay ignored). Fits `timeout_sec = 7200` — the
   un-ignored test is a seconds-scale JSON join; no worktree
   `workflow.toml` edit (main-checkout copy binds the driver,
   `e6-drt-e2e/design/verify-timing.md`).
1. `cargo clippy --workspace --all-targets -- -D warnings` —
   clean.
2. Golden-vs-DEF join: `sha256(picorv32.drt.ours.routed.def)[:16]`
   == `input_def_sha_prefix`; `image_id` recorded not gated.
3. `git diff` clean on a step-E re-run (modulo `image_id`) —
   byte-reproduction per `golden.md`; a diff is a bug to
   investigate, not a rebaseline.
4. Diff audit vs this design: `check_drt_ours.sh` delta = the
   `unrouted_kinds` fail arm only; `drt.rs` delta = un-ignore +
   doc comment + per-design pins + optional ignored determinism
   test; no emit-path or router changes outside a `residuals.md`
   §5 fix arm (named if present); `.gitignore` delta = the
   DEF exception line at most; `docs/*` = the new entry +
   §9/status sync; `Cargo.toml`/`Cargo.lock` = 0.12.9.
5. `docs/results.md` numbers == the golden's (transcribed, not
   re-typed): spot-check the comparison table vs `delta.vs_*`
   and the census vs `ours_router`.

## Files

- `tests/designs/check_drt_ours.sh`
- `tests/designs/picorv32/picorv32.drt.ours.reference.json`
- `tests/designs/picorv32/picorv32.drt.ours.routed.def` (only if <1 MB)
- `crates/grus-core/tests/drt.rs`
- `.gitignore`
- `docs/results.md`
- `docs/e6-drt.md`
- `docs/status.md`
- `docs/roadmap.md` (optional)
- `Cargo.toml`
- `Cargo.lock`
- `tests/designs/picorv32/runs/` (gitignored — emit/check/probe artifacts)
