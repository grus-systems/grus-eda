# Implementation & verification steps

## Step 1 — Scenario files

1. Create the four `.sdc` files per `scenarios.md`: copy `base_prop.sdc`,
   append the one `set_max_delay`/`set_min_delay` command at the end (the
   `set_propagated_clock` line is already last in `base_prop.sdc` — keep
   the new command after it, matching `pmd_*`/`pm2_*` ordering).
   `pmp_port2` additionally changes `set_clock_latency 0.6` to
   `set_clock_latency 1.1` — the only preamble line it may touch.
2. Append `pmp_port pmp_minport pmp_port2 pmp_alls` to `SCENARIOS` in
   `tests/designs/sta_exceptions/generate_ref.sh`.

## Step 2 — Regenerate the golden

```
bash tests/designs/sta_exceptions/generate_ref.sh
```

from the repo root (it re-execs inside
`ghcr.io/librelane/librelane:3.0.16`; start Colima first if needed per
`AGENTS.md` section 2). Every value must come from this run. Then `git
diff` the golden: expected **additive only** — four new keys, no changes
to existing scenario values.

## Step 3 — Inspect the new goldens before running the test

Check in `exceptions.reference.json` against `scenarios.md`'s prediction
tables:

- `pmp_port.u_ff0/D.max_arr` — 0.8 confirms the delay check shares the
  ordinary propagated port seed (H-src/H-prop); 1.4 or ~0.87 means a
  different anchor — decompose per-transition before any code change.
- `pmp_port2` rows vs `pmp_port` rows — must be identical; any diff
  means the delay-check anchor carries the network latency.
- `pmp_minport.u_ff0/D.min_arr` — 0.1 vs 0.7 / ~0.17 same discrimination
  on the hold side.
- `pmp_alls` — `u_ff0/D` and `u_ff2/D` delay-checked by `d_in`, `u_ff1/D`
  by `sel` alone, `y2` by `sel` with `req = 0.25`, `y` unchanged. If
  `u_ff1/D`/`y2` look uncovered, inspect whether OpenSTA treated
  `[all_inputs]`'s `clk` member oddly before deciding anything.
- Record which hypothesis the golden confirmed — it feeds the
  `docs/results.md` entry and the `path-delays.md` resolution.

## Step 4 — Reconcile (only if the golden diverged)

- Expected: `pmp_*` arrivals equal the `base_prop` port seeds and the
  existing composition already passes — no production change.
- If `u_ff0/D` delay-check arrivals differ from the ordinary port seed:
  the fix site is the composition's covered-candidate arrival
  (`analysis.rs:3197-3255`) or, if the anchor *value* is wrong for delay
  checks only, a dedicated anchor path — `latency_ns` (`sdc.rs:330`)
  itself is shared with ordinary checks that `base_prop` verifies, so
  editing its propagated arm must also keep `base_prop` green. If the
  behaviour cannot be modelled cleanly, record a measured model limit in
  `docs/results.md` instead of approximating.

## Step 5 — Test strict, then prove the new golden can fail

1. `cargo test -p grus-core test_sta_exceptions_match_opensta` — all 105
   non-error scenarios at rel 1 % / abs 5 ps, no tolerance edits.
2. Fail-proof (AGENTS.md lesson 7): mutate the propagated port anchor —
   e.g. make `Sdc::latency_ns`'s `ClockRef::Design if
   self.propagated_clock` arm return `clock_source_latency_ns +
   clock_latency_ns` — rerun: `base_prop`, `pmd_*`, `pm2_*`, `pmo_*`,
   `pml_*` and all four `pmp_*` must go red (`pmp_port.u_ff0/D.max_arr`:
   ours ~1.4 vs ref 0.8). Restore.
3. `cargo test --workspace` and
   `cargo clippy --workspace --all-targets -- -D warnings`.

## Step 6 — Docs, version, commits

- `docs/path-delays.md`:
  - Header status line — add `pmp_*` to the scenario list.
  - Section 1 `set_max_delay` paragraph — resolve the "exact anchor TBD"
    note: under `set_propagated_clock` the measured port launch anchor is
    the SDC `-source` latency (identical to the propagated arrival at the
    `clk` port pin — `clk` is the propagation root, insertion there is
    zero; the two named candidates coincide on this fixture), and the
    delay check shares the port's ordinary propagated arrival
    (`pmp_*`/`base_prop` numbers).
  - Section 4 — scenario count 102 -> **106** entries (98 + 4 = **102**
    non-error comparisons; recount from the regen before writing) and the
    honest-coverage note: `pmp_*` exercises the propagated port launch
    anchor, with the caveat that SDC `-source` latency and the
    propagated `clk`-port-pin arrival coincide numerically because `clk`
    is the propagation root.
  - Section 5 — remove item 3 (propagated-clock input-port launch
    anchor) and record the resolution (the same "Resolved by ..." style
    used above it), including the H-src/H-prop degeneracy note.
- `docs/results.md`: dated entry `### 2026-11-19 — propagated-clock
  port launch anchor (`pmp_*`)` — the hypotheses table, scenario list,
  regen + test commands run, predicted vs measured numbers (which
  hypothesis won, whether our `input_clock_latency_ns` anchor matched or
  needed the fix), `pmp_port2` null-diff result, fail-proof result,
  regression runs.
- Test doc comment above `test_sta_exceptions_match_opensta`
  (`analysis.rs:8261-8263`): "plus 102 SDC scenarios" -> new count.
- `Cargo.toml`: `[workspace.package] version` 0.1.90 -> 0.1.91 in the
  same commit (`Cargo.lock` follows via build).
- Suggested commit split, each committed when green: (1) `test:` four
  `.sdc` files + `SCENARIOS` + regenerated `exceptions.reference.json`;
  (2) `docs:` doc updates + test comment + version bump. A production
  fix (step 4) commits separately before the docs.
- Append the agent trailer to each commit.

## Verify commands (workflow.toml)

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`

## Risk checklist

- **Predictions wrong** — `t_setup`/`t_hold` were derived from
  committed-golden rows, not re-measured under the new scenarios' slews;
  a few-ps drift is expected, a structural miss means stop and decompose.
- **`pmp_alls` selector surprise** — `[all_inputs]` contains `clk`; both
  engines should ignore it, but if OpenSTA resolves it differently the
  golden will show uncovered-looking rows or an error record. Diagnose
  first; `[all_inputs -no_clocks]` is the fallback, not a silent swap.
- **`pmp_port2` not identical to `pmp_port`** — means the anchor is not
  propagation-clean; investigate before the test run, not after.
- **Regen touches existing values** — expected additive-only diff;
  anything else means the script/fixture broke.
- **Anchor fix needed** — `latency_ns`'s propagated arm is shared with
  `base_prop`-verified ordinary checks; a delay-check-only divergence
  needs a separate path in the composition, and if no clean model fits,
  record the measured limit instead of forcing it.
- **Container/PDK** — docker via Colima; PDK from `~/.ciel`.
