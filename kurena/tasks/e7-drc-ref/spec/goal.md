# e7-drc-ref — DRC reference surfaces on routed DEF

E7 ticket 1 (reference generation, no Rust work). E7 is a subset
checker at best — Magic's tech file is the canonical rule deck and is
only partially expressible in LEF. This ticket commits the three
reference surfaces that bound what a checker could verify, plus the
scope boundary between LEF-expressible and Magic-only rules.

Depends on `e6-drt-ref` for a routed DEF (`<design>.drt.routed.def`
from the baseline variant; if that ticket hasn't landed, `gcd.routed.def`
from earlier OpenROAD runs may substitute — the spec names the input).

## Scope

1. Committed script `tests/designs/generate_drc_ref.sh <design>
   <routed-def>` in the pinned image.
2. Three reference surfaces on the same routed DEF:
   - **Magic**: `magic -dnull -noconsole -rcfile
     /pdk/sky130A/libs.tech/magic/sky130A.magicrc` (needs
     `-e PDK_ROOT=/pdk`), `lef read` (tlef + cell lef) + `def read` +
     `load` + `select top cell` + `drc euclidean on` + `drc style
     drc(full)` + `drc check` + `drc count total` + `drc listall why`
     (per-rule breakdown) — P-DRC already confirmed 0 errors on
     `gcd.routed.def`
   - **OpenROAD DRT check**: the `-output_drc` report surface from
     `e6-drt-ref` (already committed — record its emptiness/count,
     not a re-run unless needed)
   - **Antennas**: `check_antennas` report on the routed DEF
3. Machine-readable `<design>.drc.reference.json`: { magic_total,
   magic_per_rule{rule_name:count}, magic_version, drt_drc_count,
   antenna_violations, antenna_per_net?, input_def_sha_prefix }.
4. **Scope-boundary record** (the load-bearing artifact for later
   tickets): enumerate the rule categories Magic checked (`drc
   listall why` style, or Magic tech-file section names) and classify
   each as {LEF-expressible (SPACING/WIDTH/ENCLOSURE-class), antenna,
   Magic-only (geometry/rule-deck)}. This classification is documented
   in the JSON + `docs/`, since it defines what a `grus-eda drc`
   subset could honestly claim.
5. Sanity control: deliberately break the routed DEF (e.g. shift one
   wire to create a spacing violation — scripted, committed) and
   re-run Magic; the per-rule count must go nonzero and `drc listall
   why` must name the violated rule. This proves the reference surface
   can see violations, not just report 0.
6. `docs/results.md`: commands, counts, version, sanity-control
   outcome.

## Files

- `tests/designs/generate_drc_ref.sh`
- `tests/designs/<d>/<d>.drc.reference.json` (+ broken-variant JSON or
  rpt)
- `docs/results.md`, possibly `docs/e7-drc.md` for the rule taxonomy
