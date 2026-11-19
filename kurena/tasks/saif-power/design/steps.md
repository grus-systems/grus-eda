# Implementation steps — revision fix pass

The feature is already implemented (`kurena: implement saif-power`,
v0.1.96). This pass applies the rejected-review fixes: the verification
record must match what was measured, and the odd-TC split must be
golden-covered. One logical unit per commit; Conventional Commits + agent
trailer per `AGENTS.md` §5. No version bump — `0.1.96` already carries
this in-flight change.

## Step 1 — fixture + script fixes (container work)

- `vcd2saif.py`: add `--window-end <ts>` overriding aggregation `last_ts`
  and `DURATION`.
- `generate_saif_ref.sh`:
  - invoke `python3 vcd2saif.py --ghost-scope tb/dut --window-end 1990
    mb.vcd mb.saif`; fix the header comment's window wording;
  - fix the session-B comment: committed `mb.saif.openroad.txt` is
    `grep -v`-filtered stdout (INFO/WARNING/banner/blank), not "whole
    stdout";
  - rewrite §5 probe variants to mutate via name-anchored regexes
    (`fixture.md` §3): `tc10`, `duty_a`, `duty_y`, `igtx` (on `y`),
    `port`, `noghost`.
- Run the committed script end-to-end inside
  `ghcr.io/librelane/librelane:3.0.16` (Colima). Verify the regenerated
  `mb.saif` (`clk`/`CLK` `TC 199 T0 990 T1 990`; `a`/`q` `T0 1180`;
  `y`/`Y` `T1 1180`; `DURATION 1980`), then rerun and confirm `git diff`
  is empty (AGENTS §7 rule 1).
- Commit: `test: regenerate microbench SAIF fixture with odd-TC window`
  — regenerated `mb.saif`, `mb.saif.openroad.txt`, `mb.saif.reference.json`
  + script/converter changes.

## Step 2 — test updates

- `saif.rs`: `test_split_tc` gains the `t0 == t1` tie case → `(100,99)`;
  `test_resolve_nets_ports_pins_and_drops` mirrors the committed fixture
  (`clk`/`CLK` `sig(199, 990, 990)` → `(100,99)`).
- `power.rs`: replace the bit-for-bit SAIF==VCD report assertion with the
  per-net comparison (`fixture.md` §Test) + a named bound on the report
  delta; fix the stale `clk TC=200` comment.
- `cargo test --workspace`, `cargo clippy --workspace --all-targets --
  -D warnings`.
- Commit: `test: cover odd-TC split and window delta in SAIF golden test`.

## Step 3 — `docs/results.md` correction

Update the 2026-11-19 entry (still in-flight on this branch — correct in
place) per `probe.md` P4:

- window `[10, 1990]` = `DURATION 1980`; `clk TC 199` odd, tie→rise →
  `(100,99)`; SAIF window is 10 ns shorter than the VCD annotation window
  and the `clk` per-net delta (one `#2000` fall) is named;
- `igtx` probe recorded as mutating `y` — now true of the script;
  `duty_a`/`duty_y` replace `duty_clk` (clk duty is degenerate under the
  truncated window — say so);
- note the `grep -v` WARNING/INFO filter on `mb.saif.openroad.txt`;
- refreshed measured table + golden numbers from the regen;
- "Checks run" gains `cargo test --workspace` (full suite);
- `docs/roadmap.md` N7: adjust wording if it cites the old window.
- Commit: `docs: correct SAIF probe targets and window in results`.

## Verification gates

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`
- `bash tests/designs/microbench/generate_saif_ref.sh` idempotent (rerun →
  clean `git status`)
- `grus-eda power --saif tests/designs/microbench/mb.saif` end-to-end CLI
  run recorded (stderr shows `saif_unmatched_scopes = 1` from the ghost)
- Prove the test can fail: corrupt `clk` `TC 199`→`398` → red → restore.

## Out of scope (stays open)

- `when`-qualified `internal_power` blocks (`conditional_power_blocks`
  diagnostic already counts them).
- Matching OpenSTA's statistical activity propagation on large designs
  (gcd-class divergence, already documented for VCD).
- State-dependent leakage from `T0`/`T1` duty (Liberty `leakage_power` `when`
  conditions are not modeled; flat `leakage_power_w` stands).
