# Benchmarks

Runtime measurements extracted from the verification log. Every number here is
copied verbatim from an entry in `docs/results.md`; each section cites the
entry dates so the full measurement context (input files, machine state,
caveats) can be traced. Accuracy comparisons against the reference tools are
**not** in this document — they are the main content of `docs/results.md`.

**Measurement conditions.** Apple Silicon throughout; entries up to 2026-11-13
say only "Apple Silicon", entries from 2026-11-16 onward are an M4 Mac mini
unless noted otherwise. `cargo install` users get release builds; entries mark
whether the measurement was a debug or release binary. Median-of-5 where the
entry says `REPS=5`. This is a research prototype — nothing here is a sign-off
claim.

## Test designs

OpenROAD-flow-scripts `sky130hd` designs plus picorv32; cell/net counts from
the 2026-11-22 perf-profile baseline entry:

| design | cells | nets |
|---|---|---|
| gcd | 384 | 269 |
| picorv32 | 10,205 | 6,811 |
| aes | 12,839 | 9,039 |
| ibex | 16,644 | 12,137 |
| jpeg | 61,569 | 46,568 |

## Readers and file I/O (E0, 2026-11-09; release build)

| input | runtime |
|---|---|
| Liberty `.lib` cold parse (cache explicitly deleted) | 40.75 ms |
| Liberty warm mmap cache load | 5.16 ms |
| tech LEF + cell LEF (18 KB + 1.3 MB) | 4.95 ms |
| Yosys JSON netlist (gcd) | 3.34 ms |
| gate-level Verilog (gcd) | 0.90 ms |
| placed DEF read + HPWL (gcd) | 0.85 ms |

Parser scan optimization (v0.3.6, 2026-11-22, allocation-free SPEF/DEF/VCD
scans): `sta.spef.read` ×1.34–1.39 across designs (jpeg 161 → 115 ms),
`sta.load.def` ×1.17–1.26, VCD activity load ×1.52 (420 → 276 ms).

## Placement (E2, 2026-11-09; v0.3.5 on 2026-11-22)

Full placement pipeline on gcd (global placement + Abacus legalization +
4-round local refinement + DEF write): **17.29 ms** — Nesterov global place
11.30 ms, legalization 5.99 ms, DEF write 1.06 ms.

`grus-eda place` wall after the v0.3.5 incremental-extrema work
(median ms, REPS=5, M4 mini):

| design | before v0.3.5 | after v0.3.5 |
|---|---|---|
| gcd | 10 | 10 |
| picorv32 | 350 | 320 |
| aes | 380 | 370 |
| ibex | 560 | 480 |
| jpeg | 2,170 | 1,560 |

## Static timing analysis and power

Successive optimization records — each row is the cumulative state at that
version. Median ms, REPS=5, same M4 mini within each comparison.

Baseline (v0.3.2, 2026-11-22) — full-cell wall, `sta` with placement RC:

| design | wall (ms) |
|---|---|
| gcd | 50 |
| picorv32 | 860 |
| aes | 1,480 |
| ibex | 670 |
| jpeg | 6,000 |

Cumulative effect of the 2026-11-22 performance tickets:

| change | version | headline result |
|---|---|---|
| parallel `propagate_slew` | v0.3.3 | jpeg sta placement-RC 6,000 → 2,060 ms (×2.9); slew phase itself ×5–5.6 on aes/picorv32 |
| slew-only `sta_pin_slews` | v0.3.4 | jpeg power placement-RC 2,170 → 1,030 ms (×2.1) |
| allocation-free parsers | v0.3.6 | jpeg sta routed-SPEF 1,650 → 1,380 ms (×1.20) |
| DMP hot-loop memoization | v0.3.7 | jpeg sta placement-RC 1,290 → 1,090 ms (×1.18); ibex ×1.24 |

Earlier landmark (E3.6, 2026-11-13): closed-form DMP charge/crossing —
picorv32 SPEF STA **10.45 s → 0.98 s** (×10.7) with setup/hold slacks
unchanged within tolerance.

End-to-end STA on the router's own guide parasitics (v0.3.11, release):
gcd 14.41 ms whole-run elapsed; picorv32 167.62 ms (parasitics phase 30.88 ms).

## IR drop (E8, v0.7.3, 2026-11-23; release)

Sparse DC solve on the PDN mesh:

| design | net | solve elapsed | peak RSS |
|---|---|---|---|
| gcd | VPWR | 0.78 ms | 23.2 MB |
| gcd | VGND | 1.28 ms | 23.2 MB |
| picorv32 | VPWR | 9.42 ms | 41.9 MB |
| picorv32 | VGND | 10.93 ms | 41.9 MB |

Matrix sizes 240×240 (gcd) and ~5.4k×5.4k (picorv32); mesh build, row
generation and CSV I/O are excluded from the solve elapsed.

## DRC (E7, v0.12.0, 2026-11-23; release)

50-rule LEF-expressible subset over the full shape set:

| design | load | assemble | check |
|---|---|---|---|
| gcd | 13.9 ms | 0.7 ms | 55.5 ms |
| picorv32 | 51.0 ms | 13.9 ms | 14,451 ms |

Check is the dominant phase; spacing pair scans dominate.

## Detailed routing (E6; release)

Maze router, 8 iterations per run; since v0.14.3 the soft iterations search in
parallel on a per-call rayon pool (snapshot-search / ordered-commit, hard
iteration serial). Wall-clock for the routing loop is the `route_ms` census
field.

First full router results (v0.12.4, 2026-11-23 — one coherent measurement
series across both designs):

| design | guided | routed | unrouted | collisions | wall |
|---|---|---|---|---|---|
| gcd | 269 | 269 | 0 | 0 | 1,886 ms |
| picorv32 | 6,744 | 6,636 | 108 | 0 | 8,125,811 ms (≈135 min) |

Full Magic-gate leg on picorv32 (v0.12.9, 2026-11-25): `route_ms=8,206,939`
(~2.3 h, 8 iterations), emit census `guided=6,744 routed=4,728 unrouted=2,016
collisions=0`. The `--verify` second route emitted a byte-identical DEF
(determinism check). The unrouted residue is dominated by pins with no legal
access out of their halo (`stub_outside_cover` on 1,988 distinct nets) — the
failure class the post-gate ticket series (`drt-pin-collision`,
`drt-preemit-spacing`, `drt-guide-escape`, `drt-occupancy-ds` — all merged since)
is actively driving down, so per-version census numbers move fast; the dated
rows are in `docs/results.md`.

Parallel-era measurements (2026-11-27, all on the M4 Mac mini machine context
of their entries; `route_ms` median-of-5 on gcd unless noted):

| version | change | gcd `route_ms` | picorv32 |
|---|---|---|---|
| v0.14.2 | dense track-indexed occupancy vectors | 12,656 → 11,960 ms (−5.5%) | — |
| v0.14.3 | rayon snapshot-search / ordered-commit | 11,953 → **3,490 ms** at 8 threads (−70.8%); threads=1 7,122 | — |
| v0.15.1 | streamed per-attempt graph rebuild (memory) | peak RSS 153.1 → 95.2 MB; `route_ms` median 3,823 ms | peak RSS 2.37 GB → **958 MB** (−59.6%); `route_ms` 7,229,267 → 4,130,770 ms (−42.9%) |

The v0.15.1 `route_ms` rows compare different machine states across the
ticket's measurement series — the entry's own reconciliation applies; treat
the time delta as secondary to the RSS delta the ticket measured for.

Comparability warning: rows across versions are **not** like-for-like. Router
features added after v0.12.4 (pin-collision occupancy costing, hull masking,
notch fill, halo escape) change both the work per iteration and the achievable
unrouted count, and some census rows in the log are debug runs at
`MazeConfig::default()` rather than gate configuration. Treat this section as
scale markers, not a regression series.

## Source

`docs/results.md` — entries current through 2026-11-27. Discrepancies or
updated numbers should be fixed there first; this document is a digest, not
a record of its own.
