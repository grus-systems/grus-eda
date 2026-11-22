# mc-golden — Steps

Two commits is the natural split: the golden promotion (which
produces the `probe.sh` commit hash the results entry must name),
then the doc close-out. Version bump `0.2.5 → 0.2.6` (patch —
promotion, no API change) lands in the functional commit
(`AGENTS.md` §5).

## Step 0 — Golden rename + regen + test (commit 1, `test:`)

1. `probe.sh`: rename the two consolidated outputs to
   `*.reference.json` and fix the three comment/`echo` mentions
   (`goldens.md`). No other script change.
2. Container up if needed; run `bash
   tests/designs/sta_clocks/probe.sh` whole in
   `ghcr.io/librelane/librelane:3.0.16`.
3. `diff` `two_clock.reference.json` vs `two_clock.probe.json` and
   `two_clock_latch.reference.json` vs `two_clock_latch.probe.json` —
   byte-identical required; `git status` shows every
   `probe_t2_*.txt` unchanged. Stop on any diff (E0 lesson 1).
4. `git rm` the two `.probe.json`; `git add` the two
   `.reference.json` (rename recorded).
5. `analysis.rs`: the two filename pairs, `expect` wording, doc
   comment (`test.md`); add the PROBE_ONLY committed-file parse-gate
   loop.
6. `tests/designs/README.md`: `.probe.json` → `.reference.json` in
   the `sta_clocks` paragraph.
7. `Cargo.toml` `0.2.5 → 0.2.6`; `cargo check` refreshes
   `Cargo.lock`.
8. Gates: `cargo test -p grus-core sta_clocks` green; `cargo test
   --workspace` green; `cargo clippy --workspace --all-targets --
   -D warnings` clean.
9. Fail-proof mutations (`test.md` §"Fail-proof"): missing-file run
   red, perturbed-number run red, both restored and green again.
10. Commit (`test:` type + `Co-Authored-By` trailer). This commit's
    hash is the "`probe.sh` commit `<hash>`" for `docs/results.md`.

## Step 1 — Doc close-out (commit 2, `docs:`)

- `docs/path-delays.md` §5 item 1 → resolved record + header reword.
- `docs/multi-clock.md`: status header, evidence names, §2 heading,
  §3 item 5 tick (`docs.md`).
- `docs/results.md`: dated entry naming step-0's commit hash, the
  byte-identical regen proof, scenario count, STRICT pass, named
  deferrals, commands run, fail-proof runs.
- `docs/roadmap.md`: N2 multi-clock line closed.
- Gate: `cargo test --workspace` still green (docs-only commit —
  cheap sanity, not a substitute for step 0's gates).

## Final verification

- `cargo test --workspace` — kurena `test-workspace` verify.
- `cargo clippy --workspace --all-targets -- -D warnings` — kurena
  `clippy` verify.
- `cargo test --release -p grus-core sta` — the `sta-regression`
  acceptance line. The spec has no `## Verify` section, so the
  pipeline runs the two default verifies only; this run is manual
  and its result goes in the `results.md` entry (say what ran).

## If the regen diverges

A non-identical `reference.json` is a reproduction failure of the
committed script, not a golden to fix by hand: stop, diff, find the
cause, and come back to the card — same rule the v0.2.5 re-fold
followed.

## Files

- `tests/designs/sta_clocks/probe.sh`
- `tests/designs/sta_clocks/two_clock.probe.json` → `two_clock.reference.json`
- `tests/designs/sta_clocks/two_clock_latch.probe.json` → `two_clock_latch.reference.json`
- `tests/designs/README.md`
- `crates/grus-core/src/sta/analysis.rs`
- `docs/path-delays.md`
- `docs/multi-clock.md`
- `docs/results.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
