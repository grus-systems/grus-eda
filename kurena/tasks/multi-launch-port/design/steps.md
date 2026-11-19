# Implementation & verification steps

## Step 1 — Fixture and scenario changes

1. Apply the `exceptions.def` edits from `fixture.md` (component, pin,
   nets). Sanity: `y2` is reachable from `u_ff0/Q` and `u_ff2/Q` through
   `u_mux2`; `sel`/`d0`/`d2` each gain exactly one sink; `COMPONENTS 8`,
   `PINS 5`, `NETS 11`.
2. Verify the two preamble anchors exist in all 97 `.sdc` files
   (`grep -L 'set_output_delay -max 0.7 -clock core_clk \[get_ports y\]'`
   and the `set_load` line must both print nothing), then insert the `y2`
   lines per `fixture.md` section 2 with `sed`.
3. Create the seven new scenario files from `fixture.md` section 3.
4. Append the seven names to `SCENARIOS` in `generate_ref.sh`.

## Step 2 — Regenerate the golden

Run the committed script as written from the repo root:

```
bash tests/designs/sta_exceptions/generate_ref.sh
```

It re-invokes itself inside `ghcr.io/librelane/librelane:3.0.16` (docker
via Colima; start it first if needed per `AGENTS.md` section 2). Every
value in the new `exceptions.reference.json` must come from that run —
never edit it by hand. Unlike the `mdo_*`/`pmo_*` regen, the diff will
*not* be additive-only: the new `u_mux2` sinks shift `d0`/`d2`/`sel`
loads, so `u_ff1/D` rows and propagated-clock numbers are expected to
move, and every scenario gains a `y2` row. Diff anyway and sanity-check
that shifts are small (ps scale), not structural.

## Step 3 — Inspect the new goldens before running the test

Check, in `exceptions.reference.json`:

- Winner identities per `overview.md`: `ml_both.y2` vs `ml_max0.y2` /
  `ml_max2.y2` (predicted: `ml_both` reports `u_ff0`'s candidate —
  identical arrival/slack to `ml_max0`, ~13.5 ps above `ml_max2`);
  `pml_both.y2.max_arr` vs `base_prop.y2.max_arr` (predicted equal —
  `u_ff2` arrival winner).
- `ml_all.y` equals `mdo_max1.y`; `ml_all.y2` equals `ml_both.y2`.
- `ml_min`/`pml_min`: `y2`'s reported min candidate is the delay check
  (`req = 1.0`, slack ≈ −0.1), not `sel`'s ordinary check (`req = 0.7`
  ideal / `0.1` propagated, slack ≈ +0.2). If masked, raise the
  `set_min_delay` value per `overview.md` and re-run step 2.
- Delay checks are the reported candidates on their sides; `sel` did not
  take over a delay scenario's max-side report either.

## Step 4 — Test updates in `analysis.rs`

- Doc comment above `test_sta_exceptions_match_opensta`
  (~line 8261): "three-flop DEF (muxed `u_ff1/D`, split clock buffers)
  plus 95 SDC scenarios" — refresh for the second mux/`y2` and 102
  scenarios.
- `test_sta_exception_diagnostics`: the `exc_diag` winner-req assert
  (`2.550057`, 5 ps tolerance) may drift — the `u_ff1/D` `t_setup` term
  sees the new `D`-pin slew from the added `d0`/`d2`/`sel` loads. Verify
  against the regen; update the literal and comment if needed (precedent:
  `4.562147 -> 4.550057` last task). The `fp_to_y`/`fp_from_din`/
  `fp_uf0_uf1`/`mc_both` asserts should hold unchanged — check anyway.

## Step 5 — Test strict, then prove the new golden can fail

1. `cargo test -p grus-core test_sta_exceptions_match_opensta` — all
   non-error scenarios at the existing tolerances (rel 1 %, abs 5 ps).
   No tolerance edits; a real divergence is fixed in the `OutputPort`
   composition or recorded as a measured fixture/model limit in
   `docs/results.md`.
2. Fail-proof (AGENTS.md lesson 7): mutate the covered-candidate pick —
   e.g. in the `OutputPort` delay arm keep only the first covered
   candidate or compose from the per-transition worst instead of the
   covered-source arrival winner — and rerun: `ml_both`/`pml_both` must
   go red (the ~13.5 ps ideal split and ~55 ps propagated split are both
   above tolerance). Restore the code.
3. Also run `cargo test --workspace` and
   `cargo clippy --workspace --all-targets -- -D warnings`.

## Step 6 — Docs, version, commits

- `docs/path-delays.md`: section 4 — update the scenario count (95 ->
  102) and the honest-coverage note: `y2` is now driven by two FF sources
  through `u_mux2`, so the covered-candidate winner rule is exercised per
  source at a port (`ml_both` vs `ml_max0`/`ml_max2`, `pml_both` under
  skewed launches). Section 5 item 2 marked resolved (name the
  scenarios). Section 1's output-port paragraph can gain a sentence
  noting the multi-launch winner is now golden-covered.
- `docs/results.md`: dated entry — fixture change (`u_mux2`, `y2`, net
  edits, A0/A1 rationale), the `set_load 0.05` symmetry choice, the
  `set_min_delay 1.0` masking rationale, scenarios added, the regen
  command run, winner outcomes (predicted vs measured identities and the
  ps splits), which existing values shifted, pass/divergence per new
  scenario, fail-proof result.
- `docs/timing-exceptions.md` (~line 145): the fixture description is
  stale ("3-FF design ... 72 SDC scenarios") — update for `u_mux2`/`y2`
  and the current count.
- `docs/roadmap.md` "Remaining" bullet: drop "multi-driver output-port"
  from the section-5 pointer (keep two-clock/latch fixtures and the other
  open items).
- `Cargo.toml`: bump `[workspace.package] version` 0.1.89 -> 0.1.90 in
  the same commit as the functional change (`Cargo.lock` follows via
  build).
- Suggested commit split, each committed when green: (1) `test:` the DEF
  + preamble edits + new scenario files + `SCENARIOS` + regenerated
  `exceptions.reference.json`; (2) `chore:`/`docs:` test doc-comment and
  assert-literal updates + docs + version bump. If a production fix was
  needed in step 5, commit it separately per the goal.
- Append the agent trailer to each commit.

## Verify commands (workflow.toml)

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`

## Risk checklist

- **Predicted winner wrong** — A0/A1 asymmetry at `u_mux2` or the skewed
  launch could land differently than the `u_mux` measurements suggest;
  the golden decides (regen inspect, step 3). Flipping the A0/A1
  assignment is the fallback only if `ml_both` does not decompose.
- **`ml_min` masked by `sel`** — if `sel`'s min slack still undercuts the
  delay check, raise `set_min_delay` and regenerate; do not loosen the
  assertion to accept `sel`'s ordinary check.
- **`set_max_delay` masked** — `sel`'s max slack (~+0.8) is far above the
  delay check's (~−1.1); verified in step 3.
- **`get_cells {u_ff0 u_ff2}` parse** — supported both sides (our parser:
  `CellSel::Cells([...])`; OpenSTA: a Tcl list), already used by
  `m2_both`/`pm2_both`.
- **Container/PDK** — docker via Colima; PDK from `~/.ciel` (`PDK_ROOT`
  override supported by the script).
- **Regen shifts an existing entry** — expected within ps scale (new
  `u_mux2` loads); a structural change (missing endpoints, error records
  appearing) means the fixture broke — stop and investigate.
- **`y2` missing from a scenario's endpoints** — would mean a `.sdc` file
  missed the preamble lines; the endpoint-count assert will catch it
  against our engine, which emits output ports unconditionally.
