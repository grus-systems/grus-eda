# gcd — `report_power` re-comparison protocol

The end-to-end check: the propagated activity model on a real design,
against the OpenSTA numbers already in `docs/results.md`. No new
committed test — gcd is CLI-scale (`no gcd power test exists` is the
standing note); the comparison lands as a results entry.

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

`$PDK` = `$PDK_ROOT/sky130A` or `~/.ciel/sky130A` (the `sky130_hd()`
lookup's host form). Default parasitics = zero-wire — matching the
committed table's configuration (the earlier `--spef`/`--parasitics
placement` variants stay as they were; the re-comparison is on the
zero-wire row only, since the activity model is what changed).

**`gcd.vcd` provenance** (record in the entry): gitignored (`*.vcd`
except microbench), Icarus Verilog output of the committed
`gcd_tb.v` (`$dumpfile("gcd.vcd")`, `$dumpvars(0, dut)` — scope
`gcd_tb/dut`, leaf names resolve to gcd's port pins), ~18 MB, present
in the worktree via `worktree_seed`. Verify `ls
tests/designs/gcd/power/gcd.vcd` first; if it is absent, regenerate
with iverilog (`gcd_tb.v` + the `gcd` netlist) and record the exact
command actually run in the entry — the 2026-11-18 generation flags
were never recorded, so honesty requires naming the reproduction, not
guessing it.

Also record the same command **without `--vcd`** (defaults-only run)
— one extra line in the entry showing what the propagated model does
with no annotation at all. It is *not* a comparison point (OpenSTA's
reference is VCD-seeded); it documents the bare-defaults behavior on a
real design.

## Reference numbers (verbatim from the 2026-11-18 entry)

| group | OpenSTA internal | OpenSTA switching | ours (v0.1.96-era, exact counts) int/sw |
|---|---|---|---|
| Sequential | 2.98e-4 | 1.07e-5 | 1.56e-4 / 1.02e-5 |
| Combinational | 5.88e-5 | 8.93e-5 | 1.35e-4 / 4.17e-5 |
| Clock | 0 | 0 | 0 / 0 |
| **Total** | **3.57e-4** | **1.00e-4** | **2.91e-4 / 5.20e-5** |

Leakage matched <2% in every group then (`cell_leakage_power` sum) and
is expected to be unchanged — activity resolution does not feed
leakage beyond `when` weights.

## What to record in `docs/results.md`

- The exact command run (above), the `activity:` origin-split line it
  prints (gcd's per-net origin counts — the `report_activity_annotation`
  analogue; note `gcd.vcd`'s port annotation count), and the
  diagnostics stderr line (`cells_without_lib_cell`, driven-net
  unresolved count, etc.).
- A three-column table: OpenSTA / ours-then / ours-now, per group and
  column, with deltas on ours-now.
- **Moved-toward / still-diverging split, stated plainly**:
  - Expected moved: Combinational and Sequential *internal* — the
    2026-11-18 root cause ("OpenSTA propagates statistically, ours
    read exact counts") is the thing AP1–AP4 replaced. Sequential
    internal should land near `2.98e-4` — OpenSTA's `findSeqActivity`
    register propagation ≈ our Q=D copy; combinational internal near
    `5.88e-5` if the local-marginal model holds on a real cone.
  - Expected residual: *switching* columns partially — wire-RC is
    still zero-wire (the named secondary cause); M1 local marginals
    drop joint correlation OpenSTA's model may keep; gcd's own
    sequential feedback cones (if any) go `unknown` on both sides but
    OpenSTA's `findSeqActivity` and our `{0,0}` need not agree.
  - Whatever the numbers are, record them — the honest-residuals
    rule: failed or divergent runs stay in the log with the root
    cause named.
- Whether `driven_nets_without_activity` / `dead_annotation_entries`
  moved on gcd vs the old counts (the old run had no pin-namespace
  resolution; scoped `dut/<leaf>` paths now resolve port drivers).

## Cross-check (cheap sanity, optional but recommended)

`grus-eda power --vcd tests/designs/microbench/mb.vcd --json
/tmp/mb.ap4.json` on `mb.def` — the committed `mb.power.json` must
stay byte-identical (it is regenerated anyway per golden.md; this is
the same check from the CLI side).

## Files

- `docs/results.md` — the re-comparison entry
- `tests/designs/gcd/power/gcd.vcd` — input only (untracked artifact,
  seeded); `power_vcd_check.tcl` — reference, unchanged
