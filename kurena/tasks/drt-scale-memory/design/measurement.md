# measurement — peak RSS protocol and the per-structure split

Two instruments, mirroring the `drt-occupancy-ds`/`drt-rayon`
protocols: `/usr/bin/time -l` for the headline peak, and a committed
structural census for attribution. The census is a capacity walk
(`Vec::capacity() × size_of`), not an allocator hook — exact for the
Vec-dominated structures here, deterministic, and cheap enough to
leave permanently compiled in behind an env gate.

## 1. Instrumentation being added (commit 1)

New `crates/grus-eda-core/src/drt/mem.rs`, exported via `drt.rs`.
All emission is `GRUS_DRT_MEM`-gated stderr, `drt: mem <point> …`
lines matching the `GRUS_DRT_TIMING` style; zero output when unset.

```rust
/// `ru_maxrss` high-water mark in bytes (monotonic — a "worst so
/// far" reading at each checkpoint, so the leg that set the peak is
/// attributable). libc::getrusage; units differ by platform
/// (macOS bytes, Linux KiB) — normalize under cfg.
pub fn peak_rss_bytes() -> u64;

pub fn route_graphs_bytes(g: &RouteGraphs) -> usize;     // in graph.rs
pub(crate) fn net_topo_bytes(t: &NetTopo) -> usize;      // in maze.rs
fn occupancy_bytes(o: &Occupancy) -> usize;
fn marks_bytes(m: &[Vec<Mark>]) -> usize;
fn routes_bytes(r: &[Option<NetRoute>]) -> usize;
```

- Every `Vec`/`String` contributes `capacity() × size_of::<T>()`;
  the walk is exact.
- `BTreeMap` exposes no capacity. `members`/`cover` rows report
  `(entries, inner-vec bytes, len × BTREE_NODE_EST)` where
  `BTREE_NODE_EST: usize = 80` is labeled an **estimate** in the
  emitted line (`~` prefix) — never folded into the exact totals.
- `DesignDb`/`TechDb`/`tracks`/`vias` are not walked field-by-field;
  they are the `rest` row: `peak_rss_bytes() − Σ measured rows` at
  the `post_topos` checkpoint. Honest remainder, no fake precision.

Checkpoints (emit one `drt: mem` line each):

| point | site | fields |
|---|---|---|
| `post_graphs` | main.rs after `build_route_graphs_ext` | rss, graphs |
| `post_premark` | `route_nets_with_progress` after `premark_obs` | rss, occ |
| `post_topos` | after the `topos` collect | rss, topos Σ + per-field Σ (`attach`, `members`, `relax`, `access`, `cover`, `csr`) |
| `iter` | end of each iteration's commit pass | rss, occ, marks Σ, routed-so-far Σ, outcomes-buf bytes |
| `post_route` | after the min-area patch | rss, occ, marks, route_db |
| `post_emit` | main.rs after `apply_routes`/`write_routed_def` | rss, net_geoms |

Per-iteration emission is what makes a killed picorv32 run still
yield data (the `drt-occupancy-ds` precedent).

## 2. Baseline capture (step 0 — at HEAD, no code change)

```sh
cargo build --release -p grus-eda
GRUS=target/release/grus-eda
LIB=$HOME/.ciel/sky130A/libs.ref/sky130_fd_sc_hd
TLEF=$LIB/techlef/sky130_fd_sc_hd__nom.tlef
LEF=$LIB/lef/sky130_fd_sc_hd.lef

# gcd — ≥3 reps, median RSS is the table figure
/usr/bin/time -l $GRUS drt --tlef $TLEF --lef $LEF \
  --def tests/designs/gcd/gcd.placed.def \
  --guide tests/designs/gcd/gcd.guide \
  --out-def /tmp/gcd.mem.base.def \
  --reference tests/designs/gcd/gcd.drt.reference.json

# picorv32 — inputs are gitignored; regenerate via
# tests/designs/picorv32/generate_ref.sh if placed.def is absent.
# Multi-hour detached leg (serial route recorded ~128 min on
# 2026-11-26; post-drt-rayon wall unmeasured for this design).
/usr/bin/time -l $GRUS drt --tlef $TLEF --lef $LEF \
  --def tests/designs/picorv32/picorv32.placed.def \
  --guide tests/designs/picorv32/picorv32.guide \
  --out-def /tmp/picorv32.mem.base.def \
  --reference tests/designs/picorv32/picorv32.drt.reference.json \
  --self-drc-tolerance 3
```

- `/tmp/gcd.mem.base.def` must sha256 to
  `1422d02765f6aba99e25f1792436607c788ca31397c0bf843340bd65fb2f9482`
  (the committed golden). `/tmp/picorv32.mem.base.def` is **kept** —
  it is the pre-change byte-identity baseline for the picorv32 arm.
- Record `maximum resident set size` per rep + `route_ms` from the
  census line. Single rep for picorv32 (cost), policy stated in the
  results entry.

## 3. After-instrumentation split run

Same commands + `GRUS_DRT_MEM=1`. The `post_topos` checkpoint's
`rest` row attributes db/tech/misc; per-iteration rows show whether
`occ`/`marks`/`outcomes` grow past the per-net pair. This table
drives the pick in `options.md` — **no reduction is coded before
these numbers exist.**

## 4. After-run

Identical commands post-change: `drt: mem` rows before vs after +
new `/usr/bin/time -l` peaks. The acceptance pair is (old peak, new
peak) per design; the split table is the explanation. Also `cmp`
each new DEF against its step-0 baseline — byte-identity is the
correctness gate, recorded with the numbers.

## 5. Results table format (for `docs/results.md`)

| structure | gcd B | picorv32 B | after gcd B | after picorv32 B |
|---|---|---|---|---|
| route_graphs | | | | |
| net_topos — attach | | | | |
| net_topos — members (~est) | | | | |
| net_topos — relax | | | | |
| net_topos — access | | | | |
| net_topos — cover (~est) | | | | |
| net_topos — csr | | | | |
| occupancy | | | | |
| marks | | | | |
| routes + residuals | | | | |
| outcomes hi-water | | | | |
| rest (rss − measured) | | | | |
| **peak RSS (`time -l`)** | | | | |
