# Goal

Evaluate the remaining deferred check arcs — `non_seq_setup_*` /
`non_seq_hold_*`, `three_state_*`, `preserve` (+ `Clear`/`Preset`
as they currently sit in the same counted-defer bucket). Today
they are named and counted via `recognized_check_arcs` but never
evaluated. Public designs exercise ~zero instances, so a
hand-written fixture is required (`sta_edge/async_arcs` is the
closest precedent).

## Work

1. **Census the lib**: which `sky130_fd_sc_hd` cells actually
   carry each arc type (`non_seq_*` on which pin pairs,
   `three_state_*` enable pins, `preserve` blocks) — record the
   families so the fixture uses cells that exist.
2. **Probe OpenSTA** (committed script + artifacts):
   - are `non_seq_*`/`three_state_*`/`preserve` arcs evaluated
     as checks in OpenSTA 3.0.16 at all — do they produce
     endpoint rows, and under which report command
     (`report_checks`, pulse-width surface, nothing)?
   - if evaluated: the anchor/parity semantics per type (which
     edge pairs, constraint table source).
   - `Clear`/`Preset`: already probed non-propagating
     (async-arc-probe) — confirm they stay count-only rather
     than silently gaining evaluation.
3. **Implement** whatever is measured as evaluated (likely
   small — either real check rows or a pinned 'OpenSTA also
   ignores these' record); keep anything OpenSTA ignores in
   the counted bucket with updated docs.
4. Golden coverage on the new fixture at STRICT for whatever
   evaluates; results.md + roadmap updates; version bump.

## Files

- `tests/designs/sta_edge/check_arcs/` (new fixture: def +
  sdc + probe script + committed artifacts)
- `crates/grus-core/src/sta/analysis.rs` (if eval is measured)
- `crates/grus-core/src/readers/liberty.rs` /
  `crates/grus-core/src/lib_db.rs` (if arc data is needed)
- `docs/results.md`, `docs/roadmap.md`
- `Cargo.toml` / `Cargo.lock`
