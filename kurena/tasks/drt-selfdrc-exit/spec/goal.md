# drt-selfdrc-exit: self-DRC findings must reach the exit code

External review S2-1: "`routed` is not DRC-clean" — external-pin
collisions are soft penalties (never forbidden even on the final
hard iteration), all-pins-connected returns `Routed`, and self-DRC
runs only when asked, with findings not reflected in the exit
code. A caller can read `routed=N` + exit 0 and reasonably believe
the emit is clean; today it can still carry real violations.

## Scope

1. `grus-eda drt` exit code: nonzero when self-DRC reports findings
   (current behavior runs self-DRC by default — findings_total
   appears in the census line). Decide and record whether
   `--no-self-drc` (if present) keeps exit 0, and whether a
   `--self-drc-tolerance` escape hatch is warranted for known
   named residuals; the default must be fail-loud.
2. Census line: make the semantic gap explicit — a `routed` count
   is connectivity-only; when findings exist the line already shows
   findings_total=N (keep) — add nothing that lets `routed` be
   misread as clean (e.g. print `self_drc` before the unrouted
   list, or a trailing `CLEAN`/`VIOLATIONS` marker — driver's
   choice, document it).
3. Engine side: if `route_nets`/`apply_routes` can represent the
   routed-but-violating state explicitly (e.g. a
   `RouteOutcome::RoutedWithFindings` or the census fields already
   suffice), prefer the honest enum over implicit state. Small.
4. Tests: a fixture/fault-injection asserting nonzero exit when
   findings exist (the mutation/probe idiom already in the tree).

## Files

- `crates/grus-eda/src/main.rs` (exit code + census line order)
- `crates/grus-eda-core/src/drt/` (only if the honest-enum direction
  lands)
- `crates/grus-eda-core/tests/drt.rs` (exit-code test)
- `docs/results.md` dated entry + `docs/e6-drt.md` contract note
  (what `routed` does and does not claim)
