# e8-irdrop-ref — Overview

E8 (IR drop / power-grid analysis) ticket 1: commit the PDNSim reference
artifacts and their reproduction script, per the scoping done-when
(`kurena/tasks/next-stage-scoping/design/done-when.md` §E8) and the P-IRD
measurements logged in `docs/results.md` (2026-11-22
"next-stage-scoping"). Reference only — no solver implementation in this
ticket.

## Deliverables

- `tests/designs/generate_irdrop_ref.sh` — the committed generator
  (`script.md`). Design-parameterized (`<design>` as `$1`), host wrapper
  re-invoking itself inside `ghcr.io/librelane/librelane:3.0.16`,
  mirroring `tests/designs/generate_cts_ref.sh` / `generate_prop_ref.sh`.
- `tests/designs/gcd/gcd.irdrop.reference.json` — per net (`VPWR`,
  `VGND`) and per current-source convention (`embedded`,
  `sta_per_instance`): connectivity verdict, total power, supply
  voltage, average voltage, average + worst drop/bounce, analyzed
  counts, CSV file names + SHA-256 prefixes (`golden.md`). Every value
  emitted by Tcl/python inside the container run — nothing
  hand-written.
- `tests/designs/gcd/gcd.irdrop.voltage*.csv` — the per-instance
  voltage CSV(s), the criterion-2 golden. Naming and commit rules in
  `golden.md`.
- `tests/designs/picorv32/picorv32.irdrop.*` — same set, committed or
  gitignored per the < 1 MB rule, **if** the run is clean; otherwise
  explicitly deferred with the reason in `docs/results.md`.
- `docs/results.md` entry, `tests/designs/README.md` artifact row,
  `docs/roadmap.md` §E8 status line.

## Key decisions

- **New top-level script, not a `generate_ref.sh` step.** Same
  reasoning as `e5-cts-ref`: the IR-drop reference reads only committed
  inputs (`<d>.placed.def`, `<d>.sdc`) — no LibreLane flow rerun, so
  artifacts regenerate in seconds and `git diff` drift is scoped to the
  IR-drop outputs. `<design>` is `$1` so picorv32 support is free.
- **Both current-source conventions are measured in one script run**
  (`conventions.md`): (a) PDNSim's embedded per-instance power
  estimate, and (b) `set_pdnsim_inst_power -inst <name> -power <watts>`
  fed from `report_power -instances [get_cells *]` produced in the same
  container run. Convention (b) is the expected mirror for the E8
  engine (our N7 `power` emits per-instance power); the golden pins the
  choice and the measured delta.
- **Two openroad processes, one per convention.** The
  clean-state precedent (step 9b/9c, `generate_cts_ref.sh` step C):
  process A measures connectivity + the embedded convention + dumps the
  per-instance power report; a python step turns that report into a
  sourced Tcl of `set_pdnsim_inst_power` calls; process B measures the
  explicit convention. This also isolates the convention-B measurement
  from any solution-reuse state in process A.
- **Both nets, both semantics.** `check_power_grid` and
  `analyze_power_grid -voltage_file` run on `VPWR` **and** `VGND`. The
  golden records whether the `VGND` report describes a bounce (rise) or
  a drop, verbatim — semantics are read from the report wording, not
  assumed.
- **picorv32 is included if it lands cleanly.** `picorv32.placed.def`
  exists (gitignored, seeded; 10,205 components, VPWR/VGND
  SPECIALNETS). Runtime of the resistor-mesh solve at that size is the
  open question — if the run is clean the artifacts follow the picorv32
  convention (commit < 1 MB, else gitignore + SHA-256 prefix); if it
  exposes a surprise (runtime, nondeterminism, parser drift) defer with
  the reason in `docs/results.md`.
- **No version bump.** Nothing Cargo-visible changes; the goldens have
  no consumer test yet (that is the next E8 ticket). The commit states
  this per the acceptance checklist. `cargo test --workspace` and
  `cargo clippy --workspace --all-targets -- -D warnings` still run
  clean (verify commands).

## Reference facts (P-IRD, `docs/results.md` 2026-11-22)

- Command surface in the pinned image: `analyze_power_grid`,
  `check_power_grid`, `set_pdnsim_net_voltage`,
  `set_pdnsim_source_settings`, `set_pdnsim_inst_power`. No
  `*psm*`/`*irdrop*` commands.
- `analyze_power_grid -net VPWR` on `gcd.placed.def`: total power
  1.04 mW, supply 1.80 V, average voltage 1.80 V, average IR drop
  0.223 mV, worst-case IR drop 0.591 mV (0.03%).
- `-voltage_file` emits a per-instance CSV
  (`Instance,Terminal,Layer,X location,Y location,Voltage`, 384+ rows).
- `check_power_grid`: "All shapes on net VPWR are connected."
- `-outfile` is not a flag (`STA-0562`); `-help` surfaces are absent —
  probe flag signatures by executing, and abort loudly on an unmatched
  parser surface.
- `set_pdnsim_inst_power` signature (OpenROAD docs, BSD): `-inst
  <name> -power <watts> [-corner <corner>]`; overrides the computed
  power for one instance.
- `report_power -instances <insts>` prints a per-instance
  Internal/Switching/Leakage/Total (Watts) table; `[get_cells *]` is
  the documented way to cover the whole design.

## Files

Implementation is expected to touch:

- `tests/designs/generate_irdrop_ref.sh` (new)
- `tests/designs/gcd/gcd.irdrop.reference.json` (generated, committed)
- `tests/designs/gcd/gcd.irdrop.voltage*.csv` (generated, committed)
- `tests/designs/picorv32/picorv32.irdrop.reference.json` (generated, committed — if run clean)
- `tests/designs/picorv32/picorv32.irdrop.voltage*.csv` (generated, committed or gitignored — if run clean)
- `.gitignore` (only if picorv32 CSVs exceed 1 MB)
- `tests/designs/README.md`
- `docs/results.md`
- `docs/roadmap.md`
