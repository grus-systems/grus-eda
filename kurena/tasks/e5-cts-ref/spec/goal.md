# Goal

E5 (clock tree synthesis) ticket 1: commit the TritonCTS
reference artifacts and reproduction script on gcd, per
the scoping done-when
(`kurena/tasks/next-stage-scoping/design/done-when.md` §E5,
P-CTS measured on the pinned image).

## Work

- Extend `tests/designs/gcd/generate_ref.sh` (or a new
  committed script following its conventions) to run, in
  `ghcr.io/librelane/librelane:3.0.16`:
  `clock_tree_synthesis -root_buf clkbuf_16 -buf_list
  "clkbuf_8 clkbuf_4 clkbuf_2" -wire_unit 20` +
  `repair_clock_nets` on `gcd.placed.def`/`gcd.sdc`,
  then `estimate_parasitics -placement` (with
  `set_wire_rc` first — EST-0018) and the report
  surface: `report_clock_skew -setup/-hold`,
  `worst_clock_skew -setup/-hold`, `report_cts`,
  `report_cts_config` (`report_clock_tree` does not
  exist in this image — do not call it).
- Commit the artifacts under `tests/designs/gcd/`:
  - `gcd.cts.def` (TritonCTS output DEF — needed as the
    e2e STA fixture's netlist side),
  - `gcd.cts.reference.json` — parsed report metrics:
    skew setup/hold, max insertion delay (clock-pin
    latency), buffer count per cell type, dummy-load
    count, sink count, path depth, clock-net
    wirelength (expected per P-CTS: 5 `clkbuf_16` +
    3 dummies, 35 sinks, depth 2, ≈0.31 ns latency,
    ≈0 ps skew — record what the script actually
    emits, never hand-write).
  - `gcd.cts.sta.reference.json` — OpenSTA
    propagated-clock endpoint report on the TritonCTS
    DEF (the criterion-3 golden).
- Record in `docs/results.md`: script steps, measured
  values vs the P-CTS scoping numbers, artifact sizes.
- If `picorv32` CTS reference is cheap to add in the
  same script, include it as the scale-up fixture —
  otherwise name it for ticket 2.

## Done when

The reference set exists, is reproducible from the
committed script in the pinned image, and the scoping
P-CTS numbers are confirmed or corrected in
`docs/results.md`. No CTS implementation in this
ticket — reference only.
