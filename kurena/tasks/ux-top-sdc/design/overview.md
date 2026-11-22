# E9 `ux-top-sdc` design overview

## Goal

Close the two remaining E9 items owned by this ticket without changing validation semantics:

1. Make `--top` usable on mangled Yosys names such as `$paramod$...\picorv32` while keeping name validation strict.
2. Add tested, fixture-traceable SDC examples for a single clock, asynchronous clock groups, and a flip-flop clock-enable/select signal.

The change is intentionally small: one helper in `crates/grus-eda/src/main.rs`, focused unit coverage in the same file, one new user-facing document, status/results updates, and a patch version bump.

## Current behavior

- `check_top_name(expected, kind, actual)` accepts only `expected == actual` or `expected == None`.
- It is called once per loaded netlist in `check-e0` (Yosys JSON, Verilog, DEF) and once in `floorplan`.
- Omitting `--top` already works on single-top inputs: the Yosys reader selects the first non-blackbox module with cells (first module as fallback), the Verilog reader selects the first `module` declaration, and `check_top_name(None, ...)` performs no comparison. No new auto-detection code is needed unless implementation inspection finds a path that still demands `--top`.
- `--top` currently has the generic help text `Top-level module name` in both `CheckE0Args` and `FloorplanArgs`.
- Committed SDC fixtures already cover the requested patterns:
  - `tests/designs/gcd/gcd.sdc`: one clock plus input/output delays and transition/load.
  - `tests/designs/sta_clocks/t2_grp.sdc` on `two_clock.def`: two clocks cut by `set_clock_groups -asynchronous`.
  - `tests/designs/sta_exceptions/base.sdc` on `exceptions.def`: `sel` is an enable/select driving `u_mux/S` and `u_mux2/S` into `u_ff1/D`/`y2`; only `clk` has `create_clock`.

## Deliverables

1. Replace exact-only `--top` checking with exact-or-final-backslash-segment matching.
2. Keep rejection loud for unrelated names, substring/prefix matches, and strings equal to a non-final segment.
3. Update both `--top` help strings to name the accepted forms and the omitted-argument behavior.
4. Add unit tests in `main.rs` for exact match, final-segment match, wrong-name rejection, non-final-segment rejection, and omitted `--top` acceptance.
5. Exercise single-top auto-accept in the existing GCD CLI test by omitting `--top` on `check-e0`; add the same omitted-`--top` floorplan smoke check if it stays small.
6. Add `docs/sdc-examples.md` with exactly three fenced `sdc` examples, each traceable to a committed fixture.
7. Add a test in `main.rs` that extracts the three fenced `sdc` blocks from `docs/sdc-examples.md` and passes each through `grus_core::sta::parse_sdc`.
8. Bump `[workspace.package] version` from the version present at implementation time (`0.6.0` in the inspected tree) by one patch release.
9. Update `docs/status.md` E9 and add a dated `docs/results.md` entry that records pre-change findings and the exact commands run.

## Non-goals

- No substring, prefix, glob, regex, or multi-delimiter top-name matching.
- No new top-selection policy for multi-module inputs.
- No new SDC parser commands or STA semantics.
- No second `create_clock` for an FF enable/select signal.
- No `set_clock_groups` in the single-clock examples.
- No change to `set_clock_groups -allow_paths`; `t2_grpa.sdc` is a contrasting fixture, not the asynchronous-cut example.
- No golden/reference regeneration and no external OpenSTA run: this ticket changes CLI validation, tests, and documentation only.

## Spec discrepancies to preserve

The goal text requires both a patch bump and a `docs/results.md` record, but the spec's `## Files` section omits `Cargo.toml` and the acceptance checklist asks only for the pre-change state, not the usual exact command log. The goal text and repository reporting convention win for this design: implementation must still update `Cargo.toml` and `docs/results.md` and record the pre-change single-top state plus exact commands run.

## Acceptance map

- `--top picorv32` on `$paramod$...\picorv32`: covered by the final-segment rule and a direct unit test.
- Validation not weakened: wrong names, non-final segments, and substring/prefix candidates stay hard errors.
- Single-top auto-accept: existing reader behavior is recorded; the GCD CLI test exercises `check-e0` without `--top`.
- Help updated: both Clap `top` fields get the new text.
- SDC examples: `docs/sdc-examples.md` has exactly three parseable `sdc` blocks and cites committed fixtures.
- Every example tested: `sdc_examples_parse` extracts all three blocks and calls `parse_sdc`.
- Status/results/version: `docs/status.md`, `docs/results.md`, and root `Cargo.toml` are updated in the same change.
- Gates: `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` must pass.

## Files

- `crates/grus-eda/src/main.rs`
- `docs/sdc-examples.md`
- `docs/status.md`
- `docs/results.md`
- `Cargo.toml`
