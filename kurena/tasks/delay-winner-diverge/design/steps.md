# Implementation & verification steps

## Step 1 — Stage A: mux-pin swap + regenerate

1. Apply the stage-A edit in `fixture.md` to
   `tests/designs/sta_exceptions/exceptions.def` (swap `u_mux A0`/`A1` on
   nets `d0`/`d2` only).
2. Regenerate the golden from the repo root, exactly as committed:

   ```
   bash tests/designs/sta_exceptions/generate_ref.sh
   ```

   (re-invokes itself inside `ghcr.io/librelane/librelane:3.0.16`; start
   Colima first if needed per `AGENTS.md` §2). Every value must come
   from that run. `git diff` the file before proceeding.
3. Inspect the regenerated JSON before running the test — extract the
   `u_ff1/D` rows of `base_prop`, `pm2_prop`, `pm2_both`, `m2_max0`,
   `m2_max2`, `m2_both` (a short `python3`/`jq` one-liner, not by eye):

   - `m2_both.max_arr == m2_max0.max_arr` and `> m2_max2.max_arr`
     → pure-delay winner is `u_ff0` (expected).
   - `pm2_both.max_arr == pm2_prop.max_arr` → merged domain reports
     `u_ff0`'s candidate (expected either way).
   - `base_prop.max_arr > pm2_both.max_arr` → arrival winner ≠ delay
     winner: **split already materialised** (stage A sufficed; the
     overview's prediction was wrong). Skip step 2.
   - Otherwise (`base_prop ≈ pm2_both`, `u_ff0` winning both): record
     the measured `g = m2_max0.arr - m2_max2.arr` and proceed to step 2.

## Step 2 — Stage B: clock-buffer insertion + regenerate

1. Apply the stage-B edit in `fixture.md` (`u_clkb3` between `u_clkb2`
   and `u_ff2/CLK`; `COMPONENTS 7`, `NETS 10`, new net `c3`).
2. Regenerate via the same committed command.
3. Verify the split from the JSON:

   - `m2_both.u_ff1/D.max_arr == m2_max0`'s (delay winner `u_ff0`,
     ideal clock).
   - `pm2_both.u_ff1/D.max_arr == pm2_prop`'s (merged delay domain
     still reports `u_ff0`'s `launch_c1 + d_A1`).
   - `base_prop.u_ff1/D.max_arr > pm2_both`'s by > ~10 ps → the
     ordinary max arrival is won by a different source (`u_ff2`'s
     delayed launch overcame its smaller `d_A0`). Sanity-check the
     winner is `u_ff2` not `sel`: `base_prop`'s arr should exceed
     `sel`'s reach (~≤0.72 ns); if it doesn't, `sel` may be winning —
     enlarge `T_buf` per `fixture.md` and regen.
   - `pm2_min2` still shows the min-side delay check on `u_ff1/D`.

   If margin is thin (< ~2x the 5 ps tolerance), enlarge `T_buf` and
   regen — the split must be unambiguous to be a discriminator.

## Step 3 — Strict test, then branch on the result

```
cargo test -p grus-core test_sta_exceptions_match_opensta
```

STRICT tolerances (rel 1 %, abs 5 ps) on all 72 scenarios — no
tolerance edits, ever.

- **Green**: the implemented pairing (hypothesis (b), `overview.md`) is
  confirmed by golden data. Continue to step 4.
- **Red only on the divergent scenarios** (`pm2_both`/`m2_both` and
  possibly `pmd_multi`/`md_nofrom`): the pairing is wrong. Likely
  hypothesis (a) — OpenSTA reports the covered-source arrival max.
  Then the fix is in composition, not the test: in `run_sta_partitioned`'s
  delay-check block (`analysis.rs` ~3300-3348) report the covered-class
  `arrival_max` instead of `delay_launch_max + delay_max` — which also
  retires the `delay_max`/`delay_launch_max` state if (a) holds (delete
  it, no half-shims). Other suspects if the red pattern differs:
  `PinTimingState::merge` (`analysis.rs` ~168-170), `merge_all`
  (~187-189), the `delay_launch` propagation argument order
  (~2647-2663). If the residual looks like a fixture/model limit rather
  than a pairing bug, stop and record it in `docs/results.md` — do not
  loosen tolerances or paper over it.
- **Red broadly**: recheck the DEF diff (counts, net names, `u_ff2/CLK`
  on `c3`); a malformed net degrades many scenarios at once.

## Step 4 — Prove the new golden can fail (AGENTS.md lesson 7)

With the test green, break the pairing on purpose — e.g. compose
`arr`/`slack` from `arrival_max`'s launch or a wrong `delay_launch` —
and rerun `test_sta_exceptions_match_opensta`. `pm2_both` (and any
other divergent scenario) must go red. Restore the code. If it stays
green, the fixture still doesn't discriminate — go back to step 2
margin checks rather than claiming victory.

## Step 5 — Stale literal, docs, version, commit

1. `analysis.rs` `test_sta_exception_diagnostics` comment (~line 8562):
   `4.562147` → the value implied by the regenerated golden (the
   unshifted winner req + one period; currently `2.550057 + 2.0 =
   4.550057` — confirm the winner req in the new file, it is
   data-net-independent).
2. `docs/results.md`: new dated entry — what changed (stage A swap,
   stage B `u_clkb3` if used, with the measured `g`/`s`), the regen
   command actually run, the winner-pairing outcome (delay winner
   `u_ff0` vs arrival winner `u_ff2`, or whatever the golden showed),
   strict-test result, fail-proof result. State explicitly which steps
   of the script were run (all of them, ideally).
3. `docs/path-delays.md`:
   - Section 4 honest-coverage note (~lines 328-334): the divergence is
     now exercised — name `pm2_both`/`pm2_prop`/`base_prop`/`m2_*` and
     the measured gap.
   - Section 5 item 3: mark the diverging-winner half resolved (with
     the fixture description); keep the CRPR-credit half open, plus the
     propagated port-launch anchor (item 4) and item 1.
   - If `pm2_both`'s req-anchor numbers are cited elsewhere in the doc
     (§1 mentions `1.013333`/`1.0048`), check whether they moved and
     sync.
4. `docs/timing-exceptions.md` ~line 145: the fixture description
   ("split `c1`/`c2` clock buffers") — if stage B landed, update to
   mention the extra buffer stage on `u_ff2`'s clock.
5. `Cargo.toml`: `[workspace.package] version` 0.1.87 → 0.1.88, same
   commit as the fixture change.
6. Verify: `cargo test --workspace` and
   `cargo clippy --workspace --all-targets -- -D warnings`.
7. Commit once green (`add:`/`update:`-style message; list the DEF
   change, regenerated golden, measured winners, doc updates; record
   the goal-mechanics correction in the body; agent trailer). If
   step 3 needed a production-code fix, commit it separately from the
   fixture commit.

## Risk checklist

- **Goal's prediction is wrong (expected)**: stage A alone leaves
  `u_ff0` winning both domains — that is why stage B exists; do not
  "fix" it by hand-editing numbers.
- **`sel` steals the arrival report** in `base_prop`: enlarge `T_buf`.
- **Margin too thin to discriminate** (`launch_ff2 - launch_ff0 - g`
  within ~10 ps): bigger `T_buf`, regen.
- **Docker/PDK**: Colima must be running; PDK from `~/.ciel`.
- **OpenSTA rejects the DEF**: check `COMPONENTS`/`NETS` counts and
  that `u_clkb3`'s output net `c3` exists — a dangling buffer input on
  `c2` is legal, a missing net is not.
- **All 72 regenerated**: the swap + clock change shifts most rows;
  any scenario whose numbers did *not* move is suspicious — say what
  moved and what didn't in `docs/results.md`.
