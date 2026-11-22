# e8-irdrop-ref — `tests/designs/generate_irdrop_ref.sh`

New committed script, `<design>` as `$1`, mirroring
`tests/designs/generate_cts_ref.sh`'s structure exactly: input guards,
PDK check, Docker bootstrap (native `docker` else
`mise exec docker-cli@latest colima@latest lima@latest -- docker`),
`docker run --rm -u $(id -u):$(id -g) -v $REPO:/work -v $PDK:/pdk -e
PDK_ROOT=/pdk -e IRD_DESIGN=$DESIGN -w /work/tests/designs/$DESIGN`
re-invoking itself, `set -euo pipefail` throughout.

## Inputs and guards

Required (fail loudly, like the CTS script's `for f` guard):

- `tests/designs/<d>/<d>.placed.def` (gitignored for picorv32 — the
  worktree seed or a prior `generate_ref.sh` supplies it)
- `tests/designs/<d>/<d>.sdc` — needed by `report_power` (clock
  activities) and to match the other flows' read set.

Scratch goes under `tests/designs/<d>/runs/irdrop/` (new directory;
the existing `tests/designs/<d>/runs` gitignore rule covers it).

PDK paths are the usual three:

```
PDK_LIB=/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
PDK_TLEF=/pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
PDK_LEF=/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
```

## Tcl-on-stdin rules (carry-over)

- `!` is history expansion on stdin — write `== 0` negations.
- `\` does not continue lines on stdin — every Tcl command is one
  physical line (the `clock_tree_synthesis`/`set_pdnsim_inst_power`
  calls included).

## Step A — connectivity + embedded convention + per-instance power report

One `openroad -no_init -no_splash` process, stdout+stderr tee'd to
`runs/irdrop/irdrop_a.log` (the transcript the parser reads).

```tcl
set D $env(IRD_DESIGN)
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/$D/$D.placed.def
read_sdc /work/tests/designs/$D/$D.sdc
```

### A0 — command-surface probe

Still inside step A, before any analysis:

```tcl
puts "IRD-SURFACE [info commands *power_grid*]"
puts "IRD-SURFACE [info commands *pdnsim*]"
```

Then verify the three required commands exist and probe the flag
surfaces by executing them — the image has no `-help` surface
(`STA-0562`), so the probe is a `catch` on an intentionally malformed
call whose error text is recorded verbatim in the transcript:

```tcl
foreach cmd {check_power_grid analyze_power_grid set_pdnsim_inst_power} {
    if {[llength [info commands $cmd]] == 0} {
        puts "IRD-FATAL missing command: $cmd"
        exit 1
    }
}
# flag-surface probe: a -net-less / key-less call must error, not default
puts "IRD-PROBE [catch {set_pdnsim_inst_power} r]; $r"
puts "IRD-PROBE [catch {analyze_power_grid} r]; $r"
puts "IRD-PROBE [catch {check_power_grid} r]; $r"
```

The step-B parser asserts each `IRD-PROBE` line reports an error
(`catch` result 1) naming the missing argument — an unmatched surface
(i.e. a bare call succeeding or a different command existing) aborts
the script, it never defaults.

### A1 — connectivity (verbatim verdict)

```tcl
catch {check_power_grid -net VPWR} conn_vpwr
puts "IRD-CONN VPWR err=$conn_vpwr"
catch {check_power_grid -net VGND} conn_vgnd
puts "IRD-CONN VGND err=$conn_vgnd"
```

The `PSM-0040`-class verdict lines print to `irdrop_a.log` inside the
command on success; the `catch` keeps a failed check (`PSM-0069`-class
error) from aborting the process — a connectivity failure is a real
finding recorded via the `IRD-CONN` line, not a script failure. The
parser extracts both the verdict text and the `IRD-CONN` error field
verbatim into the golden.

### A2 — per-instance power report (the convention-(b) feed)

```tcl
report_power -digits 6
report_power -instances [get_cells *] -digits 12 > /work/tests/designs/$D/runs/irdrop/inst_power.rpt
set inst_count [llength [get_cells *]]
set def_components [llength [[ord::get_db_block] getInsts]]
set fp [open "/work/tests/designs/$D/runs/irdrop/a_scalars.tcl" "w"]
puts $fp "set inst_count $inst_count"
puts $fp "set def_components $def_components"
close $fp
```

`report_power -instances` supports the `> file` redirect (the
`proc_redirect` surface). The design summary is printed to the
transcript as a cross-check on the report's own total. If the
redirected file is absent or the table shape is unrecognized, step B
aborts — no default-fill.

Contingency: the full `[get_cells *]` list includes the physical cells
(tap/endcap) that have no Liberty arcs; whether `instance_power`
tolerates them is unprobed. If `report_power -instances` errors on
them, narrow the feed to Liberty-mapped cells (e.g. a Tcl filter on
each cell's master/ref) and record the filter — the skipped set, its
count, and why — in `inst_power_feed`. A narrower-but-complete feed is
a valid measurement; a silently truncated one is not.

### A3 — embedded convention

```tcl
analyze_power_grid -net VPWR -voltage_file /work/tests/designs/$D/runs/irdrop/vpwr.embedded.csv
analyze_power_grid -net VGND -voltage_file /work/tests/designs/$D/runs/irdrop/vgnd.embedded.csv
exit
```

No `set_pdnsim_net_voltage` — P-IRD measured the default supply (1.80 V
on VPWR) and the golden records the reported supply verbatim, whatever
it is. No `-corner`, no `-vsrc`, no `-source_type` (defaults, matching
P-IRD). If the `VGND` run needs an explicit 0 V supply, that surfaces
as a reported-supply anomaly and is handled by recording it — not by
silently adding flags (the `results.md` entry names any deviation).

## Step B — transcript parse + inst-power Tcl emission

`python3` heredoc inside the same container run (the `*DATE`-fix
precedent; `sed` is absent from the image) reads:

- `runs/irdrop/irdrop_a.log` — `IRD-SURFACE`/`IRD-PROBE` lines,
  connectivity verdicts, both `analyze_power_grid` report blocks;
- `runs/irdrop/inst_power.rpt` — the per-instance table;
- `runs/irdrop/a_scalars.tcl` — `inst_count`;
- `tests/designs/<d>/<d>.placed.def` — the `COMPONENTS` name set;

and does:

1. Assert the probe lines all show error-1 catches naming missing
   arguments; assert the command surface contains the three commands.
2. Extract the connectivity verdict lines for VPWR and VGND verbatim.
3. Parse each `analyze_power_grid` report block into the metric set in
   `golden.md`; every unmatched expectation is a script failure.
4. Parse `inst_power.rpt` rows (`inst_name internal switching leakage
   total`); every row must be a name in the DEF `COMPONENTS` name set
   plus 4 parseable floats. Three counts go into the golden:
   `def_components` (odb insts), `inst_count` (`llength [get_cells *]`
   — the list fed to `-instances`), `report_rows` (parsed rows).
   `report_rows == inst_count` is asserted — a report that silently
   drops fed instances aborts the script. `inst_count` vs
   `def_components` is recorded, not asserted: whether `get_cells`
   covers the 151 physical cells (tap/endcap, no Liberty arcs) is an
   empirical question the first run answers, and the golden carries
   whichever truth it finds. A name present in the report but absent
   from `COMPONENTS` (name-format mismatch) aborts before process B
   runs — that is exactly the `findInst` edge the cross-check exists
   to catch early.
5. Emit `runs/irdrop/inst_power.tcl`: one line per row,
   `set_pdnsim_inst_power -inst {<name>} -power <total_watts>` — braces
   on the name (DEF names may carry `[`/`]` escapes that would eval-
   substitute unbraced), the **Total** column in watts.

## Step C — explicit convention (second process, clean state)

A **separate** `openroad -no_init -no_splash` process, tee'd to
`runs/irdrop/irdrop_b.log`:

```tcl
set D $env(IRD_DESIGN)
read_liberty ...; read_lef ...; read_lef ...
read_def /work/tests/designs/$D/$D.placed.def
read_sdc /work/tests/designs/$D/$D.sdc
source /work/tests/designs/$D/runs/irdrop/inst_power.tcl
analyze_power_grid -net VPWR -voltage_file /work/tests/designs/$D/runs/irdrop/vpwr.sta.csv
analyze_power_grid -net VGND -voltage_file /work/tests/designs/$D/runs/irdrop/vgnd.sta.csv
exit
```

Rationale for the second process: `analyze_power_grid` carries an
`-allow_reuse` flag, so solution state survives within a process;
running convention (b) in a fresh process keeps the two measurements
independent and matches the clean-parasitics precedent.

## Step D — golden assembly + CSV naming

A second `python3` heredoc:

1. Parses `irdrop_b.log` into the same per-net metric set.
2. Compares each net's embedded vs explicit voltage CSV byte-for-byte.
   Naming rule (per `goal.md`): if the two conventions produce a
   byte-identical CSV for a net, commit one file
   `<d>.irdrop.voltage.<net>.csv` and record
   `"conventions_share_voltage_map": true`; otherwise commit
   `<d>.irdrop.voltage.<net>.embedded.csv` and
   `<d>.irdrop.voltage.<net>.sta.csv` (lowercase net name in the file
   name: `vpwr`, `vgnd`). Copy the chosen files from `runs/irdrop/`
   into `tests/designs/<d>/`.
3. Computes per-file size and SHA-256 (first 16 hex) of every emitted
   CSV, and the row count (lines minus the header).
4. Writes `<d>.irdrop.reference.json` (schema in `golden.md`), then
   prints a per-net summary line (worst drop/bounce both conventions +
   delta).

## Failure modes — all loud

- Missing input DEF/SDC → guard exit 1 (script header).
- Missing command (`analyze_power_grid` etc.) → `IRD-FATAL` + exit 1
  inside Tcl.
- Probe shows an unexpected parser surface → step-B assert fails.
- `check_power_grid` reports unconnected shapes → the verdict is
  recorded verbatim and the script continues (a real finding, not a
  script failure); the golden carries it.
- Any unparsed report field, missing CSV, CSV row-count anomaly, or
  `inst_power.rpt` row/name mismatch → python `sys.exit`.
- `set_pdnsim_inst_power` error inside step C (e.g. `findInst` miss on
  a name-format edge) → Tcl aborts; investigate, don't catch.

## Determinism

The solver is deterministic given identical inputs; the acceptance
requires a rerun to byte-reproduce every committed artifact. The
script itself does nothing nondeterministic (no timestamps in emitted
files — CSVs are tool output, the JSON carries no date). The rerun
count and any residual diff are recorded in `docs/results.md`.
