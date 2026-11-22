# e7-drc-ref — Overview

E7 (DRC) ticket 1: commit the DRC reference surfaces on a routed DEF and
their reproduction script, per the scoping done-when
(`kurena/tasks/next-stage-scoping/design/done-when.md` §E7) and the P-DRC
measurements logged in `docs/results.md` (2026-11-22
"next-stage-scoping"). Reference only — no checker implementation in
this ticket. The load-bearing artifacts are the **scope boundary**
(LEF-expressible vs antenna vs Magic-only rules, `taxonomy.md`) and the
**sanity control** (a scripted broken DEF proving the surface detects
violations, `sanity.md`), not just the 0-count golden.

## Deliverables

- `tests/designs/generate_drc_ref.sh` — the committed generator
  (`script.md`). `<design>` as `$1`, `<routed-def>` as `$2` (a
  repo-relative path; required, so the checked DEF is never ambiguous).
  Host wrapper re-invoking itself inside
  `ghcr.io/librelane/librelane:3.0.16`, mirroring
  `generate_irdrop_ref.sh` / `generate_cts_ref.sh`.
- `tests/designs/gcd/gcd.drc.reference.json` — machine-readable golden
  (`golden.md`): Magic total + per-rule counts + version + checked-rule
  taxonomy, DRT report count, antenna count, sanity-control block —
  every value script-emitted, nothing hand-written.
- `tests/designs/picorv32/picorv32.drc.reference.json` — same, **if**
  `picorv32.routed.def` exists (gitignored, seeded; regenerate via
  `tests/designs/picorv32/generate_ref.sh` step 7). If the run is not
  clean (runtime, parser drift, nondeterminism), defer explicitly with
  the reason in `docs/results.md`.
- `docs/e7-drc.md` — the authored rule-taxonomy / scope-boundary
  document (`taxonomy.md`).
- `docs/results.md` entry, `tests/designs/README.md` artifact rows,
  `docs/roadmap.md` E7 progress line.

## Key decisions

- **New top-level script, no flow re-run for the golden path.** Same
  reasoning as `e5-cts-ref`/`e8-irdrop-ref`: the reference reads only
  committed-or-seeded inputs (a routed DEF + the public PDK). Routing
  is *not* re-run on the golden path — except the DRT-report
  regeneration fallback below.
- **Input DEF is an argument, with a documented preference order.**
  `$2` names the routed DEF explicitly (e6 will produce
  `<d>.drt.routed.def` from its baseline variant; absent that,
  `<d>.routed.def` from `generate_ref.sh` step 5b). The JSON records
  which file, its size and SHA-256 prefix, and its provenance tag —
  the surface measures *that* file, never an implicit one.
- **Three surfaces on the same DEF, three processes.** (1) Magic
  `drc(full)` — the canonical deck, run once on the input DEF and once
  on the broken DEF (clean-state convention from e8). (2) OpenROAD
  `check_antennas` — a real run on the input DEF (cheap, no routing).
  (3) The OpenROAD `-output_drc` report — **not** re-run; the report
  file produced alongside the input DEF is discovered and counted
  verbatim. If no report exists, the script regenerates one by
  re-running the exact step-5b route sequence into `runs/drc/` scratch
  and compares the re-routed DEF's SHA with the input's — recording
  `existing` / `regenerated_identical` / `regenerated_divergent`
  (this doubles as a routing-determinism measurement; see
  `script.md` step B).
- **Rule taxonomy is script-enumerated, doc-classified.** Magic's rule
  names are extracted from the public sky130A tech file's `drc` section
  (`/pdk/sky130A/libs.tech/magic/sky130A.tech` — PDK data, readable per
  the licensing rules), classified by a committed keyword classifier,
  and reviewed in `docs/e7-drc.md` (`taxonomy.md`). This is what bounds
  any future `grus-eda drc` claim.
- **Sanity control mutates geometry, not invocation.** A python step
  appends one disjoint `+ NEW` wire on a different signal net in the
  `NETS` section, at a sub-minimum edge gap from an existing wire
  (`sanity.md`) — deterministic: a guaranteed different-net spacing
  violation without depending on neighbor analysis. The broken Magic
  run must report nonzero errors and name the rule verbatim.
- **picorv32 conditional.** `picorv32.routed.def` (7 MB) and the empty
  `picorv32.drc.rpt` exist in seeded worktrees. If either input is
  absent the script fails loudly naming the producer
  (`generate_ref.sh` step 7); a clean run commits the JSON.
- **`runtime_s` is the named varying field.** Acceptance allows one
  varying metadata field; per-step runtimes go there and the byte-repro
  check diffs everything else.
- **No version bump.** Nothing Cargo-visible changes; the goldens have
  no consumer test yet (that is the subset-checker ticket). Stated in
  the commit + `docs/results.md` per acceptance. `cargo test
  --workspace` and `clippy -D warnings` still run clean.

## Reference facts

- **P-DRC (scoping, 2026-11-22)**: image ships Magic 8.3.623 and
  KLayout. `magic -dnull -noconsole -rcfile
  /pdk/sky130A/libs.tech/magic/sky130A.magicrc` requires
  `PDK_ROOT=/pdk` (the magicrc does `tech load
  $PDK_ROOT/.../sky130A.tech`). Sequence: `lef read` (tlef + cell lef)
  → `def read gcd.routed.def` (269 nets, 384 subcells) → `load gcd` →
  `select top cell` → `drc euclidean on` → `drc style drc(full)` →
  `drc check` → `drc count total` → `drc listall why`. Result: **0
  errors** on `gcd.routed.def`.
- **Existing DRT reports**: `generate_ref.sh` step 5b (both designs)
  runs `detailed_route -output_drc <d>.drc.rpt -verbose 0`; the files
  are gitignored. `picorv32.drc.rpt` is present (0 lines); `gcd.drc.rpt`
  is absent in a fresh/seeded worktree → the regeneration fallback is
  exercised by gcd (script.md step B).
- **e6-drt-ref not landed**: no `generate_drt_ref.sh` /
  `*.drt.routed.def` yet. Discovery order below handles it if it lands
  first; otherwise step-5b artifacts substitute per the spec.
- **Antenna surface unprobed**: `check_antennas` exists in OpenROAD
  (OpenLane runs it post-route); flag/return grammar is pinned on first
  run (`script.md` step A).

## Files

Implementation is expected to touch:

- `tests/designs/generate_drc_ref.sh` (new)
- `tests/designs/gcd/gcd.drc.reference.json` (generated, committed)
- `tests/designs/picorv32/picorv32.drc.reference.json` (generated, committed — if `picorv32.routed.def` present and run clean)
- `docs/e7-drc.md` (new — rule taxonomy / scope boundary)
- `docs/results.md`
- `tests/designs/README.md`
- `docs/roadmap.md`
- `.gitignore` (only if a committed-sized artifact must be excluded — the `runs/` and `*.drc.rpt` rules already cover all scratch)
