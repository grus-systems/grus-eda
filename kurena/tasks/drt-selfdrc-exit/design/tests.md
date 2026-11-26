# drt-selfdrc-exit — test plan

Both tests live in `crates/grus-eda/src/main.rs`'s existing
`#[cfg(test)] mod tests` — the same idiom as
`cli_end_to_end_gcd_flow_and_validation`: real PDK via `sky130_hd()`,
committed gcd inputs, `run(Cli::try_parse_from(...))` called
in-process. The exit path is `run()` returning `Err`, so no binary
spawn or stdout capture is needed. `crates/grus-eda-core/tests/drt.rs`
gets no new test (see `overview.md` — Files).

## 1. `cli_drt_parses` — update in place

Extend the existing arg-parse test:

- `--no-self-drc` parses; `--self-drc` still parses.
- `--self-drc-tolerance 3` parses into `args.self_drc_tolerance == 3`;
  default is 0.
- `--no-self-drc --self-drc-tolerance 3` is a clap error
  (`conflicts_with`).
- `--no-self-drc --self-drc` (in that order) re-enables
  (`overrides_with`, last wins) — assert `!args.no_self_drc`.

## 2. `cli_drt_selfdrc_gates_exit` — new

Inputs: `sky130_hd()` tlef/lef, `tests/designs/gcd/gcd.placed.def`,
`gcd.guide`, out DEFs under `std::env::temp_dir()`. Three `run()`
calls ≈ 3 × ~13 s (gcd `route_ms=12647` in the golden) — inside the
suite's budget.

| Arm | argv extras | Expected |
|-----|-------------|----------|
| clean | (defaults, occ enabled) | `run()` → `Ok`; gcd's pinned self-DRC is `findings_total=0`. Also pins that `unrouted=80` does **not** trip the gate (connectivity census, not a DRC gate). |
| injected | `--occ disabled` | `run()` → `Err`; message contains `self_drc findings_total=`; parse the integer and assert `> 0`; assert `--out-def` file exists (emit artifact preserved despite the verdict). |
| tolerated | `--occ disabled --self-drc-tolerance 1000000` | `run()` → `Ok` — exercises the TOLERATED verdict. |
| skipped | `--occ disabled --no-self-drc` | `run()` → `Ok` — the opt-out contract. |

### Why `--occ disabled` is the injected violation (not a mock)

`test_emit_fail_proof` Arm B (`tests/drt.rs` ~L2808-2824) already
proves through the identical write → `parse_def` → `assemble` →
`run_all` path that an occupancy-disabled emit on gcd yields
`findings_total >= 1` — real wires committed at real sub-bound gaps
by the real router, scored by the real checker on the real emitted
text. The CLI arm reuses exactly that mechanism; no fixture DEF or
mocked report is introduced. The arm asserts on the finding count
parsed out of the error message, so a silently-zero checker would
fail the test too (the Err would still fire — assert the parsed
`findings_total` digit, not just `is_err()`).

### Prove-it-red step (AGENTS §7.7)

During implementation: comment out the `bail!`, confirm the injected
arm goes red (run returns Ok where Err was asserted), restore.
Record the red run in `docs/results.md`.

### Non-vacuity already covered

The clean arm proves the gate doesn't false-positive on a 0-finding
emit; the tolerated/skipped arms prove the escape hatches don't trip
it. The emitted-DEF existence check pins "artifact still written".

## Verify commands (workflow.toml)

```
cargo test --workspace --profile verify
cargo clippy --workspace --all-targets -- -D warnings
```

Focused runs while iterating:
`cargo test -p grus-eda --bin grus-eda cli_drt_selfdrc_gates_exit -- --nocapture`.
