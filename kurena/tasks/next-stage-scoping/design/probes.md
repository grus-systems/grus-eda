# Probes

Small reference-tool probes run inside `ghcr.io/librelane/librelane:3.0.16`
(same invocation pattern as `tests/designs/*/generate_ref.sh`: `-v
$REPO:/work -v $PDK_ROOT:/pdk`, `-w` the design dir). Each probe answers one
question the criteria cannot set from the desk. Every run — including a
command that turns out not to exist — is recorded in `docs/results.md` under
a dated `next-stage-scoping` entry with the command and the relevant output
lines. Scratch files go under `tests/designs/gcd/runs/` (gitignored).

Tcl goes to `openroad -no_init -no_splash` on stdin or a script file;
remember `!` is history expansion on stdin (AGENTS.md §7.3 — write `== 0`).

`PDK_LIB`/`PDK_TLEF`/`PDK_LEF` are the usual three paths under
`/pdk/sky130A/libs.ref/sky130_fd_sc_hd/`.

---

## P-GR — guide format, guide parasitics, guide readback

**Questions**: (a) exact `*.guide` text format; (b) does
`estimate_parasitics -global_routing` exist and what does its SPEF look like
vs the placement (`gcd.routed.spef`) and routed (`gcd.rcx.spef`) SPEFs —
this bounds criterion E4-3/4; (c) can OpenROAD read a guide file back
(`read_guides`, `global_route` flag, `odb` accessor) — decides whether "our
guides into the reference's own parasitic estimator" is available as the
cleanest possible comparison.

```tcl
read_liberty $env(PDK_LIB)
read_lef $env(PDK_TLEF)
read_lef $env(PDK_LEF)
read_def /work/tests/designs/gcd/gcd.placed.def
read_sdc /work/tests/designs/gcd/gcd.sdc
set_routing_layers -signal met1-met5 -clock met3-met5
set_global_routing_layer_adjustment met1 0.30
set_global_routing_layer_adjustment met2-met5 0.20
global_route -guide_file /work/tests/designs/gcd/runs/canonical/gcd.probe.guide -congestion_iterations 100
estimate_parasitics -global_routing -spef_file /work/tests/designs/gcd/runs/canonical/gcd.probe.guide.spef
puts "guide-cmds: [info commands *guide*]"
puts "ep-flags: [catch {estimate_parasitics -help} msg]; $msg"
```

Then on the host: `head -60 gcd.probe.guide`; count guide boxes/nets; diff
per-net `*D_NET` total C between the guide SPEF and `gcd.routed.spef` /
`gcd.rcx.spef` (a few awk lines; record the median/max per-net delta as the
provisional E4-4 anchor).

**Decides**: E4 criteria 1–4 (coverage math, wirelength spread, parasitic
isolation feasibility), whether a guide reader/writer is round-trippable.

## P-CTS — TritonCTS surface and gcd clock numbers

**Questions**: flag surface of `clock_tree_synthesis` in this image, the
report commands that carry skew/insertion delay, and the actual magnitude of
skew/insertion delay/buffer count on gcd — sets the E5 tolerances.

```tcl
puts "cts-cmds: [info commands *clock_tree*] [info commands *clock_skew*] [info commands *cts*]"
catch {clock_tree_synthesis -help} msg; puts $msg
read_liberty $env(PDK_LIB)
read_lef $env(PDK_TLEF)
read_lef $env(PDK_LEF)
read_def /work/tests/designs/gcd/gcd.placed.def
read_sdc /work/tests/designs/gcd/gcd.sdc
# buffer list: replicate what the image's own cts.tcl uses for sky130hd
# (grep the container's openroad scripts for clock_tree_synthesis to copy the
# exact -buf_list/-root_buf/-wire_unit the flow uses)
clock_tree_synthesis -root_buf sky130_fd_sc_hd__clkbuf_16 \
    -buf_list "sky130_fd_sc_hd__clkbuf_8 sky130_fd_sc_hd__clkbuf_4 sky130_fd_sc_hd__clkbuf_2" \
    -wire_unit 20
repair_clock_nets
set_wire_rc -layer met2
estimate_parasitics -placement
report_clock_skew -setup
report_clock_skew -hold
report_checks -path_delay max -fields {slew capacitance input} -format full
write_def /work/tests/designs/gcd/runs/canonical/gcd.probe.cts.def
```

If `report_clock_skew` is absent in `openroad`, repeat inside `sta -no_init`
reading the CTS DEF. Record: inserted buffer count (from the log or the DEF
diff), max clock-pin arrival (insertion delay), setup/hold skew.

**Decides**: E5 criteria 1–3 (skew/insertion-delay magnitudes → tolerances)
and the exact report-command names for the golden.

## P-IRD — PDNSim availability

**Questions**: does the pinned `openroad` ship power-grid analysis at all,
and if so what is its command/metric surface.

```tcl
puts [info commands *power_grid*]
puts [info commands *analyze*]
puts [info commands *psm*]
puts [info commands *pdnsim*]
```

If `analyze_power_grid`/`check_power_grid` exists, run on
`gcd.placed.def` (PDN is in the DEF's SPECIALNETS):

```tcl
read_liberty $env(PDK_LIB)
read_lef $env(PDK_TLEF)
read_lef $env(PDK_LEF)
read_def /work/tests/designs/gcd/gcd.placed.def
check_power_grid -net VDD     ;# or analyze_power_grid -net VDD
```

Record: command surface, the worst-drop number, and whether the metric is
per-node voltage map or a single worst value (decides E8 criterion 2's map
shape). **If no such command exists, record that** — E8 then has no
in-container reference and is documented as deferred rather than being given
a self-defined golden.

## P-DRC — what can check a routed DEF

**Questions**: for the E7 standalone scoping — is there an independent
checker in the image that reads a routed DEF (or GDS made from it)?

```bash
which magic klayout openroad 2>/dev/null; magic --version 2>/dev/null
ls /pdk/sky130A/libs.tech/magic/ | head
```

Then, if magic exists:

```tcl
# inside `magic -dnull -noconsole -rcfile /pdk/sky130A/libs.tech/magic/sky130A.magicrc`
def read /work/tests/designs/gcd/gcd.routed.def
drc count
```

Magic may not read DEF directly in this build; the fallback is the
`gcd.drc.rpt`/`picorv32.drc.rpt` surface from `detailed_route -output_drc`
(already committed — empty for clean routes) plus `check_antennas` for the
antenna rule subset. Record which checker can see a foreign routed DEF —
that fixes E7's reference or drops the standalone scoping.

---

## What each probe unblocks

| Probe | Unblocks | If it fails |
|---|---|---|
| P-GR | E4 criteria 1–4 | Without `-global_routing`, E4's parasitic done-when falls back to comparing our guide-RC against the placement estimate plus routed-SPEF bounds — record the fallback in the doc |
| P-CTS | E5 tolerances + golden command | `clock_tree_synthesis` exists in the image's flow (LibreLane runs CTS), so failure is flag-name drift — re-probe with `-help` output |
| P-IRD | E8 entirely | No reference → E8 deferred, recorded honestly in `docs/results.md` + `docs/roadmap.md` |
| P-DRC | E7 standalone scoping | No DEF-reading checker → E7 pairs with E6 only |
