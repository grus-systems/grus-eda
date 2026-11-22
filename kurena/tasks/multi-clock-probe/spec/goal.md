# Goal

Measure-first ticket for multi-clock SDC support — the blocker
for `docs/path-delays.md` §5's last open item (literal
two-clock-domain `set_min_delay` fixture) and a real gap on any
design with more than one clock. Today `create_clock` with a
second source port silently overwrites the first
(`sdc.rs:863`), and the eval model is single-clock.

This ticket is **probe + scope only**: measure where we break
and what OpenSTA's rules are, then write the findings down. No
implementation unless the probe reveals a gap small enough to
close inside this ticket's size budget — otherwise the output
is a sequenced spec for the real implementation tickets.

## Work

1. **Where we break.** Feed the existing `sta_exceptions`-style
   fixture a two-clock SDC (a second `create_clock` on a second
   port driving a second FF group). Record exactly what
   happens: parse bail, silent overwrite (wrong results),
   panic, or partial eval. Enumerate every place the model
   assumes one clock: `Sdc::clock_port`, `create_clock`
   overwrite, `clock_parity`, `edge_parity`, launch/capture
   pairing, `set_clock_uncertainty`/`set_clock_latency`
   "takes one clock" errors, virtual-clock I/O anchoring.

2. **OpenSTA's rules** (measured inside the LibreLane
   container, recorded in `docs/results.md`):
   - two `create_clock`s on separate ports: how launch/capture
     pairs form (same-domain pairs only? cross-domain paths —
     which get timed and how is the capture edge chosen for
     different periods?);
   - `set_clock_uncertainty`/`set_clock_latency` per-clock
     semantics;
   - does `set_max_delay -to` a second-domain endpoint get a
     capture anchor from that domain's clock (the measured
     capture-anchor rule per domain)? That is the exact
     two-clock `set_min_delay` question from §5;
   - period ratios that are non-integer (e.g. 2.0 vs 1.4):
     capture-edge search horizon.

3. **Deliverable**: `docs/results.md` dated entry (break
   points + measured rules) and, unless the gap turned out
   small, `docs/` or the task's design doc gets the sequenced
   ticket plan (what order: domain tagging → launch/capture
   pairing → uncertainty/latency per clock → the §5 fixture).
   Version bump is NOT needed (no functional change); if any
   doc/code does change, bump patch.

## Files

- `crates/grus-core/src/sta/sdc.rs` (probe edits only if needed)
- `crates/grus-core/src/sta/analysis.rs` (ditto)
- `tests/designs/sta_exceptions/*.sdc` or a new fixture dir
- `docs/results.md`
- `docs/path-delays.md`
- `docs/roadmap.md`
- `kurena/tasks/multi-clock-probe/` (design notes land here)
