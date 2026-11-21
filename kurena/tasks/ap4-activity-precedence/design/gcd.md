# gcd — `report_power` re-comparison protocol

The end-to-end check: the propagated activity model on a real design,
against the OpenSTA numbers in `docs/results.md`. No new committed
test — gcd is CLI-scale (`no gcd power test exists` is the standing
note); the comparison lands as a results entry.

**Base state**: the comparison table landed in `22b96d6` and stands
(every group within ~2%). The revision keeps the numbers and repairs
the *record*: reproducible diagnostics, `gcd.vcd` provenance, and the
card-question answers.

## Inputs — identical to the reference run

Reference: OpenROAD `read_vcd -scope gcd_tb/dut` + `report_power` via
the committed `tests/designs/gcd/power/power_vcd_check.tcl` (LibreLane
3.0.16, `tt_025C_1v80`, `gcd.placed.def`, `gcd.sdc`, `gcd.vcd`) — the
2026-11-18 N7 entry. Our run consumes the same four inputs:

```sh
cargo run --release -p grus-eda -- power \
  --lib   "$PDK/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib" \
  --tlef  "$PDK/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef" \
  --lef   "$PDK/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef" \
  --def   tests/designs/gcd/gcd.placed.def \
  --sdc   tests/designs/gcd/gcd.sdc \
  --vcd   tests/designs/gcd/power/gcd.vcd \
  --json  /tmp/gcd.power.ap4.json
```

`$PDK` = `$PDK_ROOT/sky130A` or `~/.ciel/sky130A`. Default parasitics
= zero-wire — matching the committed table's configuration.

Also record the same command **without `--vcd`** (defaults-only run)
— one extra line in the entry. It is *not* a comparison point
(OpenSTA's reference is VCD-seeded); it documents the bare-defaults
behavior on a real design.

## `gcd.vcd` provenance (mandatory in the entry — memo item)

What is verifiable from the file itself (record verbatim):

- gitignored (`*.vcd` except `tests/designs/microbench/`), ~18 MB,
  carried in the worktree.
- Header: `$version` = **Icarus Verilog**, `$date` = `Wed Nov 18
  07:36:28 2026`, `$timescale 1ps`, scope tree `gcd_tb` → `dut` plus
  233 cell scopes — the committed `gcd_tb.v` (`$dumpfile("gcd.vcd")`,
  `$dumpvars(0, dut)`; deterministic LFSR stimulus, 400 transactions).
- Simulated netlist: `gcd.synth.v` — the flattened yosys netlist
  (dut-level leaf names `\dpath.a_lt_b$in1[9]`, `\ctrl.state.out[2]`,
  `_180_` … are synth wire names = DEF net names), plus the PDK's
  `sky130_fd_sc_hd.v` cell models (`celldefine` scopes are the 233
  cell scopes).
- Composition (measured, this session): **1509 `$var` declarations —
  223 dut-level + 1286 under the 233 cell scopes.** The cell scopes
  hold each cell's port signals *and* internals (e.g. `not0_out_Y`);
  under `annotation_net_of` the cell *output* pin vars (`_181_/Y` …)
  resolve as net drivers, while input pins, internals, and
  io-unmatched dut wires are dead. The census is an independent
  cross-check on the dead count's *composition*, not a substitute for
  the CLI number — record both, and if they disagree investigate
  rather than assume coincidence.

Reproduction command — record it in the entry and state **whether it
ran this session** (iverilog is not on the host PATH; the recipe is
unverified unless run):

```sh
iverilog -o /tmp/gcd_tb.vvp \
  tests/designs/gcd/power/gcd_tb.v \
  tests/designs/gcd/gcd.synth.v \
  "$PDK/sky130A/libs.ref/sky130_fd_sc_hd/verilog/primitives.v" \
  "$PDK/sky130A/libs.ref/sky130_fd_sc_hd/verilog/sky130_fd_sc_hd.v"
(cd tests/designs/gcd/power && vvp /tmp/gcd_tb.vvp)   # writes gcd.vcd
```

(`primitives.v` added by the implementation run: without it iverilog
fails with 560 unknown-UDP-primitive errors — the cell models'
`sky130_fd_sc_hd__udp_*` primitives live there.)

If it is not executed, the entry says the artifact is the
2026-11-18 generation carried in the worktree — never implied-run.

## The card question (answer both halves in the entry)

**(a) Did `power_vcd_check.tcl` actually re-run this session?**
The v0.3.0 entry claims a re-run quoting `Annotated 931 pin
activities` and the `STA-1453` vcd-period warning. Revision protocol:
re-run it now in the LibreLane container — the
`generate_activity_ref.sh` docker pattern:

```sh
docker run --rm -u "$(id -u):$(id -g)" \
  -v "$REPO_ROOT:/work" -v "$PDK_HOST_ROOT:/pdk" -e PDK_ROOT=/pdk \
  ghcr.io/librelane/librelane:3.0.16 \
  openroad -exit /work/tests/designs/gcd/power/power_vcd_check.tcl
```

(`docker` via `mise exec docker-cli@latest colima@latest lima@latest
--` when the daemon is Colima; `colima start --memory 12 --cpu 8`
first if it is not running.) Quote the verbatim stdout lines —
`Annotated N pin activities`, the `STA-1453` warning, the
`report_power` group table — in the revision block. If the container
cannot run, correct the claim: the OpenSTA numbers then stand as the
**2026-11-18 record**, attributed as such, and the entry says the Tcl
was not re-executed this session.

**(b) Where was `dead_annotation=1286` read from?**
Pre-revision truth (state it plainly): the counter existed only on
`ActivityResolution::dead_annotation_entries` — no CLI path printed
it, so the figure was read from a non-CLI surface (name the actual
mechanism used: temporary print / test harness — whichever it was).
Post-revision the committed CLI prints it on the `activity:` line
(api.md §7) — quote that line verbatim. The VCD census above is the
independent cross-check on the count's composition; if the CLI prints
a different number than the recorded 1286, investigate and record
which is right rather than assuming the census equality is
structural.

## Diagnostics movement lines (mandatory in the entry)

Quote verbatim from the revision run:

- the `activity:` line — now including `dead_annotation=` and
  `dead_user=` — for both the `--vcd` run and the no-annotation run;
- the `power:` diagnostics `eprintln` values (`cells_without_lib_cell`,
  `driven_nets_without_activity`, `active_nets_without_cell_driver`,
  `internal_power_without_slew`, `unresolved_related_pin`,
  `conditional_power_blocks`, SAIF counters).

Movement vs 2026-11-18: `dead_annotation_entries` has **no recorded
counterpart** in the old entry — the flat net-name annotation did not
count unresolved dump paths at all (state this; it is the honest
"movement" line: surface added, not count changed).

## Reference numbers (verbatim from the 2026-11-18 / v0.3.0 entries)

| group | OpenSTA internal | OpenSTA switching | ours (2026-11-18) int/sw | ours (v0.3.0) int/sw |
|---|---|---|---|---|
| Sequential | 2.98e-4 | 1.07e-5 | 1.56e-4 / 1.02e-5 | 2.984e-4 / 1.024e-5 |
| Combinational | 5.88e-5 | 8.93e-5 | 1.35e-4 / 4.17e-5 | 5.873e-5 / 8.765e-5 |
| Clock | 0 | 0 | 0 / 0 | 0 / 0 |
| **Total** | **3.57e-4** | **1.00e-4** | **2.91e-4 / 5.20e-5** | **3.572e-4 / 9.789e-5** |

Leakage matched <2% in every group in both runs; total leakage
`1.3004e-9` vs `1.30e-9`. The R1 run re-checks these columns against
the same command — they must not move (the diagnostics surface is
report-only); if any watt column moves, that is a bug in the revision,
not a result.

## What to record in `docs/results.md` (revision block)

- The exact commands run (our CLI both ways; the container Tcl), with
  run/not-run stated for each.
- The `gcd.vcd` provenance paragraph + reproduction recipe +
  run-status.
- The card-question answers (a) and (b), each with its verbatim
  evidence.
- The diagnostics movement lines (above).
- The re-verified three-column table — bit-identical to the v0.3.0
  table expected; diff only if the re-run genuinely moved a column
  (then name it).

## Files

- `docs/results.md` — the revision block inside the AP4 entry
- `tests/designs/gcd/power/gcd.vcd` — input only (untracked artifact,
  seeded); `power_vcd_check.tcl` — reference, unchanged
- `tests/designs/README.md` — gcd `power/` provenance bullet
