# Ticket 13 — Routed SPEF for gcd: route, extract, and add the `routed_spef` golden mode (review step 6, part 1)

## Context

Review `docs/history/review-2026-11-11.md` Section 5, step 6: "A routed SPEF, then
decide E3.4 and E3.5 against it (Section 3)."

Every SPEF in this repository today is OpenROAD's **placement estimate**
(`estimate_parasitics -placement`), despite the file name `*.routed.spef`
(see `docs/e3-sta.md` Section 3, E3.2 item 1, and Section 5 "Not scheduled":
coupling capacitors, resistor loops and `*NAME_MAP` are implemented in the
SPEF reader but **untested against OpenSTA**, because no routed SPEF exists
here). This ticket creates a true routed SPEF for gcd and the matching
OpenSTA golden, so tickets 14 and 15 can score both parasitic estimators
against routed truth.

## Measured spike (maintainer, 2026-11-14, M4 mini, pinned container)

The route + extraction flow below was run by hand on `gcd.placed.def`
before this ticket was written. It works as-is; do not redesign it.

- Tcl sequence: `set_routing_layers -signal met1-met5 -clock met3-met5`,
  `global_route -guide_file gcd.guide -congestion_iterations 100`,
  `detailed_route -output_drc gcd.drc.rpt -verbose 0`,
  `write_def gcd.routed.def`,
  `define_process_corner -ext_model_index 0 X`,
  `extract_parasitics -ext_model_file /pdk/sky130A/libs.tech/openlane/rules.openrcx.sky130A.nom.calibre -lef_res`,
  `write_spef gcd.rcx.spef`.
  (The `define_process_corner` + `extract_parasitics -lef_res` form is what
  LibreLane's own `OpenROAD.RCX` step runs in this image.)
- Result: detailed-route DRC report **empty** (0 violations); RCX extracted
  269 nets, 1,306 RC segments. Whole container run ≈ 10 s.
- `gcd.rcx.spef` is 215,743 bytes (< 1 MB, committable): 269 `*D_NET`s,
  1,306 ground capacitors, **3,544 coupling-capacitor entries**, 1,037
  resistors, `*NAME_MAP` used throughout, `*R_UNIT 1 OHM`.
- `gcd.routed.def` is 224,344 bytes (< 1 MB, committable).
- The SPEF header carries a `*DATE` line with the wall-clock time, so the
  raw output is **not** bit-deterministic across runs. The generation script
  must normalise that one line (see below) and the maintainer will verify
  determinism by running the new step twice and comparing SHA-256.

## Task

Extend `tests/designs/gcd/generate_ref.sh`:

1. Add a new step **5b** (immediately after the existing step 5, so the
   routed SPEF exists before the STA golden step 9 runs; do not renumber
   the existing steps). It runs the spike's Tcl sequence inside the same
   pinned container, starting from the committed `gcd.placed.def`, and
   writes `gcd.routed.def`, `gcd.drc.rpt` and `gcd.rcx.spef` next to the
   other gcd artifacts.
   - After `write_spef`, normalise the `*DATE` line of `gcd.rcx.spef` to a
     fixed string so regeneration is bit-deterministic.
   - The script must fail loudly if the DRC report is non-empty (a golden
     taken from a dirty route is worthless). Print the violation count.
2. Extend the step-9 OpenSTA Tcl that builds `gcd.sta.reference.json` with
   a fourth mode, `routed_spef`, using the exact same
   `collect_endpoints_json` machinery as the other modes: same DEF
   (`gcd.placed.def` — routing does not move instances, and keeping the
   DEF fixed isolates parasitics as the only changed variable), same SDC,
   with `read_spef gcd.rcx.spef` as the parasitics source. The existing
   three modes' emitted values must not change.
3. Update the script's header comment (list of generated files) and the
   gcd section of `tests/designs/README.md`: the two new artifacts with
   their sizes, and an explicit note that `gcd.routed.spef` is the
   placement estimate while `gcd.rcx.spef` is the routed extraction.

## Files

- `tests/designs/gcd/generate_ref.sh`
- `tests/designs/README.md`

Do **not** edit `gcd.sta.reference.json` or commit generated artifacts in
your patch: golden values are emitted by the script when the maintainer
runs it (AGENTS.md Section 2 — never hand-write reference numbers). The
maintainer runs the generation, checks determinism and the DRC assertion,
and commits `gcd.rcx.spef`, `gcd.routed.def` and the regenerated golden.

## Acceptance

- `bash -n tests/designs/gcd/generate_ref.sh` passes.
- The diff touches only the two files above.
- The new Tcl follows the file's existing idioms (heredoc into
  `openroad -no_init -no_splash`, no `!` negations in Tcl fed on stdin —
  write `== 0` forms).
- Maintainer acceptance (outside the job): full gcd generation runs
  end-to-end in the pinned container; step 5b run twice gives identical
  SHA-256 for `gcd.rcx.spef` and `gcd.routed.def`; DRC report empty; the
  regenerated golden gains `routed_spef` with 53 endpoints and the other
  three modes are value-identical to the committed golden.
