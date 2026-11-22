# perf-dmp-hotloop — call-count instrumentation and decision rule

The spec requires the counts to come first and the mechanism to be
chosen by them. The instrumentation is **ephemeral**: a local patch
applied, run, recorded, then reverted before the implementation
commit — counters are never committed. Keep the patch as
`/tmp/dmp-counters.patch` until the results entry is written.

## What to count

All counters are plain `AtomicU64`s (or `thread_local` `Cell<u64>`s
flushed at walk end — atomics are simpler and the run is
instrumented anyway). Dump with `eprintln!` once at the end of the
`sta` / `power` command.

### Per-walk call multiplicity

`driver_response` is invoked from five sites; tag each call with
which walk is active. Cheapest instrumentation: a `thread_local`
`Cell<u8>` `CURRENT_WALK` set at the top of each walk function and
cleared on exit (`propagate_slew` → `sta.prepare.clock.slew`,
`propagate_clock` → `sta.prepare.clock.walk`, `eval_sta` →
`sta.eval.walk`, `slew_eval` → `sta.slew.walk`,
`driver_input_slew` → seeds).

Per walk tag:

- `driver_response` total calls;
- pi-model-path calls (net has `Some(pi)` — the expensive ones)
  vs lumped-path calls;
- `HashSet` of the full memo key
  `(net, pol, delay_table ptr, slew_table ptr, in_slew.to_bits())`
  per walk → **unique count**; exact-repeat share
  `1 − unique/total`, reported separately for the pi path (the one
  whose cost matters) and the lumped path.

### Intra-call `table(c)` repeats

Inside `driver_response`, wrap `table` in a counting closure:

- total `table(c)` evaluations per call;
- `HashSet<c.to_bits()>` per call → repeated-eval count;
- break down by origin if cheap (`is_lumped_bailout`,
  `delay_slope`, `residual`, final `lumped`) — the expected
  repeats are `table(total)` ×3 (`is_lumped_bailout` in
  `solve_dmp`, `residual(hi)` at `hi == total`, and
  `is_lumped_bailout` again in `driver_waveform_slew`), plus
  `table(total ± 5%)` ×2–3 across the `delay_slope` calls in
  `is_lumped_bailout`, `rd_of`, and `driver_waveform_slew`, and
  the final `lumped(ceff)` if `ceff` lands on a previously
  sampled `c`.

### `rc_crossing` bisection share

- `rc_crossing` total;
- `rc_crossing_closed` hits vs `rc_crossing_bisect` entries;
- inside `rc_crossing_bisect`: `brent_checked` accepted vs fell to
  the 40-iteration bisection fallback;
- `fit_ramp_duration` calls and its early-degenerate
  (`rc_slew(lo, tau) >= slew`) hits;
- `pi_waveform_slew` calls, broken into the `rc_slew` fast paths
  (`rd*total <= 1e-15`, `c2`/`rpi` ≈ 0) vs the general two-pole
  Brent;
- `pi_charge`: `pi_charge_discrete` accepted vs numeric fallback;
- `driver_waveform_slew` `is_lumped_bailout` early-returns.

### `eval_2d` site cost

Total `eval_2d` calls (all callers — `driver_response` tables,
`port_drive`, constraint evals). Only meaningful as a share check
against a quick `sample` rerun after memoization; count it now so
the number is already on record.

## Runs

Instrumented release build (`cargo build --release` with the
patch), then:

```sh
BIN=target/release/grus-eda
PDK=$HOME/.ciel/sky130A/libs.ref/sky130_fd_sc_hd
LIB=$PDK/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
TLEF=$PDK/techlef/sky130_fd_sc_hd__nom.tlef
LEF=$PDK/lef/sky130_fd_sc_hd.lef

# primary
$BIN sta   --lib $LIB --tlef $TLEF --lef $LEF \
  --def tests/designs/jpeg/jpeg.placed.def --sdc tests/designs/jpeg/jpeg.sdc \
  --parasitics placement --json /tmp/jpeg.sta.json
$BIN power --lib $LIB --tlef $TLEF --lef $LEF \
  --def tests/designs/jpeg/jpeg.placed.def --sdc tests/designs/jpeg/jpeg.sdc \
  --parasitics placement

# sanity (second design, catches jpeg-only artifacts)
$BIN sta   --lib $LIB --tlef $TLEF --lef $LEF \
  --def tests/designs/ibex/ibex.placed.def --sdc tests/designs/ibex/ibex.sdc \
  --parasitics placement --json /tmp/ibex.sta.json
```

SPEF mode optional — the walks are identical; parasitics only
change the pi models. Record the dumps into `/tmp/dmp-counts.txt`;
the numbers go into `docs/results.md` verbatim and into the design
docs' mechanism section.

## Decision rule

Let `R` = exact-repeat share of **pi-path** `driver_response` calls
summed over all walks of the `sta` run (per-walk breakdown kept for
the results entry).

| measurement | action |
|---|---|
| `R` ≥ ~25% | **M1** run-scoped memo is the mechanism — implement per [`memoization.md`](memoization.md). Expected ceiling ≈ `R` × (DMP share of the walk). |
| `R` < 25% but intra-call `table` repeat share ≥ ~15% of `eval_2d` calls, or `eval_2d` is a visible sample share | **M2** per-call `table` memo (always bit-identical); M1 still allowed if its absolute hit count × ~200 µs/solve clears noise — decide and record. |
| `rc_crossing` brent share high (≳ 50%) | **Residual, not a fix target**: widening closed-form coverage or tightening the Brent bracket changes the returned f64 → `cmp`-dirty `--json`, violating the equivalence bar. Name the share as the residual in `docs/results.md`. |
| `eval_2d` share ≥ ~20% of walk samples after M1 | **M4** arithmetic-preserving `eval_2d` hardening (`nldm.rs`). No reciprocal-multiply or axis re-scaling — f32 bits must not move. |
| everything small | Record the negative result; the ticket shrinks to the honest residual report — do not ship a mechanism the counts don't support. |

After the counts land, update [`overview.md`](overview.md) and
[`memoization.md`](memoization.md) to name the chosen mechanism(s)
and strike the others — that edit rides in the implementation
commit (acceptance: "implemented mechanism chosen by the counts,
named in design docs").
