# e5-cts-ref — `tests/designs/generate_cts_ref.sh`

New committed script, `<design>` as `$1`, mirroring
`tests/designs/generate_prop_ref.sh`'s structure exactly: input guards,
PDK check, Docker bootstrap (native `docker` else
`mise exec docker-cli@latest colima@latest lima@latest -- docker`),
`docker run --rm -u $(id -u):$(id -g) -v $REPO:/work -v $PDK:/pdk -e
PDK_ROOT=/pdk -e CTS_DESIGN=$DESIGN -w /work/tests/designs/$DESIGN`
re-invoking itself, `set -euo pipefail` throughout.

## Inputs and guards

Required (fail loudly, like the prop script's `for f` guard):

- `tests/designs/<d>/<d>.placed.def` (gitignored for picorv32 — the
  worktree seed or a prior `generate_ref.sh` supplies it)
- `tests/designs/<d>/<d>.sdc` — the CTS run's constraints
- `tests/designs/<d>/<d>.cts.sdc` — the propagated STA golden's
  constraints (`sta-golden.md`)

Scratch goes under `tests/designs/<d>/runs/cts/` (new directory; the
existing `tests/designs/<d>/runs` gitignore rule covers it). Do not
reuse `runs/canonical/` — that tree belongs to the flow script.

PDK paths are the usual three:

```
PDK_LIB=/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
PDK_TLEF=/pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
PDK_LEF=/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
```

## Step A — TritonCTS + report surface + census

One `openroad -no_init -no_splash` process, stdout+stderr tee'd to
`runs/cts/cts.log` (the transcript the metrics parser reads; the
`guide_ours_readback` tee precedent). Tcl on stdin, so `!` is history
expansion — write `== 0`, and keep each Tcl command on one line (the
P-GR finding: `\` does not continue lines on stdin).

```tcl
set D $env(CTS_DESIGN)
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/$D/$D.placed.def
read_sdc /work/tests/designs/$D/$D.sdc

# snapshot pre-CTS instance names for the inserted-instance census
set block [ord::get_db_block]
foreach inst [$block getInsts] { set pre([$inst getName]) 1 }

clock_tree_synthesis -root_buf sky130_fd_sc_hd__clkbuf_16 \
    -buf_list "sky130_fd_sc_hd__clkbuf_8 sky130_fd_sc_hd__clkbuf_4 sky130_fd_sc_hd__clkbuf_2" \
    -wire_unit 20
repair_clock_nets
set_wire_rc -layer met2          ;# required before estimate_parasitics (EST-0018)
estimate_parasitics -placement

report_cts_config
report_cts
report_clock_skew -setup
report_clock_skew -hold
worst_clock_skew -setup
worst_clock_skew -hold
```

The `clock_tree_synthesis` line is written as ONE line in the committed
script (the `\`-continuation above is display-only — stdin continuation
does not work). The buffer list is the P-CTS-measured sky130hd flow
default (`/pdk/.../config.tcl` + `cts.tcl` in the image); do not call
`report_clock_tree` — it does not exist in this image.

Then, still in that process, the census Tcl:

```tcl
# per-inserted-instance census -> runs/cts/cts_census.txt
set fp [open "/work/tests/designs/$D/runs/cts/cts_census.txt" "w"]
foreach inst [$block getInsts] {
    if {[info exists pre([$inst getName])] == 0} {
        set master [[$inst getMaster] getName]
        # output-pin net fanout: iterms on the net of the inst's output pin
        # (the dummy-load classifier — see metrics.md)
        ...
        puts $fp "[$inst getName] $master <out_fanout>"
    }
}
close $fp
# scalars -> runs/cts/cts_scalars.tcl  (set def_components N, set def_signal_nets N, ...)
write_def /work/tests/designs/$D/$D.cts.def
```

`write_def` runs inside the same process so the committed DEF is
exactly the post-CTS/repair state the reports describe.

## Step B — metrics JSON assembly

`python3` heredoc inside the same container run (the `*DATE`-fix
precedent; `sed` is absent from the image) reads:

- `runs/cts/cts.log` — parses the `report_cts` table, the two
  `report_clock_skew` tables, the `worst_clock_skew` lines, and the
  `report_cts_config` block;
- `runs/cts/cts_census.txt` + `cts_scalars.tcl` — DEF-side truth;

and emits `<d>.cts.reference.json` (field-by-field source and shape in
`metrics.md`). Parser failures are script failures — unmatched
expectations abort, no default-fill.

## Step C — propagated STA golden

A **separate** `openroad -no_init -no_splash` process (the 9b/9c
clean-parasitics precedent):

```tcl
source /work/tests/designs/$D/runs/cts/cts_collect.tcl
read_liberty ...; read_lef ...; read_lef ...
read_def /work/tests/designs/$D/$D.cts.def
read_sdc /work/tests/designs/$D/$D.cts.sdc
set_wire_rc -layer met2
estimate_parasitics -placement
# sta::worst_slack/-min, sta::total_negative_slack (user units),
# collect_endpoints_json, collect_clock_pins_json
# -> <d>.cts.sta.reference.json, block "cts_placement_rc"
```

`runs/cts/cts_collect.tcl` is emitted by a heredoc in the script and
carries `collect_endpoints_json` and `collect_clock_pins_json` copied
verbatim from `tests/designs/generate_prop_ref.sh` (the established
duplication — `collect_endpoints.tcl` lives in `runs/canonical` and is
not guaranteed present). `-group_path_count` is `100000` (the prop
script's value; covers picorv32's 1,798 endpoints).

## Determinism

Acceptance requires a rerun to byte-reproduce the committed artifacts:

- TritonCTS is deterministic on this input; verify by running the
  script twice and checking `git diff` is empty on all three artifacts.
- If `write_def` embeds a nondeterministic header (it is not expected
  to — `gcd.placed.def`/`gcd.routed.def` are byte-stable), normalize
  in-script like the SPEF `*DATE` fix and record the normalization in
  `docs/results.md`.
- `.cts.sdc` is committed input, not generated — it never changes.
