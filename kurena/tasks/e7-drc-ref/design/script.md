# e7-drc-ref — `tests/designs/generate_drc_ref.sh`

New committed script, `$1 = <design>`, `$2 = <routed-def>` (required;
repo-relative path such as `tests/designs/gcd/gcd.routed.def`, or a
bare filename resolved under `tests/designs/<d>/`). Structure mirrors
`generate_irdrop_ref.sh` exactly: input guards, PDK check, Docker
bootstrap (native `docker` else `mise exec docker-cli@latest
colima@latest lima@latest -- docker`), `docker run --rm -u
$(id -u):$(id -g) -v $REPO:/work -v $PDK:/pdk -e PDK_ROOT=/pdk -e
DRC_DESIGN=$DESIGN -e DRC_DEF=<resolved path> -w /work/tests/designs/$DESIGN`
re-invoking itself, `set -euo pipefail` throughout.

Header comment states: what it measures (Magic `drc(full)` + DRT
report + `check_antennas` on one routed DEF, plus the scripted
broken-DEF sanity control and the tech-file rule taxonomy), that no
LibreLane flow is re-run on the golden path (only the optional
DRT-report regeneration routes), the input-def preference order, and
`Usage: generate_drc_ref.sh <design> <routed-def>`.

## Inputs and guards (host side, before docker run)

- `<routed-def>` — required arg; resolve to
  `tests/designs/<d>/<file>` if bare, else a repo-relative path.
  Fail loudly if missing. For picorv32 the file is gitignored — the
  error names `tests/designs/picorv32/generate_ref.sh` as producer
  (step 7 routes).
- `tests/designs/<d>/<d>.placed.def` + `<d>.sdc` — required **only**
  when no DRT report is found and regeneration kicks in (step B).
- `${PDK_HOST_ROOT}/sky130A` — same guard as the other scripts.

Suggested default (documented in the header): callers pass
`tests/designs/<d>/<d>.drt.routed.def` when `e6-drt-ref` has landed,
else `tests/designs/<d>/<d>.routed.def`. The script does not
auto-select: `$2` is required so the checked DEF is never implicit.

Scratch: `tests/designs/<d>/runs/drc/` (covered by the existing
`tests/designs/<d>/runs` gitignore rule). All Magic/OpenROAD
transcripts, the broken DEF, the regen DEF/report, and any `.mag`
scratch Magic writes live here — Magic steps run with cwd inside
`runs/drc/` (`(cd "$DRC_RUN_DIR" && magic ...)` + absolute `/work`
paths) so `def read`/`load` byproducts never land in the design dir.

PDK paths:

```
PDK_LIB=/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
PDK_TLEF=/pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
PDK_LEF=/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
MAGICRC=/pdk/sky130A/libs.tech/magic/sky130A.magicrc
MAGICTECH=/pdk/sky130A/libs.tech/magic/sky130A.tech
```

## Tcl-on-stdin rules (carry-over)

- `!` is history expansion on stdin — write `== 0` negations.
- `\` does not continue lines on stdin — every Tcl command is one
  physical line.
- Transcript echoes can contain marker text inside `puts` commands;
  every parser regex anchors at `^` (the `IRD-` precedent) — Magic
  markers are `DRCREF-`, OpenROAD markers `DRC-`.

## Step A — antenna surface (openroad process 1)

One `openroad -no_init -no_splash` process, transcript tee'd to
`runs/drc/antenna.log`. Minimal reads first: tlef + cell lef + the
input DEF. Contingency: if `check_antennas` errors naming a missing
liberty/tech input, add `read_liberty` (and `read_sdc` only if the
surface demands it) and record the deviation in `docs/results.md`.

```tcl
set D $env(DRC_DESIGN)
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/<resolved-def-path>

puts "DRC-ANT-SURFACE [info commands *antenna*]"
if {[llength [info commands check_antennas]] == 0} {
    puts "DRC-FATAL missing command: check_antennas"
    exit 1
}
set acode [catch {check_antennas -verbose} ares]
puts "DRC-ANT-RESULT code=$acode result=$ares"
```

If `check_antennas -verbose` errors with an unrecognized-flag class
error (grammar pinned on first run), run bare `check_antennas` next in
the same process and record `verbose_flag: rejected` — both attempts'
output stays in the transcript. Antenna metrics go in the JSON: the
surface list, the verbatim command, `code`, the Tcl return/result, and
the parsed violation count (`antenna_violations`) + per-net detail
lines if any (`antenna_per_net`, else `antenna_report_lines` verbatim —
routed DEFs come from `detailed_route` without `repair_antennas`, so a
nonzero count is real data, not a blocker).

## Step B — DRT report discovery or regeneration (openroad process 2, conditional)

Search order (first existing wins, `drt_report_source: "existing"`):

1. `runs/drt/` e6 reports matching the input DEF variant — glob
   `tests/designs/<d>/runs/drt/*.drc*.rpt` and
   `tests/designs/<d>/<d>.drc.*.rpt`; when the input DEF is
   `*.drt.routed.def` prefer the baseline variant's report
   (`*baseline*` or the file e6 names in `<d>.drt.reference.json`).
2. `tests/designs/<d>/<d>.drc.rpt` — the `generate_ref.sh` step-5b
   report (pairs with `<d>.routed.def` inputs).

If none found: **regenerate** in `runs/drc/` scratch via the exact
step-5b command sequence (reads `<d>.placed.def` + `<d>.sdc` — hence
the conditional guard):

```tcl
read_liberty ...; read_lef (tlef); read_lef (lef)
read_def /work/tests/designs/$D/$D.placed.def
read_sdc /work/tests/designs/$D/$D.sdc
set_routing_layers -signal met1-met5 -clock met3-met5
global_route -guide_file /work/tests/designs/$D/runs/drc/$D.regen.guide -congestion_iterations 100
detailed_route -output_drc /work/tests/designs/$D/runs/drc/$D.drc.regen.rpt -verbose 0
write_def /work/tests/designs/$D/runs/drc/$D.routed.regen.def
exit
```

Step 5b sets **no** `set_global_routing_layer_adjustment` (the
met1 0.30 / met2–met5 0.20 adjustments belong to the step-5c guide
golden, a different process) — mirror 5b verbatim, adjustments
included only if a future `generate_ref.sh` adds them there. Then
compare `sha256(<d>.routed.regen.def)` to the
input DEF's sha: identical → `drt_report_source:
"regenerated_identical"`; different → `"regenerated_divergent"` with
both SHAs recorded (divergent means the report describes a different
route — honest data, recorded, not smoothed). A non-empty regen report
is also data: record `violation_lines` verbatim (the step-5b scripts
abort on nonempty, so expect 0 lines; a nonzero count says routing or
the checker changed).

`drt_drc_count` = line count of the discovered/regenerated report
(`wc -l`; step-5b treats `-s` empty as clean), plus
`drt_report_sha256_16` and `drt_report_file` (repo-relative). If
`<d>.drt.reference.json` exists, also read its baseline
`drc_violations` into `drt_reference_json_violations` and flag
divergence in `docs/results.md` (record both, never assert away).

## Step C — Magic clean run (process 3)

`runs/drc/magic_clean.log` tee'd; cwd inside `runs/drc/`:

```bash
(cd "$DRC_RUN_DIR" && magic -dnull -noconsole -rcfile "$MAGICRC" << 'EOF' | tee magic_clean.log
... Tcl sequence ...
EOF
)
```

Tcl sequence — the P-DRC sequence verbatim, wrapped in markers:

```tcl
puts "DRCREF-VERSION-BEGIN"
catch {exec magic --version} vres
puts "DRCREF-VERSION $vres"
lef read /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
lef read /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
def read /work/<resolved-def-path>
load <DESIGN>
select top cell
drc euclidean on
drc style drc(full)
drc check
puts "DRCREF-COUNT-BEGIN"
drc count
drc count total
puts "DRCREF-WHY-BEGIN"
set whys [drc listall why]
foreach w $whys { puts "DRCREF-WHY {$w}" }
puts "DRCREF-COUNT-TOTAL [llength $whys]"
puts "DRCREF-END"
quit -force
```

Notes for the implementer — grammar is pinned from the actual
transcript, not assumed:

- `load <DESIGN>`: the DEF's `DESIGN` name. For our files it equals
  the filename stem; extract `DESIGN <name> ;` from the DEF in bash
  (host side or in-container) once, pass it to both Magic runs via env
  (`DRC_TOP`) — do not hardcode `$D`.
- `drc count total` vs `drc count`: P-DRC used `drc count total`;
  capture both lines verbatim. If `drc listall why` returns empty and
  `drc count total` is nonzero (or vice versa), that is a surface
  finding — record verbatim, abort the assembly.
- If `[drc listall why]` returns a Tcl list (as in OpenLane's
  `run_magic_drc.tcl`), the `llength` + itemized `DRCREF-WHY` lines are
  the machine-readable capture; if it prints instead of returning, the
  marker-bracketed transcript lines are parsed — whichever it does, the
  parser asserts the two agree.
- `exec magic --version` inside Tcl may fail (already inside magic);
  fallback grammar: the startup banner line (`Magic version ...` /
  `Magic X.Y revision Z`) — record `magic_version_source` verbatim
  (`version_cmd` / `banner`). Whichever path, `magic_version` is the
  verbatim string (expected `8.3.623`).
- `quit -force` (or `exit` — whichever the image accepts; EOF alone
  may leave Magic waiting). Record what worked.

## Step D — mutation + Magic broken run (python3 + magic process 4)

A `python3` heredoc performs the `add_spacing_probe_wire` mutation
(`sanity.md` for the algorithm): parse the input DEF's `NETS` section,
choose the first signal net with a horizontal `+ ROUTED`/`+ NEW`
`met2` segment (fallback layer order `met2 → met1 → met3`), and append
`+ NEW <layer> ( x1 y+W+gap ) ( x2 y+W+gap )` to a different signal
net at a sub-minimum edge gap from that wire. Writes
`runs/drc/<d>.drc.broken.def`; prints a `DRC-MUTATION` summary line
(net_a, net_b, layer, gap_um, segment_dbu). Layer width/spacing
constants are parsed from the tech LEF (`LAYER met2` `WIDTH` /
`SPACING`) — not hardcoded.

Then a second `magic` process, identical Tcl sequence, `def read` of
the broken DEF, transcript `runs/drc/magic_broken.log`. The parser
requires `magic_total > 0` and a nonempty `DRCREF-WHY` set — if the
broken DEF still reads 0, the sanity control failed and the script
aborts (that is exactly the "surface can't see violations" case it
exists to catch).

## Step E — assembly (python3 heredoc)

Reads: `antenna.log`, `magic_clean.log`, `magic_broken.log`, the
input DEF (for `COMPONENTS`/`DESIGN` and the sha), the DRT report
file, and `$MAGICTECH` (the `drc` section for `rules_checked` —
`taxonomy.md`).

Emits `tests/designs/<d>/<d>.drc.reference.json` (schema in
`golden.md`). Every unmatched expectation aborts — no default-fill.
Prints a one-line summary (magic total, drt count, antenna count,
broken total, top violated rule).

## Failure modes — all loud

- Missing routed DEF / PDK → guard exit 1.
- Missing `check_antennas` → `DRC-FATAL` + exit 1 inside Tcl.
- Missing DRT report AND missing `<d>.placed.def`/`<d>.sdc` for regen
  → exit 1 naming `generate_ref.sh`.
- Magic transcript lacks `DRCREF-END` (crash/hang) → exit 1.
- Clean-run count/listall disagreement, or any unparsed grammar →
  python `sys.exit`.
- Broken run reports 0 errors or empty why-set → abort (the control
  failed).
- Tech-file `drc` section yields zero rule names → abort (extraction
  grammar drifted).

## Determinism

Magic DRC and `check_antennas` are deterministic on identical inputs.
The JSON carries no dates; `runtime_s` (per-step seconds, from shell
`SECONDS`) is the named varying field. The step-5b regen path is
deterministic (proven by `regenerated_identical` when it fires).
Rerun-twice `git diff` is the acceptance check — record the rerun
count and any residual varying field in `docs/results.md`.
