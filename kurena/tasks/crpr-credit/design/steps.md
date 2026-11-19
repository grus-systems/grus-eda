# Implementation & verification steps

## Step 1 — Scenario files and `SCENARIOS`

1. Create the five `.sdc` files exactly per `fixture.md` (each is the full
   `base_prop.sdc` text + the two-derate spread block + its own lines).
2. Append `pcr_clk pcr_clkfp pcr_max pcr_min pcr_off` to `SCENARIOS` in
   `generate_ref.sh` (after `pmp_alls`).

## Step 2 — Regenerate the golden

```
bash tests/designs/sta_exceptions/generate_ref.sh
```

Run the committed script as written from the repo root (it re-invokes
itself inside `ghcr.io/librelane/librelane:3.0.16`; start Colima first per
`AGENTS.md` §2 if needed). All 111 scenarios must come from that one run —
state in `docs/results.md` that the full script ran. `git diff` the golden:
it must be additive-only (new `pcr_*` keys); any change to an existing key
is nondeterminism — stop and record it.

## Step 3 — Inspect the new goldens before the test

Extract the `pcr_*` rows (`python3`/`jq`, not by eye) and check against
`fixture.md` §4:

- `pcr_clk`: the req_max-down / req_min-up opposite-direction pair is
  present at `u_ff1/D`, `u_ff0/D`, `u_ff2/D` — this is the recorded proof
  that clock-net early/late spread exists. `u_ff1/D` max winner is still
  the `u_ff2` path (`max_arr` ~0.82, not ~0.76).
- `pcr_clkfp`: `u_ff1/D` `max_arr` ~0.757 (the `u_ff0` path — if it shows
  ~0.82 the false path did not bite; recheck the SDC). Record
  `max_slack`/`max_req`: they will be either un-credited or `+S` — this
  number *is* the ordinary-check credit measurement.
- `pcr_max`: `u_ff1/D` max row is the delay check (`max_req` ~1.0, not ~2.0
  — if ~2.0 the delay check lost the report; recheck winner identities).
- `pcr_min`: `u_ff1/D` min row is the delay check (`min_req` ~0.75, not
  ~0.25).
- `pcr_off`: `u_ff1/D` max row is the delay check (`max_req` ~1.0,
  `max_arr` ~0.82 — the `u_ff2` path).
- Spread magnitude: `S` can be read as `pcr_clkfp.max_req`/`pcr_max.max_req`
  minus the un-credited prediction (~0.999). If < ~10 ps, widen derates per
  `fixture.md` §5 and re-run step 2.

## Step 4 — Strict test, then branch

```
cargo test -p grus-core test_sta_exceptions_match_opensta
```

STRICT tolerances (rel 1%, abs 5 ps) — never loosen.

Expected red pattern (OpenSTA almost certainly credits ordinary checks —
`sta_edge/timing_derate` already proves that on a single-net cone):

- **`pcr_clkfp` red by ~S on `u_ff1/D.max_slack/max_req`**: per-path credit
  confirmed for the ordinary check; our consensus `CRPR_BAD` under-credits.
  Implement the launch-tag machinery (`overview.md` branch A): tag masks on
  `PinTimingState` interned per run, `merge`/`merge_all` updated, seeds
  tagged (`launch_seeds` -> `(net, edge)` bit; `port_seeds`/`latch_seeds` ->
  BAD bit), `credit_for` reads the tag mask of the extreme being composed
  (`crpr_max[pol]` for closing req, `crpr_min[pol]` for hold req, latch
  opening edge uses its `open_pol` tag), and the now-dead
  `crpr_sink_net`/`crpr_driver_net`/`ff_clk`/`latch_q`/`crpr_memo` are
  deleted — no parallel mechanisms.
- **`pcr_max`/`pcr_min` red by ~S on the delay-check rows**: OpenSTA
  credits delay checks. Max side: add `credit_for(check.edge, pol)` to the
  delay-check `req` (`analysis.rs` ~3250), reading the tag from
  `class_states[ci][ep]`. Min side: verify it goes green with **no extra
  term** — `w.req` already carries `-credit` from the class's hold req; if
  `pcr_min` then shows a `-S` residual the inherited credit is wrong and
  the hold-tag read is off.
- **`pcr_max`/`pcr_min` green while `pcr_clkfp` is red**: delay checks are
  NOT credited (branch B). Apply the tag machinery anyway (the ordinary
  check needs it), leave the max-side `req` without a credit term, and fix
  the min side to strip the inherited credit (`w.req + credit + n` —
  otherwise the tag fix would regress `pcr_min`). Update the
  `analysis.rs` comment at ~3239 to state the measured fact.
- **`pcr_off` red**: something credits paths with no shared net — that is a
  bug in the tag/scope logic, not a tolerance question.
- **All five green**: cannot happen given `sta_edge/timing_derate`
  (ordinary credit is already golden-proven) — if it does, the knob made no
  spread; recheck step 3's opposite-direction pair before concluding
  anything.
- **Credit value != `S`**: partial credit or a different common point —
  record the measured rule in `docs/results.md` before implementing
  (lesson 6: reproduce the reference's arithmetic).

Keep the `credit` semantics identical everywhere else: `!propagated_clock`
-> 0, unreached/missing driver -> 0, contested tag mask -> 0, tag net/edge
mismatch -> 0.

## Step 5 — Prove the new golden can fail (lesson 7)

With the test green, break the credit on purpose and rerun:

- Force all seed tags to the BAD bit (or drop the `+credit` in the delay
  `req`): `pcr_clkfp` (and `pcr_max`/`pcr_min` when credited) must go red
  by ~`S`. If `pcr_clkfp` stays green the fixture does not discriminate —
  go back to step 3 margin checks; do not claim the credit is verified.
- `pcr_off` must stay green through any such mutation that keeps the
  shared-net predicate — it is the control that pins the credit to shared
  paths.

Restore and re-verify green.

## Step 6 — Docs, version, commit

1. `docs/results.md`: dated entry — the five scenarios, the spread knob and
   its measured `S`, the `pcr_clk` opposite-direction evidence, whether
   `pcr_clkfp` showed per-path credit on the ordinary check, the
   delay-check verdict (`pcr_max`/`pcr_min`) and the `pcr_off` control
   result, what changed in the implementation (or "no production change"),
   the fail-proof result, and that the full `generate_ref.sh` ran.
2. `docs/path-delays.md`:
   - §1 max/min-delay paragraphs: replace the "CRPR credit the clock check
     adds is not applied (unexercised)" sentence with the measured rule.
   - §2 composition order: add the credit term (or the measured
     `no credit` statement) and the launch-tag description.
   - §4 coverage note: `pcr_*` set, 111 scenarios.
   - §5: resolve the CRPR item, naming the scenarios and the outcome.
3. `docs/timing-exceptions.md`: "CRPR safety" section — if tags landed,
   rewrite "every launch in the cone agrees" to the per-extreme tag rule;
   bump the scenario count to 111 (the line is already stale at 102).
4. Update the `analysis.rs` comment at ~3239-3243 and the test doc comment
   at ~8261 ("106 SDC scenarios" -> 111).
5. `Cargo.toml`: `0.1.91` -> `0.1.92` in the same commit as the functional
   change (`Cargo.lock` follows via build).
6. Verify: `cargo test --workspace` and
   `cargo clippy --workspace --all-targets -- -D warnings`.
7. Commit once green: `add:`-style message listing the scenarios, the knob,
   the measured outcome, and the fix-or-record decision; if a production
   fix was needed, commit it separately from the fixture commit. Agent
   trailer.

## Risk checklist

- **`pcr_clkfp` stays green** (no credit even on the winning shared-net
  path): fixture failed to produce spread — check the `pcr_clk`
  opposite-direction pair first; if absent, derates are not reaching the
  clock cells (selector typo; OpenSTA would have errored on
  `set_timing_derate` — look for an `error` record in the JSON).
- **`u_ff2` path keeps winning `pcr_clkfp`**: the false path did not cover
  it — check `false_path_endpoints`/`u_ff1/D` row presence; `-to
  [get_cells u_ff1]` resolves to `u_ff1/D`, `-from [get_cells u_ff2]` to
  `u_ff2`'s launch keys — both already exercised by `fp_uf0_uf1`.
- **`sel` steals `pcr_clkfp`'s report**: `max_arr` < ~0.7 and not ~0.757 —
  would need a `set_false_path` on `sel` too; not expected (`sel` max
  arrival ~0.55).
- **Tag count > 8** on real designs: mask overflow — seed all launches BAD
  and record the bound in `docs/`; single-clock fixtures are unaffected.
- **Exact-arrival ties between different tags** union the mask -> credit 0
  (conservative): same policy as `par_*` masks; document it.
- **Regression on real-design goldens**: single-clock designs have one FF
  tag, identical to the old consensus — `gcd`/`picorv32`/`aes`/`ibex`/
  `jpeg` STA tests should not move; any diff is a bug in the merge/tag
  propagation, not a semantic improvement — investigate, don't accept.
- **Container/PDK**: Colima running, PDK at `~/.ciel` (`PDK_ROOT`
  override supported).
