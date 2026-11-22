# Goal

Close `docs/path-delays.md` section 5 item 3: whether OpenSTA applies
a CRPR (common-path pessimism removal) credit to `set_max_delay`/
`set_min_delay` checks is unmeasured, and our delay-check composition
does not reproduce it (`analysis.rs` comment: "the CRPR credit the
clock check adds is not reproduced here"). The ordinary clock check
already applies the credit (`credit_for`, shared-net driver
early/late spread); the delay check does not. All existing scenarios
have zero clock-net late/early spread, so the gap is invisible in the
current golden.

## Work

1. **Create a nonzero-spread fixture scenario first.** The spread
   must come from the clock network, not the data path. Candidate
   knobs, in order: `set_timing_derate` with `-early`/`-late`
   factors (check which selector — `-clock`, net/cell delay — makes
   OpenSTA's clock-check slack move on the shared tree), or a
   clock-path latency/transition asymmetry. Measure against the
   *ordinary clock check* first: find the smallest SDC addition
   where OpenSTA's `u_ff1/D` slack on the `base_prop` shape changes
   exactly the way a CRPR credit predicts (the implementation's
   `credit_for` exists, so if derate reaches the clock net on both
   sides the ordinary check already discriminates).
   Add `crpr_*`/`pcr_*` scenarios to `tests/designs/sta_exceptions/`
   and `SCENARIOS` in `generate_ref.sh`:

   - `pcr_clk` — propagated clock + the spread knob, no delay rule:
     baseline proving the spread exists and the clock check sees
     the credit.
   - `pcr_max` — same + `set_max_delay N -from ... -to [get_cells
     u_ff1]` on a shared-tree launch (e.g. `u_ff0`, which shares
     `c1` with the capture `u_ff1`): discriminates whether OpenSTA
     credits the delay check.
   - `pcr_min` — same shape for `set_min_delay`.
   - Optionally `pcr_off` — same spread, delay rule on the
     non-shared launch (`u_ff2` on `c2`): credit must NOT appear
     (control).

   Regenerate `exceptions.reference.json` by running the committed
   `generate_ref.sh` inside `ghcr.io/librelane/librelane:3.0.16`
   (docker is available — local colima may need `colima start`
   first). Every value comes from that run.

2. **Reconcile.** If OpenSTA credits the delay check and our slack
   diverges, implement the credit in the delay-check composition
   (`analysis.rs`: reuse `crpr_sink_net`/`credit_for` semantics —
   shared-net driver spread — applied per the measured rule). If
   OpenSTA does not credit it, our zero credit already matches;
   record the measurement in `docs/results.md` and close the item
   without production changes.

3. **Docs**: `docs/path-delays.md` section 5 item 3 resolved with
   the measured outcome; `docs/results.md` dated entry (scenarios,
   spread source, whether credit appeared, fix-or-record outcome).
   Version bump (patch) in the same commit.

## Files

- `tests/designs/sta_exceptions/*.sdc`
- `tests/designs/sta_exceptions/generate_ref.sh`
- `tests/designs/sta_exceptions/exceptions.reference.json`
- `crates/grus-core/src/sta/analysis.rs`
- `docs/path-delays.md`
- `docs/results.md`
- `Cargo.toml`
- `Cargo.lock`
