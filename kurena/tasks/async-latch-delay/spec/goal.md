# Goal

Close the measurable half of `docs/path-delays.md` section 5 item 1:
whether OpenSTA honours `set_max_delay`/`set_min_delay` on the two
endpoint kinds our delay coverage counts in
`unsupported_path_delay_endpoints` — async-check pins (recovery/
removal on RESET_B-type pins) and latch-D pins (Check{latch:true},
borrow semantics). Both fixtures already exist with measured
golden (`tests/designs/sta_edge/async_reset`, `.../latch_borrow`);
the missing piece is delay-check scenarios on them. The literal
two-clock-domain fixture stays open (multi-clock SDC unsupported).

## Work

1. **Measure first.** Inside the LibreLane container, run OpenSTA
   on the existing `async_reset` and `latch_borrow` DEF+Liberty
   with `set_max_delay`/`set_min_delay` `-to` the async pin and
   `-to` the latch-D pin (and the `-from` variants that make
   sense). Record for each: does OpenSTA apply the check (req
   moves), ignore it, or error? If it applies, capture the exact
   req composition (recovery/removal-anchored? opening-edge
   anchored? N-offset?). Report the verdict per endpoint kind in
   `docs/results.md` before implementing anything.

2. **Reconcile.** If OpenSTA applies a delay check on either
   endpoint kind, implement coverage for that kind in the
   exception model (`ep_kinds` already tags them) with the
   measured req rule and add golden scenarios at STRICT
   tolerance. If OpenSTA ignores/rejects it, keep the counted
   skip and record the measured evidence — that closes the item
   with data either way.

3. **Docs**: `docs/path-delays.md` §5 item 1 split: resolved
   halves (async/latch probes) struck with the measured outcome,
   two-clock-domain stays open with the blocker restated;
   `docs/results.md` dated entry; `docs/timing-exceptions.md`
   only if the skip surface changes. Version bump (patch).

## Files

- `tests/designs/sta_edge/async_reset/**`
- `tests/designs/sta_edge/latch_borrow/**`
- `tests/designs/sta_exceptions/*.sdc` (only if the probe needs
  the exception fixture instead)
- `crates/grus-core/src/sta/analysis.rs`
- `docs/path-delays.md`
- `docs/results.md`
- `docs/timing-exceptions.md`
- `Cargo.toml`
- `Cargo.lock`
