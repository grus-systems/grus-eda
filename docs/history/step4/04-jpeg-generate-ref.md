# Step 4 ticket 04: jpeg reference generation script

Repository: grus-eda (this repo). Work on `dev`. Follow
`AGENTS.md` (English only, Conventional Commits, stage explicit
paths, no `--force`, executable bit on scripts). The jpeg RTL,
`jpeg.sdc` and `config.yaml` are already committed under
`tests/designs/jpeg/`.

## Task

Create `tests/designs/jpeg/generate_ref.sh` by adapting
`tests/designs/aes/generate_ref.sh` (read it fully first; it is
the settled pattern after the aes spike, including the step 7f
per-edge usage dump via `dbGCellGrid` getUsage/getCapacity and
the uniform layer adjustment). Change only what the design
requires:

- Design name `jpeg_encoder`, files from `config.yaml` (note the
  `include/dct_cos_table.v` entry), SDC `jpeg.sdc` (5.0 ns),
  output names `jpeg.*`.
- Step 7e/7a: the high-utilisation variant is `jpeg.util65.def`
  (65%), dumped by step 7f exactly as for aes.
- Layer adjustment: jpeg's ORFS SDC also sets a global routing
  layer adjustment; check `flow/designs/sky130hd/jpeg` in
  OpenROAD-flow-scripts for the value and use it uniformly (the
  aes value was 0.3; do not assume jpeg's is the same — state the
  value you found and its source in the report).
- Steps 8/9: `jpeg.reference.json` and
  `jpeg.sta.reference.json` with the same fields as aes.
- Add `tests/designs/jpeg/runs` to `.gitignore` if not covered.
- Set the executable bit on the new script (the aes script lost
  it through the patch flow and CI failed; do not repeat that).

This ticket delivers the script only; the maintainer runs it.

## Files

- Create: `tests/designs/jpeg/generate_ref.sh`
- Possibly modify: `.gitignore`

## Report format

Files created/changed; the layer-adjustment value and its
source; any deviations from the aes script and why; risks for
the first run (especially the include file handling).
