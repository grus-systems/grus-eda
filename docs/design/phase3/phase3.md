# Phase 3 — reference-tool checks and release gate

Design for the "Remaining Reference-Tool Checks & Release Gate" item in
`docs/status.md` (Remaining Follow-ups 2). Requires the M4 / Docker route
(`ghcr.io/librelane/librelane:3.0.16`, Colima); generation runs on the
self-hosted Mac mini (the self-hosted Mac mini over SSH), never on this
workstation. Implementation tickets live in [`phase3/`](.). Like
Phase 2, this round is implemented by the maintainer directly.

## 1. Scope

| Ticket | Scope | Done when |
|---|---|---|
| [phase3/01-picorv32-e1e2-goldens.md](01-picorv32-e1e2-goldens.md) | Extend `tests/designs/picorv32/generate_ref.sh` with the E1/E2 steps the other four designs have (verilog counts, orient fixture, `util65.def`, GCell counts and sweeps, `gcell_usage.txt`, `reference.json`); add `test_picorv32_congestion_map` and the missing HPWL-gap assert in `test_picorv32_place_and_legalize` | picorv32 E1 map test and E2 HPWL check run and are recorded in `docs/results.md` (pass or `#[ignore]`d with measured numbers, per the F2 precedent — thresholds are never fitted to the measurement) |
| [phase3/02-sta-edge-cases.md](02-sta-edge-cases.md) | The three "Not scheduled" items of `docs/e3-sta.md` Section 5: SPEF resistor loops, `set_clock_uncertainty` on non-register endpoints, non-`dlxtn` latches. Hand-written minimal fixtures + OpenSTA goldens; measure first, fix only what is a real divergence | each case has a committed fixture, a golden emitted by OpenSTA as written, and a test; divergences are either fixed or recorded with root cause |

Out of scope: the `dev -> main` merge decision (the user's call), and the
`Accelerate`/Metal evaluation (optional Phase 2 leftover).

## 2. Rules

- Same as `AGENTS.md` §2 and the Phase 2 round: goldens are emitted by the
  committed script run as written inside the pinned container; never hand-write
  or round reference numbers; record every comparison in `docs/results.md`.
- Tcl fed to `openroad`/`sta` on stdin: `!` is history expansion — write
  `== 0`, never `![...]` (AGENTS.md §7 rule 3).
- Generated artifacts over 1 MB stay git-ignored with SHA-256 prefixes in
  `tests/designs/README.md`; the `generate_ref.sh` script must reproduce them.
- Fixtures for ticket 02 are hand-written DEF/SPEF/SDC. They are test inputs,
  not reference outputs — the reference outputs are the OpenSTA JSON goldens,
  and the fixture files themselves must be small enough to commit (< 1 MB).
- picorv32 artifacts already committed/generated are inputs; the new steps must
  not change `picorv32.placed.def`, `picorv32.routed.spef`,
  `picorv32.rcx.spef`, or `picorv32.sta.reference.json` (verify with `git
  status`/`sha256` after a full run).
