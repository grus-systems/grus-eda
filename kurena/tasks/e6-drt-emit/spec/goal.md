# e6-drt-emit: routed DEF emission + self-check

E6 ticket 6, depends on `e6-drt-maze`. Turns route trees into DEF
`NETS` geometry the whole toolchain already reads — and checks the
result with our own subset DRC.

## Scope

1. **Route → DEF geometry** — each committed route emits DEF `NETS`
   wire records (`+ ROUTED <layer> <width>` + point sequences with
   `NEW` branches at forks) and via records (via name at transition
   points, matching the `e6-drt-via` selection). Track-aligned
   segments collapse into maximal straight runs; the emitter output
   re-parses through our own DEF reader byte-stable (write → parse
   → write → identical).
2. **`grus-eda route`/`drt` surface** — a CLI path that takes a
   placed DEF + a guide file and writes a routed DEF (the
   delegation path's native replacement). Naming and flags mirror
   the existing commands' conventions.
3. **Self-DRC gate** — run `drc::report::run_all` on OUR routed
   output: the 50-rule subset checker must report 0 violations —
   the same bar Magic's deck put on TritonRoute's output (per-rule
   names join the committed goldens as before). Any violation is a
   real routing bug to fix or name.
4. **Structural sanity** — wire-length and via-per-cut-layer census
   vs `<d>.drt.reference.json` `variants.delegated`/`baseline`
   numbers — informational deltas, not pass/fail (topology
   differences are expected; the DRC bar is the gate).

## Non-goals

- Magic container check of our routed DEF (the e2e ticket runs the
  real deck on it).
- Antenna checks, timing-driven objectives, ECO/incremental modes.

## Files

- `crates/grus-core/src/drt/emit.rs` (new — DEF wire/via writer)
- `crates/grus-core/src/design_db.rs` or `readers/def.rs` (emission
  hook as needed)
- `crates/grus-eda/src/main.rs` (CLI)
- `crates/grus-core/tests/drt.rs` (roundtrip + self-DRC gate)
- `docs/e6-drt.md` (emission section)
- `docs/results.md` (dated entry — self-DRC census, wirelength/via
  deltas vs reference)
