# e5-cts-ref — Overview

E5 (clock tree synthesis) ticket 1: commit the TritonCTS reference
artifacts and their reproduction script, per the scoping done-when
(`kurena/tasks/next-stage-scoping/design/done-when.md` §E5) and the
P-CTS measurements logged in `docs/results.md` (2026-11-22
"next-stage-scoping"). Reference only — no CTS implementation in this
ticket.

## Deliverables

All under `tests/designs/gcd/` (plus the picorv32 pair if included):

- `gcd.cts.def` — `gcd.placed.def` after TritonCTS +
  `repair_clock_nets`; the netlist side of the criterion-3 e2e STA
  fixture.
- `gcd.cts.reference.json` — parsed CTS metrics (schema in
  `metrics.md`): skew setup/hold, max insertion delay, per-cell-type
  buffer census, dummy loads, sinks, depth, clock-net wirelength.
- `gcd.cts.sta.reference.json` — OpenSTA propagated-clock endpoint
  golden on the CTS DEF (schema in `sta-golden.md`); the criterion-3
  comparison target.
- `gcd.cts.sdc` — `gcd.sdc` + `set_propagated_clock`; see
  `sta-golden.md` for why it is a separate file and not
  `gcd.propagated.sdc`.
- `tests/designs/generate_cts_ref.sh` — the committed generator
  (`script.md`).

## Key decisions

- **New top-level script, not a `generate_ref.sh` step.** Mirroring
  `tests/designs/generate_prop_ref.sh`: the CTS reference reads only
  committed inputs (`<d>.placed.def`, `<d>.sdc`) — no LibreLane flow
  rerun, so the new artifacts regenerate in seconds, `git diff` drift
  is scoped to the CTS outputs, and the script is design-parameterized
  (`<design>` as `$1`) so picorv32 support is free.
- **Metrics come from three tool sources**, each named per field in
  the JSON (`metrics.md`): the `report_cts`/`report_clock_skew`/
  `report_cts_config` console transcript (captured via `tee` — the
  step-9c precedent), an in-process odb census of inserted instances
  (before/after diff inside the CTS process — no DEF text diff), and
  `worst_clock_skew`. Nothing is hand-written; python3 only formats
  what the transcript/census files contain (the `guide_caps.py`
  precedent).
- **picorv32 is included if it lands cleanly.** The script takes the
  design name; `picorv32.placed.def` is gitignored-but-seeded.
  `picorv32.cts.def` (~7 MB) and `picorv32.cts.sta.reference.json`
  (~3 MB) follow the picorv32 convention: gitignored, SHA-256 prefixes
  in `tests/designs/README.md`. `picorv32.cts.reference.json` (scalar
  metrics) commits like `picorv32.reference.json`. If the run exposes
  a surprise (multi-clock, long runtime, nondeterminism), defer to
  ticket 2 with the reason in `docs/results.md`.
- **No version bump.** No Cargo-visible behavior changes; the goldens
  have no consumer test yet (that is the next E5 ticket). The commit
  states this per the acceptance checklist.

## Files

Implementation is expected to touch:

- `tests/designs/generate_cts_ref.sh` (new)
- `tests/designs/gcd/gcd.cts.sdc` (new)
- `tests/designs/gcd/gcd.cts.def` (generated, committed)
- `tests/designs/gcd/gcd.cts.reference.json` (generated, committed)
- `tests/designs/gcd/gcd.cts.sta.reference.json` (generated, committed)
- `tests/designs/picorv32/picorv32.cts.sdc` (new)
- `tests/designs/picorv32/picorv32.cts.def` (generated, gitignored)
- `tests/designs/picorv32/picorv32.cts.reference.json` (generated, committed)
- `tests/designs/picorv32/picorv32.cts.sta.reference.json` (generated, gitignored)
- `.gitignore`
- `tests/designs/README.md`
- `docs/results.md`
