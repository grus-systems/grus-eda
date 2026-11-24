# drt-progress: route_nets progress display

External review: `route_nets` emits nothing until it finishes — on
picorv32 that's ~2h of silence (measured), on bigger designs more.
A periodic progress line (nets completed, unrouted so far, elapsed)
is cheap and answers "is it stuck?" without attaching a debugger.

## Scope

1. During `grus-eda drt` routing, print a progress line to stderr
   every N nets completed or every T seconds (whichever first;
   pick a sane cadence — e.g. every 500 nets or 60 s — and record
   the choice). Line must include nets done / total, unrouted so
   far, elapsed wall time.
2. Keep stdout census output unchanged (the `== drt emit:` block is
   a consumed contract — progress goes to stderr only).
3. No progress spam in tests/library use: the progress sink is a
   CLI-level concern — library callers don't get stderr writes.
   Prefer a callback/hook on `route_nets` (or a config flag) over
   eprintln inside the engine.
4. Determinism unaffected: progress lines must not enter any
   committed artifact or census digest.

## Files

- `crates/grus-core/src/drt/maze.rs` (progress hook — minimal
  surface change; no shared-state change for the hook itself)
- `crates/grus-eda/src/main.rs` (wire the hook to stderr in `drt`)
- `docs/results.md` dated entry (sample output on gcd/picorv32)
