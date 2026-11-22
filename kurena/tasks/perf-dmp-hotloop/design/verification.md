# perf-dmp-hotloop — verification plan

## Equivalence gates (acceptance)

1. **Goldens byte-identical**: `cargo test --workspace` and
   `cargo test --release -p grus-core sta` green; `git status
   --porcelain` clean on `tests/` — no reference file moves.
2. **`--json` `cmp`-clean pre/post, jpeg + ibex, placement RC**:
   keep the pre-change release binary at `/tmp/grus-eda.before`
   (steps §0); run on both binaries:

   ```sh
   for d in jpeg ibex; do
     $BIN sta --lib $LIB --tlef $TLEF --lef $LEF \
       --def tests/designs/$d/$d.placed.def --sdc tests/designs/$d/$d.sdc \
       --parasitics placement --json /tmp/$d.sta.after.json
   done
   cmp /tmp/jpeg.sta.before.json /tmp/jpeg.sta.after.json
   cmp /tmp/ibex.sta.before.json /tmp/ibex.sta.after.json
   ```

   (`.before.json` files are produced in steps §0 with the same
   command on `/tmp/grus-eda.before`.)
3. **`RAYON_NUM_THREADS` 1/4/10 `cmp`-clean** on jpeg:
   `sta.placement` `--json` at each setting on the after binary;
   `cmp` the three outputs pairwise.
4. **Fail-proof recorded** (AGENTS §7.7): break the memo on
   purpose, watch a gate go red, restore. Candidate mutations —
   run at least one, record which:
   - drop `net` from `DrvKey` (collapse across nets sharing
     tables+slew) → strict STA tests and/or the `cmp` run red;
   - key `in_slew` on a truncated bit pattern (e.g.
     `in_slew.to_bits() >> 8`) — a quantized cache — red;
   - store `r.slew` where `wave_slew` is expected — red.
5. **Memo actually exercised**: the committed test below proves
   the fast path runs and is exact.

## Committed test

Under `#[cfg(test)]` only (production code unchanged):

- TLS stats pair `DRV_MEMO_HITS` / `DRV_MEMO_MISSES` —
  `Cell<u64>`s bumped inside `driver_response`'s lookup;
  `#[cfg(test)]`-gated counters are allowed in the committed tree
  (the banned thing is the *measurement* instrumentation, which
  is per-walk tagging and `HashSet`s — none of that ships).
- One unit test: a `Context` fixture calling `driver_response`
  twice with identical args asserts `hits == 1`, the second
  result is bitwise `to_bits`-equal to the first, and a third
  call differing only in `in_slew` misses. Locks both halves of
  the contract: the fast path engages and it returns exact bits.

## Existing coverage that carries weight

- `test_sta_pin_slews_parity_{committed_designs,propagated,
  microbench,exceptions,latch_borrow_fallback}` — the `slew_eval`
  path (and its `eval_state_full` fallback) must keep producing
  the eval-state slews bit-for-bit; these are the broad proof the
  memo didn't perturb slew propagation.
- `golden_regression_values_are_reproduced` in `dmp.rs` — the
  solver itself is untouched; stays green.
- All `sta_edge`/`sta_exceptions`/`sta_clocks` golden tests —
  every arc-leg result flows through `driver_response`.

## What is deliberately not re-verified

- No new OpenSTA comparison runs: nothing about timing values
  changes (bit-identical by construction + `cmp` proof), so the
  golden files and tolerances stand. State this in
  `docs/results.md`.
- Multi-corner (`--lib-hold`) path: shares `driver_response`
  through a second `prepare_sta` — epoch isolation covers it; no
  dedicated run beyond the workspace tests.
