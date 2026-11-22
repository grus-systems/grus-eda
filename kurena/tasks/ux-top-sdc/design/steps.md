# Implementation and verification plan

## Ordered steps

1. **Confirm the pre-change single-top behavior.**
   - Re-check `parse_yosys_json_bytes`, the Verilog module-name reader, and `check_top_name(None, ...)`.
   - Expected pre-change finding: `--top` is optional on `check-e0`/`floorplan`; single-module `gcd` inputs auto-select successfully. Record this exact finding in `docs/results.md`.
   - If a command path unexpectedly still requires `--top`, implement the narrowest fix there and record the discrepancy instead of adding unrelated top-selection logic.

2. **Implement `--top` matching in `crates/grus-eda/src/main.rs`.**
   - Add `top_name_matches(requested, actual)` beside `check_top_name`.
   - Accept `None`, exact equality, or the non-empty final literal-`\` segment only.
   - Preserve the current `{kind} design name {actual:?} does not match --top {top:?}` bail message unless a stronger message is added without weakening the named values.

3. **Update help text in the same file.**
   - Change both `CheckE0Args::top` and `FloorplanArgs::top` to state: exact design name or final hierarchy segment after `\`; omit to auto-select.
   - Check `cargo run -p grus-eda -- check-e0 --help` output if convenient, but do not require a PDK load for the help check.

4. **Add focused tests in `main.rs`.**
   - Add `check_top_name_matching_rules` (exact, final segment, wrong/substring candidates, non-final segment, `None`, and empty-name edge if implemented).
   - Change the successful `check-e0` invocation in `cli_end_to_end_gcd_flow_and_validation` to omit `--top`, preserving the wrong-`--top` rejection check.
   - Optionally add one no-`--top` `floorplan` invocation with a temp output file if it remains a few lines and deletes its output.
   - Add `sdc_examples_parse`, which extracts exactly three fenced `sdc` blocks from `docs/sdc-examples.md` and calls `grus_core::sta::parse_sdc` on each.

5. **Create `docs/sdc-examples.md`.**
   - Use the three examples and fixture citations specified in `sdc-examples.md`.
   - Keep exactly three ` ```sdc` fences.
   - Explicitly state that asynchronous groups desugar to intentional false-path cuts and appear as `skipped.false_path_endpoints`, while `skipped.cross_domain_endpoints` is for ungrouped/unpairable `CrossDomain` skips.
   - Explicitly state that an FF enable/select is a data input and must not receive a second `create_clock` or a `set_clock_groups` entry.

6. **Bump the workspace patch version.**
   - The inspected tree has `[workspace.package] version = "0.6.0"`; implementation should bump the actual value found at that time by one patch (`0.6.0 -> 0.6.1` if unchanged).
   - Rationale to record: this is a backward-compatible widening of CLI argument acceptance plus documentation/tests; there is no schema or reference-format change.

7. **Update `docs/status.md`.**
   - Move E9 items `(4) --top suffix/auto ergonomics` and `(5) SDC examples doc` to DONE under the new patch version and `ux-top-sdc` ticket.
   - Keep `(2) per-clock-domain summary` and `(3) slack histogram` OPEN under `ux-sta-summary`.
   - Update the `Last updated:` header in the established format.
   - Consider adding `sta_clocks` to the E9 `Designs covered` cell because the async SDC example is fixture-traced to it.

8. **Add a dated `docs/results.md` entry.**
   Include, at minimum:
   - Pre-change finding: single-top auto-accept already worked via reader selection plus `None` acceptance, with the command/test that demonstrated it.
   - The four matching cases and expected pass/fail result.
   - The wrong-name/non-final collision rejection behavior and error wording.
   - The three SDC blocks and their fixture traces (`gcd.sdc`, `t2_grp.sdc`/`two_clock.def`, `base.sdc`/`exceptions.def`).
   - The async report semantics (`endpoints[]` 8 -> 5 on `t2_grp`; intentional cuts under `skipped.false_path_endpoints`; `cross_domain_endpoints == 0` for the committed scenario).
   - Version bump rationale.
   - Exact commands run and their outcomes.

9. **Run focused checks first.**
   - `cargo test -p grus-eda check_top_name_matching_rules`
   - `cargo test -p grus-eda sdc_examples_parse`
   - `cargo test -p grus-eda cli_end_to_end_gcd_flow_and_validation`
   - If `check-e0` help output is manually inspected, record that separately rather than pretending it is a test.

10. **Run the required gates.**
    - `cargo test --workspace`
    - `cargo clippy --workspace --all-targets -- -D warnings`

11. **Self-review the final diff.**
    - No matching logic looser than exact-or-final-segment.
    - Both `--top` help strings updated.
    - Exactly three parseable `sdc` blocks.
    - No new `create_clock` for an enable.
    - `docs/status.md` and `docs/results.md` agree with the code and version.
    - `Cargo.toml` patch bump matches the implementation-time base version.

## Fail-proof expectation

For the new matching rule, a deliberately wrong implementation must turn the tests red:

- `actual.ends_with(requested)` would wrongly accept `corv32`.
- `actual.split('\\').any(|seg| seg == requested)` would wrongly accept `sub`.
- `actual.contains(requested)` would accept unrelated substrings.

A short red/restore check is appropriate if a test ever looks suspicious; if performed, record the exact corruption and restored result in `docs/results.md`.

## Expected outcome

The final diff should stay confined to the files below. No fixture or reference JSON regeneration is required because all cited SDC inputs already exist and the document parse test reads the examples directly.

## Files

- `crates/grus-eda/src/main.rs`
- `docs/sdc-examples.md`
- `docs/status.md`
- `docs/results.md`
- `Cargo.toml`
