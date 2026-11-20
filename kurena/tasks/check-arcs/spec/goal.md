# Goal

Promote `min_pulse_width` from a counted `recognized_check_arcs`
skip to an evaluated check. `docs/roadmap.md` N3 deferred exactly
this: the arcs are named and counted but never produce endpoints.
`non_seq_*`, `three_state_*`, and `preserve` stay counted — census
shows no instantiated instances in our designs; if the measurement
below finds any, record it and keep them deferred.

## Work

1. **Measure the OpenSTA surface first.** Determine which OpenSTA
   command reports pulse-width checks (`find_timing_paths
   -path_delay min_pulse_width` or `report_checks` variant) and
   what fields it returns per endpoint (required width vs measured
   high/low pulse, slack sign). Record the finding in
   `docs/results.md`. Then build a small fixture — a new dir under
   `tests/designs/` (e.g. `sta_edge/min_pulse_width/` or alongside
   it) is likely better than growing `sta_exceptions`, since this
   is a new endpoint *kind*, not an SDC exception. Reuse the
   existing clkbuf-heavy clock-tree shape if convenient. Cover at
   least:
   - ideal clock: pulse widths from the waveform/latency seeds;
   - propagated clock: real skewed high/low widths at clock pins
     (our `u_clkb*` chain gives measurable asymmetry);
   - at least one genuine violation (tighten via duty cycle or a
     long asymmetric buffer chain, or pick the arc whose
     constraint exceeds the propagated width);
   - falling-edge-check FF (`dfrtn`-shape) if one exists in the
     lib, for the low-pulse direction.

   Emit the golden by extending `generate_ref.sh`-style Tcl inside
   the LibreLane container — every value from that run.

2. **Implement.** Add a `MinPulseWidth` endpoint kind: for every
   clock pin carrying a `MinPulseWidth` arc, evaluate high pulse
   (fall arrival − rise arrival vs the arc's high constraint) and
   low pulse (next-edge rise arrival − fall arrival vs low
   constraint) per the measured OpenSTA rule. Surface them in the
   endpoint list/report and slack accounting per however OpenSTA
   reports them (match the measured surface exactly). Keep the
   `recognized_check_arcs` counting for the still-deferred types
   (remove `MinPulseWidth` from that count once evaluated, or keep
   counting per the existing convention — match whatever
   `collect_endpoints` does for evaluated-vs-deferred arcs).

3. **Docs**: `docs/results.md` dated entry (what OpenSTA command
   surfaced the checks, fields, violation observed, our values);
   `docs/roadmap.md` N3 deferred item updated;
   `docs/path-delays.md`/`timing-exceptions.md` only if touched.
   Version bump (patch) in the same commit.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/report.rs` (or wherever endpoints emit)
- `crates/grus-core/src/lib_db.rs`
- `tests/designs/**` (new fixture dir + golden + generator)
- `docs/results.md`
- `docs/roadmap.md`
- `docs/timing-exceptions.md`
- `Cargo.toml`
- `Cargo.lock`
