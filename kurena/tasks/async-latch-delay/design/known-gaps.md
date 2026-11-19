# async-latch-delay — Known gaps note (review-2 delta)

The second review flagged three delay-check semantics that the code
implements and can reach but no committed golden pins down. Per
AGENTS.md §7.4 (claim coverage only for what an input exercises) the
record must name them. Docs-only: add the note to
`docs/path-delays.md` §5 (the standing open-questions list for this
feature) and mirror it as short bullets in the v0.1.95 `results.md`
entry's `### Not run` (the per-run coverage ledger). No code changes —
each gap is the best-guess semantic until a fixture discriminates it.

## The three gaps (code evidence at HEAD `a356baf`)

### G1 — borrow cap applied to launch-edge-relative `dly`

`analysis.rs` ~2857-2871: a `D -> Q` borrow arc clamps both extremes
through the same `cap()` — `k = floor((arr - open) / period)`,
`arr.min(open + k*period + pulse - unc - t_setup)` — where `open` is
the borrowing latch's absolute enable arrival. `arrival_max` is
absolute time; `dly_max` is absolute for port/flop launches but
launch-edge-relative for a through-latch launch (opening edge
stripped, `eval_launch` ~2542). Feeding the relative `dly` into the
absolute window picks a different `k` and bound.

Exact on every goldened case: covered launches are port/flop seeds
where `dly == arr`, so `cap(dly) == cap(arr)`. Unverified on
through-latch launches: `cap(dly)` executes whenever an enabled borrow
arc carries a latch-open launch (e.g. `delay_max`'s `u_ln -> u_f0` —
borrow still enabled), but no goldened covered endpoint reports a
borrow-capped `dly`, and the fixture has no latch-Q -> latch-D chain
(every `q*` net feeds an FF `D` pin).

### G2 — delay-check CRPR tag follows the `arr` winner, not the `dly` winner

`PinTimingState::merge` (~158-178): `crpr_max`/`crpr_min` are
replaced/unioned by the **arrival** winner's launch tag, while
`dly_max`/`dly_min` merge independently — the `dly` extreme can belong
to a different launch. The FF-D/async delay compose (~3676-3690)
credits `s.crpr_max[pol]` — the arrival winner's tag — while reporting
`arr = w.dly`. If the `dly` winner is a through-latch launch and the
`arr` winner a flop/port launch, the credited shared-point tag belongs
to a launch other than the reported one.

Divergence requires both a through-latch launch reaching a covered
endpoint and a nonzero credit (`set_propagated_clock` +
`set_timing_derate`): `pcr_*` has no latches, `latch_borrow` carries no
derate, and `delay_allregs` (the through-latch `dly` scenario) runs on
the ideal clock — so the divergent case is ungoldened.

### G3 — min-side golden coverage is per cell-variant

`set_min_delay` is golden-verified only on `dlxtp` latch-D
(`latch_borrow/delay_min.sdc` -> `u_lp/D`) and `dfrtp` `RESET_B`
(`async_reset/delay_min.sdc` -> `u_ff0/RESET_B`). The same code paths
serve, unverified:

- `dlxtn` latch-D — the latch-D min branch (~3724-3747) anchors
  `req = N + arrival_max[edge0_pol] + t_hold + unc_hold`; on `u_ln`/
  `u_ln2` the first-edge polarity and the negative latch's hold arc
  differ from the goldened `dlxtp` row. (`delay_ln` is max-side.)
- latch-cell `RESET_B` removal — `u_rbn`/`u_rbp`/`u_rtn`/`u_rtp` take
  the shared `req_hold_base + N` path, but `delay_allregs` (the only
  scenario covering latch `RESET_B`) is `set_max_delay` only, so the
  latch-side removal variant and its `edge0_pol` anchor are unprobed
  on the min side.

## Proposed wording — `docs/path-delays.md` §5

Insert after the "Resolved by the async/latch delay probe set
(2026-11-19):" block, before "Still open:" (match the file's ~66-col
wrap):

```
Known gaps in that resolution (reachable but ungoldened — review
2026-11-19):

- the `D -> Q` borrow cap clamps `dly_max` through the same
  absolute-time window as `arrival_max` (`open + k*period + pulse -
  unc - t_setup`). Exact wherever `dly == arr` — every goldened case,
  since covered launches are port/flop seeds — but a through-latch
  launch feeds a launch-edge-relative `dly` into the absolute
  window; whether OpenSTA re-bases before clamping is unverified.
  The fixture has no latch-Q -> latch-D chain, so no covered
  endpoint reports a borrow-capped `dly`.
- the delay check's CRPR credit reads `crpr_max`/`crpr_min` — the
  *arrival* winner's launch tag — while the reported `arr` is the
  independently merged `dly` winner. The winners diverge only via a
  through-latch launch and the credit is nonzero only under
  `set_propagated_clock` + `set_timing_derate`; `pcr_*` has no
  latches and `latch_borrow` no derate, so the divergent case is
  ungoldened.
- `set_min_delay` is golden-verified only on `dlxtp` latch-D
  (`u_lp/D`) and `dfrtp` `RESET_B` (`u_ff0/RESET_B`); the same code
  serves `dlxtn` latch-D and latch-cell `RESET_B` removal
  (`u_rbn`/`u_rbp`/`u_rt*`, covered max-side only by
  `delay_allregs`) unverified.
```

## Proposed wording — `docs/results.md` `### Not run`

Three bullets appended to the v0.1.95 entry's list (after the existing
edge0/two-clock/clock-gate items), one line each:

```
- Borrow-capped `dly` on through-latch launches (relative value
  against the absolute clamp window; no latch-Q -> latch-D path in
  the fixture — docs/path-delays.md §5 known gaps).
- Delay-check CRPR tag under divergent `arr`/`dly` winners (tag
  follows the arrival winner; needs through-latch launch under
  propagated clock + derate — ungoldened).
- `set_min_delay` on `dlxtn` latch-D and latch-cell `RESET_B`
  (golden-verified on `dlxtp`/`dfrtp` only).
```
