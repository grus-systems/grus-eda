# Goal

E9 ticket 3 (review items 4+5 from
`docs/history/run-2026-11-22-large-design.md`):
`--top` ergonomics and an SDC examples doc.

## Work

- `check_top_name` in `crates/grus-eda/src/main.rs`:
  accept `--top <name>` when it equals the loaded
  design name **or** the trailing hierarchy segment
  of it — `$paramod$...\picorv32` accepts `picorv32`
  (the segment after the last `\`). Everything else
  still bails with the existing loud error (do NOT
  accept substring or prefix matches — a looser
  rule would let a wrong-design slip look like a
  match). Single-top auto-accept: check whether the
  readers already pick a unique top when `--top` is
  omitted; if a command path still demands `--top`
  for a one-module input, make the single-module
  case work without it — record which state the
  code was actually in.
- Update the CLI help text for `--top` to state the
  accepted forms; add a unit test: suffix accepted,
  exact accepted, wrong name still fails, and a
  name colliding with a non-final segment fails.
- New `docs/sdc-examples.md`: minimal working SDCs
  for the three topologies the review hit —
  (a) single clock + input/output delays;
  (b) N asynchronous clocks + `set_clock_groups
  -asynchronous` (cross-domain paths become
  `cross_domain`/`false_path`-counted — name the
  observable effect in our report);
  (c) FF clock-enable gating (the "gated" clock is
  a data enable, not a clock net — show the correct
  single-clock SDC, not a second `create_clock`).
  Every example must be a real, parseable file —
  source them from existing committed fixtures
  (`tests/designs/sta_clocks/`, `gcd.sdc`,
  `picorv32.sdc`, the `set_clock_groups`/`mc-*`
  fixtures) and cite the fixture each mirrors, or
  add the example as a new committed `.sdc` that a
  parse test reads. No untested prose examples.
- `docs/status.md` E9 row + `docs/results.md`
  pointer; version bump (patch — CLI behavior
  widens compatibly, no schema change; state the
  rationale).

## Done when

`--top picorv32`-style suffix names are accepted,
wrong names still bail loudly, the SDC examples doc
exists with parseable examples each traced to a
committed fixture, and tests cover the new accept
rule.

## Files

- `crates/grus-eda/src/main.rs`
- `docs/sdc-examples.md` (new)
- `docs/results.md`, `docs/status.md`
- optionally `tests/designs/**` fixture `.sdc`
