# Implementation steps — revision fix pass 2

The feature is implemented (`kurena: implement saif-power`, v0.1.96, still
in-flight on this branch). This pass applies the second rejected-review
memo: two `vcd2saif.py` bugs and two `saif.rs` leniency nits. No semantic
change to `mb.saif` is expected — both converter fixes are no-ops on this
input. One logical unit per commit; Conventional Commits + agent trailer
per `AGENTS.md` §5. No version bump — `0.1.96` already carries this
in-flight change.

## Step 1 — `saif.rs` leniency nits (no container needed)

Start here: independent of the Docker run (`AGENTS.md` — permission-free
work first).

- `parse_saif`: `DURATION` atom → `v.parse::<u64>()` (was `f64`) — float
  literals (`1980.0`, `1980.5`) and negatives become hard errors, matching
  the `<int>` grammar and OpenSTA's rejection. `duration_s` computation
  and the `<= 0` check stay (catches `DURATION 0`).
- `tokenize` `"` branch: on `\`, push the next byte literally and skip
  both — same escape convention as atoms (`\"` → `"`, `\\` → `\`).
- Tests: `test_fractional_duration_is_error` (`(DURATION 1980.5)` and
  `(DURATION 1980.0)` both error); `test_quoted_string_escapes` (a
  `"a \"b\" c"` header string parses to one atom `a "b" c`; `\\` → `\`).
- `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`.
- Commit: `fix: reject fractional SAIF DURATION and handle quoted escapes`.

## Step 2 — `vcd2saif.py` fixes + regeneration check (container work)

- Fix 1: `name.rstrip("$end").strip()` → `words[4].removesuffix("$end")`
  (verify `python3 --version` in the image ≥ 3.9 first; fallback is the
  `endswith` + slice form — `fixture.md` §Fix 1).
- Fix 2: `parse_vcd` returns `(root, ts_num, ts_unit, first_ts, last_ts)`;
  emit `(TIMESCALE {ts_num} {ts_unit})`, drop `timescale_s` (no consumer),
  `die()` on an unknown unit instead of a `KeyError` traceback
  (`fixture.md` §Fix 2).
- Run `bash tests/designs/microbench/generate_saif_ref.sh` end-to-end
  inside `ghcr.io/librelane/librelane:3.0.16` (Colima). Then
  `git diff tests/designs/microbench/mb.saif
  tests/designs/microbench/mb.saif.reference.json
  tests/designs/microbench/mb.saif.openroad.txt` must be empty —
  byte-identical regeneration is expected (`probe.md` P3). Any diff is
  investigated and explained in `results.md` before committing.
- Commit: `fix: strip literal $end suffix and emit parsed timescale in
  vcd2saif` (converter only if the regen is clean; otherwise the
  regenerated artifacts ride with the explanation).

## Step 3 — `docs/results.md` record

Update the 2026-11-19 entry (still in-flight on this branch — correct in
place):

- one line per `probe.md` P4 item 8: the two converter fixes, the
  container `python3 --version`, and the regen outcome (byte-identical
  expected; if not, what changed and why);
- the two reader tightenings (`DURATION` integer-only, quoted escapes);
- "Checks run" refreshed with the latest full-suite rerun.
- Commit: `docs: record vcd2saif fixes and SAIF reader tightenings`.

## Verification gates

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`
- `bash tests/designs/microbench/generate_saif_ref.sh` idempotent (rerun →
  clean `git status`)
- `grus-eda power --saif tests/designs/microbench/mb.saif` end-to-end CLI
  run still clean (stderr shows `saif_unmatched_scopes = 1` from the ghost)
- The two new saif.rs tests are proven red-then-green on the un-fixed
  reader during development (fail-proof, AGENTS §7 rule 7).

## Out of scope (stays open)

- `when`-qualified `internal_power` blocks (`conditional_power_blocks`
  diagnostic already counts them).
- Matching OpenSTA's statistical activity propagation on large designs
  (gcd-class divergence, already documented for VCD).
- State-dependent leakage from `T0`/`T1` duty (Liberty `leakage_power`
  `when` conditions are not modeled; flat `leakage_power_w` stands).
