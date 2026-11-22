# e4-guides-emit — Reference generation

Committed OpenROAD `global_route -guide_file` outputs for both fixtures,
emitted by the committed `generate_ref.sh` scripts in the pinned image
(`ghcr.io/librelane/librelane:3.0.16`, same `docker run -v $REPO:/work -v
$PDK_ROOT:/pdk` re-exec pattern the scripts already use).

## New step in each `generate_ref.sh`

Append a step `5c` (after `5b`, which is unchanged and keeps writing its
gitignored `runs/canonical/<d>.guide`) to
`tests/designs/gcd/generate_ref.sh` and
`tests/designs/picorv32/generate_ref.sh`:

```tcl
openroad -no_init -no_splash << 'EOF'
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/<d>/<d>.placed.def
read_sdc /work/tests/designs/<d>/<d>.sdc
set_routing_layers -signal met1-met5 -clock met3-met5
set_global_routing_layer_adjustment met1 0.30
set_global_routing_layer_adjustment met2-met5 0.20
global_route -guide_file /work/tests/designs/<d>/<d>.guide -congestion_iterations 100
exit
EOF
```

- The `set_global_routing_layer_adjustment` calls match the capacities
  `GCellGrid::build` applies by default (`met1 0.30`, `met2..met5 0.20`)
  and the P-GR probe Tcl — apples-to-apples congestion input for the
  topology comparison. Step `5b` deliberately does **not** change: its
  guide exists only to feed `detailed_route` in the same session.
- The file lands directly in `tests/designs/<d>/<d>.guide` — it is the
  committed golden, not a `runs/` scratch artifact. Update the header
  comment of each script to list `<d>.guide` among the generated files.
- Determinism: GRT is deterministic on a fixed input/image, so a rerun
  must reproduce the committed file byte-identically; the regeneration
  discipline is the usual one — rerun the script as committed, `git diff`
  the output, investigate any diff instead of hand-editing (E0 lessons 1–2).
- `picorv32.placed.def` is gitignored and regenerated earlier in the same
  script; `picorv32.guide` is golden of a generated artifact, same
  standing as `picorv32.reference.json`.

## Size check

`gcd.guide` ≈ 70 KB (P-GR measured 1953 boxes / 269 nets) — comfortably
committable. `picorv32.guide` at ~6.8k signal nets lands around 1.5–2 MB,
over the `AGENTS.md` 1 MB guideline. The acceptance requires the committed
artifact and the comparison test needs the full geometry, so it commits;
record the measured size and the deviation rationale in `docs/results.md`.
If the file comes out dramatically larger (>5 MB), stop and reconsider —
e.g. gzip sidecar or a per-net digest — rather than committing blind.

## `tests/designs/README.md`

Add the `.guide` files to each design's artifact list with provenance:
emitted by `generate_ref.sh` step 5c inside the pinned image from
`<d>.placed.def`, consumed by the guide-comparison tests.

## Read-back check script

New `tests/designs/gcd/check_guide_readback.sh`: takes our emitted guide
path (default `tests/designs/gcd/runs/guide-check/gcd.ours.guide`), re-execs
into the container like `generate_ref.sh`, then:

```tcl
read_liberty ... ; read_lef (tlef + lef)
read_def /work/tests/designs/gcd/gcd.placed.def
read_sdc /work/tests/designs/gcd/gcd.sdc
set_wire_rc -layer met2
read_guides /work/<guide-path>
estimate_parasitics -global_routing -spef_file /work/tests/designs/gcd/runs/guide-check/gcd.ours.guide.spef
exit
```

`set_wire_rc` first (`EST-0018` otherwise). Exit status propagates
`openroad`'s; a `read_guides` failure is a hard error, not a warning. The
`estimate_parasitics -global_routing` SPEF also proves the read-back
guides are consumed — record in `docs/results.md` which of the two steps
were actually exercised (AGENTS.md §7.9: say what you ran).
