# Roadmap (`docs/roadmap.md`)

grus-eda is a research prototype. It reads the outputs of a sky130 flow (netlist, placed DEF,
SDC, optionally SPEF) and estimates placement, congestion and timing in seconds instead of
minutes. Each stage is checked against OpenROAD or OpenSTA on the same inputs; the current
state of each stage is in [`status.md`](status.md).

This file lists what is missing before the tool can be trusted on designs beyond plain
flip-flop logic. It is a plan, not a promise: items can change order or be dropped when a
measurement says so.

## Principles

- **The reference tool decides.** An item is done when grus-eda matches OpenSTA (or OpenROAD)
  on the same inputs within a stated tolerance, recorded in [`results.md`](results.md).
  Passing our own unit tests is not enough (`AGENTS.md` Section 2).
- **Public designs only.** Every test input is a public design (the OpenROAD-flow-scripts
  `sky130hd` designs, `picorv32`, `ibex`, or a small hand-written fixture that uses only
  `sky130_fd_sc_hd` cells). Designs or numbers from private work never enter this repository.
- **Fail loudly, but say how much is unsupported.** A feature we do not model must not
  silently shrink the answer. Either the run stops, or the report states what was skipped
  and how much of the design it covers.
- **OpenSTA source is not read or copied.** OpenROAD is BSD-licensed and may be read;
  OpenSTA is GPL and is used only through its published papers and its output.

## What fails today

Running grus-eda on the output of a real LibreLane run of a design with latches and clock
gating shows four classes of difference from OpenSTA:

1. Setup slack is too optimistic and hold is not detected at all: the clock is ideal, but
   the real clock tree has cell-driven nets with large latency and skew.
2. Tens of thousands of timing arcs are skipped (latch arcs, clear/preset arcs). The
   skipped count is printed as a warning but does not say what fraction of the design is
   affected.
3. The LibreLane SDC is rejected as written; it had to be rewritten by hand.
4. `set_timing_derate` is dropped, which shifts every path.

The items below are the work that closes these gaps, in the order proposed at the end.

## Work items

### R0. Say what was skipped

- Summarise skipped arcs, cells and SDC commands by kind and count, and as a share of the
  design's arcs and endpoints, at the end of every run.
- Machine-readable output (JSON) with the same summary; a non-zero exit code when a
  strict flag is set and anything was skipped.
- A short status note at the top of the README (research prototype; known unsupported
  features).
- Done when: a run on a design with latches prints a count per skipped kind, and the
  numbers match an independent count from the Liberty and netlist.

### R1. Accept real SDC files

- Support or explicitly ignore, with a recorded reason: `set_clock_transition`,
  `set_propagated_clock`, `set_driving_cell`, `set_max_transition`, `set_max_capacitance`,
  `set_max_fanout`, `-add_delay`, `-pin_load`, and `[get_clocks ...]` in every position
  where OpenSTA accepts it.
- Decide the policy for unsupported commands: stop (today) or warn and continue behind an
  explicit `--lenient` flag. R0 makes the lenient mode safe to offer.
- Decide whether `set_false_path` and `set_multicycle_path` are in scope; today they are
  rejected.
- Done when: the SDC emitted by a stock LibreLane run on a public design parses unchanged,
  and every command is either modelled or listed in the R0 summary.

### R2. Propagated clock

- Compute clock arrival through buffers and gates on the clock network, per register pin,
  for both rise and fall; apply `set_clock_latency` and `set_clock_uncertainty` on top.
- Keep the ideal-clock mode as the default when the SDC does not ask for propagation.
- Decide whether clock reconvergence pessimism removal (CRPR) is needed for the tolerances
  we want, or whether it can wait.
- Done when: on a public design with a buffered clock tree, setup and hold WNS/TNS and the
  per-endpoint arrival match OpenSTA within the default tolerance.

### R3. Hold analysis

- Make the minimum-delay path analysis correct with propagated clocks: early/late
  arrival, hold requirement from the Liberty tables, and the hold-side uncertainty.
- Done when: the hold violation count and worst hold slack on a public design match
  OpenSTA. This item depends on R2, because hold violations come from clock skew.

### R4. Clock gating

- Model gating cells on the clock path: AND/OR style gates and integrated clock-gating
  cells (`sky130_fd_sc_hd__dlclkp`), including the clock-gating setup and hold checks on
  the enable pin.
- The `dlxtn` + `and2` gate in `ibex` is already handled for endpoints
  ([`history/step4/10-latch-and-clock-gating.md`](history/step4/10-latch-and-clock-gating.md));
  extend it to the cases that need a propagated clock.
- Done when: a public design (or a small fixture) with an integrated clock-gating cell
  matches OpenSTA on the gating checks and on the gated registers' arrival.

### R5. Latches

- Today latch endpoints are collected, but the D→Q transparent path and time borrowing
  are not modelled, and only `dlxtn` is covered.
- Model transparency (a path through D→Q while the latch is open), time borrowing, and the
  other `sky130_fd_sc_hd` latch cells.
- Done when: a fixture with a latch-based path matches OpenSTA on arrival, required time
  and borrowed time at the latch.

### R6. Asynchronous arcs

- Propagate through clear/preset arcs and add recovery/removal checks.
- Done when: a design with asynchronous resets matches OpenSTA on the reset-path
  endpoints (the async endpoints already collected are the starting point).

### R7. Timing derate

- `set_timing_derate` with `-early`/`-late` and cell/net delay selection.
- Done when: the same design with and without a derate matches OpenSTA in both runs.

### R8. Libraries and corners

- Check which sky130 library variants and PDK layouts are accepted, and which corner
  files can be loaded. Today the verified setup is `sky130_fd_sc_hd__tt_025C_1v80`.
- Done when: the accepted variants are listed in `status.md`, each with a verified run.

### R9. Known accuracy gaps (carried over)

- The placement-based RC estimate differs from routed results (the `placement RC` tests on
  `picorv32`, `aes`, `ibex` and `jpeg` are `#[ignore]`d and fail).
- The E1 hot-set overlap on `picorv32` is `0.4698`, below the `0.50` criterion.
- Out of scope: the extreme load/slew extrapolation on `jpeg`, which OpenSTA itself does
  not support (`OpenSTA #582`).

### R10. Test and CI foundations

- One public reference case per item above, generated by a committed script in the pinned
  LibreLane image (the existing `generate_ref.sh` pattern).
- Confirm that the CI `generate` and `test` jobs run end to end on the self-hosted runner
  from a clean clone, and keep the golden files small (< 1 MB).
- Done when: a clean clone passes `cargo test --release` in CI.

## Proposed order

1. R0 and R1: the tool becomes usable and honest on plain flip-flop designs.
2. R2 and R3: propagated clock and hold. This closes the largest difference.
3. R4, then R5 and R6: gating, latches, asynchronous arcs.
4. R7 and R8, then R9.

R10 runs alongside every step.
