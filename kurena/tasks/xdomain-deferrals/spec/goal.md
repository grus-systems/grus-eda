# Goal

Resolve the four parked `CrossDomain` deferrals recorded in
`docs/multi-clock.md` §2 — each is a defensive named skip; the
question per item is whether OpenSTA actually produces a result
in that corner (then implement) or also rejects (then keep the
deferral, pinned).

## The four items (from `docs/multi-clock.md` §2)

1. **Undeclared-launch zero mask**: a launch reaches a capture
   domain's endpoint with a domain mask bit not declared —
   what does OpenSTA do (report with a default grid? drop?
   error)?
2. **Latch-D foreign-launch borrow pairing**: a delay check or
   ordinary check on a latch `D` whose launch lives in a
   different domain than the latch's clock — OpenSTA's borrow
   window (if any) in that case.
3. **Unpairable hold tying parity**: cross-domain hold where
   no coincident-edge candidate exists in the LCM window —
   OpenSTA's fallback (skip? nearest? error?).
4. **`grid_lcm` non-convergence**: near-incommensurate periods
   where the grid search exceeds the bound — OpenSTA's
   horizon/failure mode.

## Work

- One container probe per item on `tests/designs/sta_clocks/`
  (extend with new `t2_*` SDCs as needed; commit artifacts).
- Implement whatever is measured as evaluated; keep genuine
  OpenSTA-also-skips cases as named deferrals, pinned.
- Update `docs/multi-clock.md` §2 (resolve or confirm each
  item), `docs/results.md` entry, version bump.

## Files

- `tests/designs/sta_clocks/` (new sdc + probe artifacts)
- `crates/grus-core/src/sta/analysis.rs` (if any item
  evaluates)
- `docs/multi-clock.md`, `docs/results.md`
- `Cargo.toml` / `Cargo.lock`
