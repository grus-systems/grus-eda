# Implementation & verification steps

## Step 1 — Fixture and scenario changes

1. Apply the `exceptions.def` edits from `fixture.md` (components, pins,
   nets). Sanity: `u_ff1/D` is reachable from `u_ff0` and `u_ff2` through
   `u_mux`; `u_ff2/CLK` is on `c2` driven by `u_clkb2`.
2. Insert the `sel` constraint lines into every `.sdc` file in
   `tests/designs/sta_exceptions/` at the anchors given in `fixture.md`
   (all 67 files contain both anchor lines).
3. Create the seven new scenario files from `fixture.md` section 3.
4. Append the seven names to `SCENARIOS` in `generate_ref.sh`.

## Step 2 — Regenerate the golden

Run the committed script as written from the repo root:

```
bash tests/designs/sta_exceptions/generate_ref.sh
```

It re-invokes itself inside `ghcr.io/librelane/librelane:3.0.16` (docker via
Colima; start it first if needed per `AGENTS.md` section 2). Every value in
the new `exceptions.reference.json` must come from that run — never edit it
by hand. `git diff` the regenerated file before committing.

## Step 3 — Inspect the new goldens before running the test

Check, in `exceptions.reference.json`:

- `m2_both`/`pm2_both` `u_ff1/D`: the reported max-side candidate should be
  the delay check (`slack ≈ N - delay - unc - t_setup`, well below the
  competing clock-check slacks). For `pm2_both`, compare `max_arr_ns`
  against `base_prop`'s `u_ff1/D` value: if the delay winner's
  `launch + delay` differs from the plain arrival winner, the
  delay-winner-vs-arrival-winner split actually materialised.
- `m2_min2`/`pm2_min2`/`pm2_prop`: delay check is the reported candidate on
  its side.
- `sel` did not take over a delay scenario's report (slack ordering
  sanity). If `sel`'s slack undercuts a delay check on some scenario, tune
  its `-min`/`-max` per `fixture.md` and re-run step 2.
- `fp_uf0_uf1`: `u_ff1/D` is now present (survives via `u_ff2`/`sel`).

## Step 4 — Update `test_sta_exception_diagnostics`

Two asserts in `crates/grus-core/src/sta/analysis.rs` (~lines 8514-8536)
hardcode single-source behaviour and will fail:

- `fp_uf0_uf1`: `u_ff0 -> u_ff1` now covers only one of three launch
  sources. Expected: `skipped.false_path_endpoints == 0` and `u_ff1/D`
  **present** in `report.endpoints`. Update the assert and the comment
  (partial coverage, not a full removal). To keep full-removal coverage in
  the test, assert `fp_to_y` instead removes `y`
  (`false_path_endpoints == 1`, `y` absent — `-to [get_ports y]` covers all
  sources, and `sel` does not reach `y`).
- `exc_diag`: the `-setup 2` shift on `u_ff0 -> u_ff1` is still applied to
  that class's candidate, but the report now takes the min-slack candidate
  across sources — an unshifted `u_ff2`/`sel` clock check (req ~2.562147)
  wins over the shifted 4.562147. Update the assert to the winner's
  required time (verify the exact number against the run) and the comment;
  the `unresolved_exception_selectors == 4` assert is unchanged.
- `fp_from_din` (`== 2`, `u_ff1/D` present) and the `mc_both`
  `ignored_commands` assert are unaffected. Also refresh the stale test
  doc comment ("three-flop DEF plus 19 SDC scenarios").

This is a required consequence of the fixture change, not an optional
`analysis.rs` edit — the goal's "only if a real divergence" clause covers
production code.

## Step 5 — Test, then prove the new golden can fail

1. `cargo test -p grus-core test_sta_exceptions` — both the golden
   comparison and the diagnostics test must pass at the existing STRICT
   tolerances (rel 1 %, abs 5 ps). No tolerance edits.
2. Fail-proof check (AGENTS.md lesson 7): break the launch pairing on
   purpose — e.g. compose `arr`/`req` from the arrival winner's launch
   instead of `delay_launch_max`, or merge the wrong launch in
   `PinTimingState::merge` — and rerun. `pm2_both` (and `m2_both` if the
   delays differ) should go red **if the skew actually separated the two
   winners**. Restore the code.
   - If it stays green: the golden does not discriminate the pairing (the
     two winners coincide, or the skew is below tolerance). Record that
     honestly in `docs/path-delays.md`/`docs/results.md` — do not claim the
     pairing is golden-verified. Optional knob to enlarge the skew: add a
     dummy clock load on `c1` (e.g. a `sky130_fd_sc_hd__clkbuf_1`
     `u_clkload` with only `( u_clkload A )` on `c1`, `X` unconnected) and
     regenerate — only if the user/reviewer wants the stricter claim.
3. If the golden instead shows a **real divergence** between our
   composition and OpenSTA on the new scenarios: fix the delay merge /
   launch pairing in `analysis.rs` per the goal (separate concern from the
   assert updates). If the divergence is a fixture/model limit, record it
   in `docs/results.md` — no silent tolerance changes.

## Step 6 — Docs and commit

- `docs/results.md`: new dated entry — scenario names added, the regen
  command actually run, pass/divergence status per new scenario, the
  winner-pairing outcome (manifested or not), the `sel` delay choice and
  its rationale, which legacy `u_ff1/D` probes are now masked by competing
  sources, and the fail-proof result.
- `docs/path-delays.md`: section 4 honest-coverage note updated to the new
  scenario count and what the two-source merge now covers; section 5 item 3
  marked resolved (name the scenarios); item 1 amended — skewed launches
  via split clock buffers are now covered (`pm2_min2`), a literal
  two-clock-domain fixture stays open.
- `docs/timing-exceptions.md`: the fixture line near section 4 ("3-FF
  design, 19 SDC scenarios") is stale — update to the mux/second-buffer
  topology and current count.
- `Cargo.toml`: bump `[workspace.package] version` 0.1.86 -> 0.1.87 in the
  same commit as the `analysis.rs` test edits.
- Commit once the tests pass: `add:`-style message describing the fixture
  extension, the seven scenarios, the regenerated golden and the test/doc
  updates; record decisions (one-command `-from` for the merged delay
  domain, `pm2_both` addition, `sel` delay values) in the body. Append the
  agent trailer. If a production fix was needed in step 5, commit it
  separately.

## Verify commands (workflow.toml)

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`

## Risk checklist

- **Skew too small to discriminate pairing** → fail-proof stays green;
  document honestly, optional `c1` dummy-load knob.
- **`sel` masks a delay scenario's report** → tune `-min`/`-max`, regen.
- **`get_cells {u_ff0 u_ff2}` parse**: supported both sides (our parser:
  `CellSel::Cells(["u_ff0","u_ff2"])`; OpenSTA: a Tcl list) — if a syntax
  hiccup appears, `[list [get_cells u_ff0] [get_cells u_ff2]]` is the
  fallback spelling.
- **Container/PDK**: docker must be running; the PDK comes from
  `~/.ciel` (`PDK_ROOT` override supported by the script).
- **Regenerated goldens shift unrelated values**: expected — `u_ff1/D`
  arrivals gain the mux delay and the `sel` path everywhere; `u_ff0`/`u_ff1`
  propagated clock arrivals drop slightly (`u_clkb` now drives 2 loads
  instead of 3). Diffs are legitimate as long as the test matches.
