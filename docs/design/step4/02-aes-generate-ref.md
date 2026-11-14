# Step 4 ticket 02: aes reference generation script

Repository: grus-eda (this repo). Work on `dev`. Follow
`AGENTS.md` (English only, Conventional Commits, stage explicit
paths, no `--force`). The aes RTL, `aes.sdc` and `config.yaml` are
already committed under `tests/designs/aes/`.

## Task

Create `tests/designs/aes/generate_ref.sh` by adapting
`tests/designs/gcd/generate_ref.sh` (read it fully first) so it
produces the aes equivalents of every gcd artifact, plus one new
artifact for the F2 congestion-map comparison. Keep the script's
overall structure, container image, and step numbering; change only
what the design requires:

- Design name `aes_cipher_top`, files from `config.yaml`, SDC
  `aes.sdc` (3.6 ns), output names `aes.*`.
- Step 7e (utilisation variants): produce `aes.util65.def` (65%
  core utilisation) in addition to keeping the flow's placed DEF,
  and run step 7b's grid/resource dump on the util65 placement.
- Step 8 reference metrics and the STA reference: same fields as
  gcd's JSON files.
- **New step 7f: per-edge usage dump.** After running OpenROAD
  `global_route` on `aes.util65.def` inside the same container
  session pattern as step 7b, write `aes.gcell_usage.txt`:
  - Header lines: `# origin_x_dbu <int>`, `# origin_y_dbu <int>`,
    `# tile_dbu <int>`, `# nx <int>`, `# ny <int>` (the GCell grid
    the numbers below refer to).
  - One line per GCell edge with nonzero usage or capacity:
    `H <x> <y> <usage> <capacity>` for the horizontal edge leaving
    cell (x, y) to the right, `V <x> <y> <usage> <capacity>` for
    the vertical edge leaving cell (x, y) upward; usage and
    capacity are integers aggregated over all signal routing
    layers.
  - Primary approach: aggregate the routing guides (`dbGuide`
    boxes per net) per edge per layer for usage, and reuse step
    7b's per-layer resource computation for capacity. If guides
    cannot produce this, say exactly what blocked it in your
    report and what data source you used instead; never fabricate
    numbers.
  - Layer adjustments: apply the same 0.3 global routing layer
    adjustment the ORFS SDC sets, consistently in the router call
    and in the capacity computation.
- The script must run end to end from a clean checkout on the
  maintainer's machine (it starts the LibreLane container itself,
  like gcd's).

Also adapt `.gitignore` entries if the repo has per-design ones for
gcd/picorv32 generated files: generated aes files over 1 MB (if
any) get the picorv32 treatment (git-ignored, listed in
`tests/designs/README.md` with generation instructions); files
under 1 MB are committed by the maintainer after a verified run,
not by this ticket — this ticket delivers the script only.

## Out of scope

- Rust tests or harness changes (next ticket).
- Running the script (needs the maintainer's container host);
  verify with `bash -n` and a Tcl syntax sanity check of the
  heredocs if feasible, and say so in the report.

## Report format

Files created/changed; how usage/capacity are computed (quote the
Tcl approach); any deviations from the gcd script and why; risks
you noticed for the first real run.
