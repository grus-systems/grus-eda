# e6-drt-ref — `tests/designs/generate_drt_ref.sh`

New committed script, `<design>` as `$1` (`gcd`, `picorv32`), mirroring
`tests/designs/generate_irdrop_ref.sh`'s structure exactly:
input guards, PDK check, Docker bootstrap (native `docker` else
`mise exec docker-cli@latest colima@latest lima@latest -- docker`),
`docker run --rm -u $(id -u):$(id -g) -v $REPO:/work -v $PDK:/pdk -e
PDK_ROOT=/pdk -e DRT_DESIGN=$DESIGN -w /work/tests/designs/$DESIGN`
re-invoking itself, `set -euo pipefail` throughout.

## Inputs and guards (host phase)

Fail loudly, like the CTS/irdrop `for f` guards:

- `tests/designs/<d>/<d>.placed.def` (gitignored for picorv32 — the
  worktree seed or a prior `generate_ref.sh` supplies it)
- `tests/designs/<d>/<d>.sdc`
- `tests/designs/<d>/runs/guide-check/<d>.ours.guide` — the delegated
  variant's input. If absent, the host phase runs
  `bash tests/designs/generate_guide_ours_spef.sh "${DESIGN}"` first
  (release `cargo build` + `grus-eda route --guide-out` — the
  documented dependency; it also re-emits `<d>.guide.ours.spef`,
  which is deterministic). If the guide is still missing afterward,
  abort with the same error idiom.

PDK check: `${PDK_ROOT:-${HOME}/.ciel}/sky130A` must exist, as in the
irdrop script. PDK paths inside the container are the usual three:

```
PDK_LIB=/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
PDK_TLEF=/pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
PDK_LEF=/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
```

Scratch (transcripts, the baseline guide, pre-copy reports) goes under
`tests/designs/<d>/runs/drt/` — the existing
`tests/designs/<d>/runs` gitignore rule covers it.

## Tcl-on-stdin rules (carry-over)

- `!` is history expansion on stdin — write `== 0` negations.
- `\` does not continue lines — every Tcl command is one physical
  line.
- Marker lines (`DRT-*`) match at `^` in the parser only — the
  transcript echoes `puts` commands behind `openroad>`/`...>` prompts.

## Step A — baseline: `global_route` + `detailed_route`

One `openroad -no_init -no_splash` process, stdout+stderr tee'd to
`runs/drt/baseline.log` (the transcript the parser reads).

```tcl
set D $env(DRT_DESIGN)
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/$D/$D.placed.def
read_sdc /work/tests/designs/$D/$D.sdc

puts "DRT-SURFACE [info commands *route*]"
puts "DRT-SURFACE [info commands *antenna*]"
puts "DRT-SURFACE [info commands *guide*]"
puts "DRT-SURFACE [info commands *wire_length*]"
foreach cmd {global_route read_guides detailed_route check_antennas write_def} {
    if {[llength [info commands $cmd]] == 0} {
        puts "DRT-FATAL missing command: $cmd"
        exit 1
    }
}
# flag-surface probes: bare calls must error, not silently default
puts "DRT-PROBE check_antennas [catch {check_antennas} r]; $r"

set_routing_layers -signal met1-met5 -clock met3-met5
set t0 [clock milliseconds]
global_route -guide_file /work/tests/designs/$D/runs/drt/$D.baseline.guide -congestion_iterations 100
set t1 [clock milliseconds]
detailed_route -output_drc /work/tests/designs/$D/runs/drt/$D.baseline.drc.rpt -verbose 0
set t2 [clock milliseconds]
check_antennas -report_file /work/tests/designs/$D/runs/drt/$D.baseline.antenna.rpt
set t3 [clock milliseconds]
write_def /work/tests/designs/$D/$D.drt.baseline.def
puts "DRT-TIME global_route_ms [expr {$t1 - $t0}]"
puts "DRT-TIME detailed_route_ms [expr {$t2 - $t1}]"
puts "DRT-TIME check_antennas_ms [expr {$t3 - $t2}]"
exit
```

- The routing prelude is the verbatim step-5b spelling — no
  `set_global_routing_layer_adjustment` (that is step 5c's guide-golden
  variant; see `overview.md`).
- `check_antennas`'s report flag is the one open surface item. The
  `-report_file` spelling is what OpenROAD's antenna checker exposes in
  the image's own flow; if it errors (`STA-0562`-style or a flag
  complaint), the script's committed form is whatever the probed
  surface actually is — `check_antennas -verbose`, `check_antennas
  -report_file <f>`, or bare `check_antennas` with the count taken from
  the log. The step-C parser asserts on the `DRT-PROBE` line and keeps
  every antenna-related log line verbatim; whichever call form ran is
  recorded in the JSON (`antenna_check_command`). Do not guess the
  grammar — write the regexes against the actual transcript.
- `write_def` emits `tests/designs/<d>/<d>.drt.baseline.def` in the
  design dir (gitignored — mirrors the `picorv32.routed.def`
  convention; DEFs are the ~200 KB–7 MB range).
- After the process: `grep -q "^DRT-FATAL" baseline.log` → abort (the
  `IRD-FATAL` guard pattern; anchor at `^` because the transcript
  echoes the `puts` lines themselves).

## Step B — delegated: `read_guides` + `detailed_route`

A second, clean-state process, tee'd to `runs/drt/delegated.log`.
Identical prelude (liberty/lef/def/sdc, surface probe, DRT-PROBE line,
`set_routing_layers`), then:

```tcl
set t0 [clock milliseconds]
read_guides /work/tests/designs/$D/runs/guide-check/$D.ours.guide
set t1 [clock milliseconds]
detailed_route -output_drc /work/tests/designs/$D/runs/drt/$D.delegated.drc.rpt -verbose 0
set t2 [clock milliseconds]
check_antennas -report_file /work/tests/designs/$D/runs/drt/$D.delegated.antenna.rpt
set t3 [clock milliseconds]
write_def /work/tests/designs/$D/$D.drt.delegated.def
puts "DRT-TIME read_guides_ms [expr {$t1 - $t0}]"
puts "DRT-TIME detailed_route_ms [expr {$t2 - $t1}]"
puts "DRT-TIME check_antennas_ms [expr {$t3 - $t2}]"
exit
```

- `set_routing_layers` stays in the delegated prelude: the layer window
  on both variants must be identical for the delta to mean "guide
  provenance" rather than "routing setup".
- `set -e` does **not** rescue the script if `read_guides` or
  `detailed_route` aborts the process — the openroad exit status then
  fails the run, which is correct: a delegated run that cannot even
  execute is a hard failure worth stopping on (the acceptance's
  "nonzero DRC or fails" clause refers to routed-quality outcomes —
  violations, antenna hits, zero-length results — which a completed
  transcript records; an unclean exit still gets its partial
  transcript checked by the step-C parser before anything is called
  data). If `detailed_route` returns control but reports nonzero DRC,
  the run continues — that is data.
- The `check_guide_readback.sh` precedent (`set_wire_rc -layer met2`
  before `read_guides`) applies to `estimate_parasitics`, not to
  `detailed_route` — no `set_wire_rc` here (none in step 5b either).

## Step C — assemble `<d>.drt.reference.json` (python3 heredoc)

Mirrors the irdrop step-D shape: every unmatched expectation aborts,
no default-fill. Inputs: `runs/drt/{baseline,delegated}.log`, the four
reports, the two routed DEFs, `runs/drt/<d>.baseline.guide`,
`runs/guide-check/<d>.ours.guide`, and (when present) the prior
`<d>.routed.def`. Schema in `golden.md`; the parser's jobs:

1. **Surface checks**: every `DRT-SURFACE` line exists; the required
   five commands appear in the collected command set; each `DRT-PROBE`
   reports `catch` 1 (a `check_antennas` bare call that exits clean is
   itself a recorded fact — keep the result verbatim rather than
   failing, but record `probe_code` so a silently-defaulting surface is
   visible).
2. **Marker checks**: `DRT-TIME` lines present in both logs
   (`global_route_ms` on baseline, `read_guides_ms` on delegated).
3. **DRC**: `drc_violations` = non-blank line count of
   `runs/drt/<d>.<variant>.drc.rpt` (empty file = 0, the
   `generate_ref.sh` step-5b convention). If the transcript carries a
   final DRT violation tally (`DRT-` info line — grammar pinned against
   the actual log), record it in `drt_summary_lines` and cross-check
   equal; a mismatch between the report file and the log tally is
   recorded, not reconciled.
4. **Antennas**: `antenna_violations` from the pinned report/log
   grammar (violation-line count in the `-report_file` output, else the
   log's reported count — whichever the probed surface produced).
   `antenna_log_lines` keeps the matching lines verbatim.
5. **Routed-DEF metrics** (see `golden.md` §"DEF-derived metrics" for
   the arithmetic): `signal_nets_routed`, `total_wirelength_um`,
   `vias_per_cut_layer` (keyed by via-def name), `via_count_total`.
6. **SHA/size stamps**: routed DEFs (both), baseline guide, ours guide,
   reports — `sha256(...).hexdigest()[:16]` + `size_bytes`, the
   irdrop convention.
7. **Prior-routed cross-check**: if `tests/designs/<d>/<d>.routed.def`
   exists, `prior_routed_def` records its SHA-256 prefix + size and
   `baseline_def_identical` (byte compare); absent → `null` with
   `prior_routed_def_present: false` (picorv32's is gitignored and may
   not exist in every worktree — that is fine, it is a cross-check, not
   an input).
8. **Report commit decision**: per report, if `size_bytes < 1_000_000`
   copy `runs/drt/<d>.<variant>.drc.rpt` →
   `tests/designs/<d>/<d>.drc.<variant>.rpt` (and the antenna twin) and
   mark `committed: true`; else leave under `runs/drt/` and mark
   `committed: false` with `file` pointing at the runs path.
9. **Delta block**: `delegated − baseline` for `drc_violations`,
   `antenna_violations`, `total_wirelength_um` (absolute + pct),
   `via_count_total`, per-cut-layer via counts, `total_route_s` —
   computed in the same python step from the two measured blocks.
10. Emit `<d>.drt.reference.json` (`json.dump(..., indent=2)` +
    trailing newline, the repo convention). Verify the file exists;
    print a `DRT-SUMMARY` line per variant (drc, antenna, wirelength,
    vias, seconds).

## Line-per-command note

The long `global_route`/`detailed_route`/`check_antennas` calls are
single physical lines — same rule as
`clock_tree_synthesis`/`set_pdnsim_inst_power` in the sibling scripts.
