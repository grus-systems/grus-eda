# docs — repository text that moves with the code

AGENTS rule 8 — `docs/` in step with the code. All English.

## `docs/results.md` — dated entry (the acceptance record)

Heading `### 2026-11-27 — drt-scale-memory: measured DRT peak
footprint + <picked change>` (v<workspace>). Contents, in the
established entry style:

- **Machine context**: Mac mini M4 / 16 GB, macOS, rustc — same block
  the `drt-rayon` entry carries. Rep policy stated (gcd ≥3 reps,
  median; picorv32 single detached rep — cost reason named).
- **Command**: the `measurement.md` §2 invocations, verbatim.
- **Split table** (`measurement.md` §5 rows): bytes per structure,
  before and after, both designs. BTreeMap rows keep the `~` estimate
  mark — never presented as exact.
- **Peak RSS table**: `maximum resident set size` before/after per
  design + Δ%. This pair is the acceptance figure.
- **`route_ms` effect**: rebuild/chunk/scratch changes cost CPU —
  record the delta honestly (per `options.md` O2/O3/O4 cost notes).
- **Identity evidence**: `route_digest` before = after
  (`0xef2b3ff70271a2e9`), gcd DEF sha256 = committed golden sha,
  picorv32 DEF `cmp` clean, `collisions=`/`iter_progress` unchanged.
- **Honest bound** (acceptance item 4): state the fitted linearity
  `peak ≈ base + α·Σ_nodes + β·Σ_arcs` with the measured α/β per
  design, and name the design size where the reduced bound is still
  a concern (e.g. extrapolate to aes/ibex-scale guided-net counts —
  "at picorv32's per-net constant, a 100k-net design projects to
  ~N GB; the bound stays linear in guided nets").
- **What was run**: which reps and which verify arms actually
  executed (picorv32 legs named as detached runs or "not measured —
  deferred", never implied).
- Failed/divergent observations stay in the log with root cause
  (AGENTS §2).

## `docs/e6-drt.md` — §7 memory paragraph

A short "Resident footprint" block inside §7 (or appended to it):

- What's held for the whole rip-up loop (`topos`, `occ`, `marks`,
  `routes`) vs. what's transient (`outcomes` window, per-attempt
  `NetRun` scratch, rebuilt `NetGraph`s if O2 lands) — matching the
  shipped code exactly.
- The occupancy-snapshot rule: `NetTopo` is built once against
  post-`premark_obs` occupancy and is never rebuilt (attach/access
  probes read live marks); `NetGraph` is occupancy-free and
  rebuildable. If O2 lands, this paragraph is where the
  drop-and-rebuild scheme is described.
- The honest bound sentence from the results entry.

## `docs/status.md` — "Last updated" line

One `v<version> drt-scale-memory` sentence in the existing style:
picked change named, gcd/picorv32 peak before→after, "route digest +
emitted DEF byte-identical".
