# async-latch-delay — Overview

## Goal

Close the measurable half of `docs/path-delays.md` §5 item 1: does OpenSTA
honour `set_max_delay` / `set_min_delay` on the two endpoint kinds our delay
coverage counts in `unsupported_path_delay_endpoints` —

- **async-check pins**: recovery/removal checks on `RESET_B`-type pins
  (`ExceptionEndpointKind::Check { latch: false }` carrying
  `recovery_arc`/`removal_arc` instead of `setup_arc`/`hold_arc`), and
- **latch-D pins** (`Check { latch: true }`, borrow semantics).

## Measured findings (verified, implemented)

The probe + golden regen established: **OpenSTA applies delay checks on
both kinds**, on max and min sides. The measured rules are richer than
the "widen the predicate" first guess:

- **async-check**: the FF formula verbatim — `late_arc` is the recovery arc,
  `early_arc` the removal arc; `req = capture_min[check.edge] − unc_setup −
  t_recovery + N (+crpr)` on the max side, `removal_req_base + N` on the min
  side.
- **latch-D**: delay-edge anchored with borrow semantics —
  `req_max = clamp(arr_dly, base, base + pulse − t_setup)` with
  `base = N + lat0 − setup_uncertainty`; `req_min = N + lat0 + t_hold +
  hold_uncertainty`. Arrivals re-base: a path launched through a latch's
  open edge reports its launch at the delay edge (new `dly` extremes); flop
  and port launches keep their clock edge. A `set_max_delay` covering a
  latch's D pin silences its `D -> Q` transparent borrow arc for every
  downstream path in the signature class; `-min` coverage does not.

These numbers are now **verified**: the regenerated `generate_ref.sh`
goldens in `e530643` agreed with `probe.md`'s record, and the
`docs/results.md` entry (v0.1.95) records the comparison plus the
fail-proof runs.

## Current state

Implemented and committed as `e530643` (fixtures, goldens, `analysis.rs`,
docs, `0.1.95` bump). The human review rejected on **one** point:

- The docstring on `test_sta_edge_latch_borrow_delay_max`
  (`analysis.rs:7754`) says the borrow-window bound is `pulse + t_setup`;
  the implemented — and golden-verified — formula is `pulse − t_setup`
  (see `api.md` §4 sign correction: OpenSTA's `report_checks` prints the
  setup term negated, "library setup time -0.18", which is what keeps
  reintroducing this error). Fix only that docstring; keep everything
  else. Optional low nit: `rustfmt` the hunks `e530643` touched — `cargo
  fmt --check` flags pre-existing spots in `analysis.rs` too; reformat
  only the new code, not the untouched file.
- Steps: `steps.md`. Nothing else moves — no fixture, golden, docs, or
  version changes.

## Plan shape (executed — kept for the record)

1. Recreated the 11 scenario SDCs (`fixture.md` §1) and extended both
   `generate_ref.sh` scripts (§2).
2. Ran the committed scripts in the container — this **is** the
   measurement. Regenerated sections agreed with `probe.md`'s recorded
   numbers (results in `docs/results.md`).
3. Implemented per `api.md` for the (kind, side) pairs the goldens
   confirmed as applied — both kinds, both sides.
4. STRICT golden tests, `docs/results.md` entry, `docs/path-delays.md`
   §1/§3/§5 updates, patch version bump — done.

The literal two-clock-domain half of item 1 stays open (multi-clock SDC
unsupported); clock-gate enable endpoints stay deferred and counted — out of
scope here.

## Verdict consequences

- Both kinds applied (recorded expectation) → `delay_supported` widened to
  `Check { .. }` with check arcs present; `unsupported_path_delay_endpoints`
  keeps only `ClockGate`. Golden scenarios at `Tol::STRICT` pin every
  measured rule.
- If golden regen contradicts the record for some (kind, side) — endpoint
  byte-identical to baseline (**ignored**) or `STA-####` raised
  (**errored**) — follow `probe.md` §"If regen contradicts": keep that pair
  counted, emit the evidence verbatim, narrow `delay_supported`. Never
  extrapolate one side's verdict to the other.

## Files

- `crates/grus-core/src/sta/analysis.rs` (remaining delta: docstring at
  ~7754 + optional fmt on the hunks `e530643` touched)
