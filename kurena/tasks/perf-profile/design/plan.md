# perf-profile — optimization plan framework and candidates

This file is the committed plan deliverable's skeleton: the phase
classification, the `rayon` plug-in candidates (pre-registered from code
reading), the Amdahl method, and the follow-on ticket slugs. The
implementation stage fills the measured shares and final ranking after
`measure.md` runs — hypotheses below are *expectations*, not measurements.

## Phase classification (fill `share` from the measured table)

| phase | class | parallel? | expectation |
|---|---|---|---|
| `sta.eval.walk` | compute (NLDM + DMP per arc-leg) | per-level `par_iter` already; wider: finer granularity inside levels | dominant on parasitics modes (Phase-2: ~85% of jpeg STA) |
| `sta.prepare.clock.slew` | compute | same levelized pattern as eval walk — `par_iter` per level | real since N3 (pulse-width slew pass), ~2× eval cost recorded 2026-11-19 |
| `sta.prepare.clock.walk` | compute | per-level parallel | second to `.slew` |
| `sta.partition.eval` (classes) | compute | `rayon::join`/scope across classes; memory-heavy (each clones `base_state`) | small on no-exception SDCs; scales with exception count |
| `sta.eval.endpoints` | compute | per-check `par_iter` + ordered collect; `skipped` merges serially | moderate (per-endpoint constraint evals) |
| `sta.parasitics` | compute | already `par_iter` ≥512 | moderate; PD-tree build per net |
| `sta.spef.read` | I/O + parse | serial text parse | scales with SPEF size (ibex 10 MB, jpeg ~? MB) |
| `sta.prepare.*` rest | mixed | `build_graph` serial; `levels` Kahn — parallel frontier possible but share-bound | small (Phase-2: ~15–19 ms jpeg) |
| `sta.partition.ctx` | serial (string `HashMap`s) | parallel fold over cells | small unless exceptions |
| `power.slews` | = full `sta` run | shares sta wins; candidate: slew-only mode skipping endpoint eval (model-parity check required) | dominant in `power` |
| `power.activity.resolve` | compute | levelized propagation | unknown — measure |
| `power.cells` / `power.switching` / `power.net_loads` | compute | per-cell / per-net `par_iter` + group reduce | likely small-to-moderate |
| `place.gp.density` | compute | already rayon inside (`PoissonWorkspace`) | dominant in GP (FFT) |
| `place.gp.wl` | compute | already rayon net gradient | second in GP |
| `place.legal.*` | **serial by construction** | ordered trial/evaluate/accept on live positions — no rayon without changing output | dominant in `place` (legalize ~70–80% in v0.1.5x) |
| `cong.decompose` | compute | already `par_iter` ≥512 | dominant in `congestion` |
| `cong.route` | compute | segment demand accumulation — parallel needs deterministic chunked reduce (f32 non-associative) | moderate |
| `*.load.*` / `*.report.json` | I/O + parse | serial; Liberty cached after first load | small warm, real on cold parse |

## Amdahl method

For phase share `s` and achieved speedup `k`, command ceiling
`T' = T·(1 − s + s/k)`; end-to-end the table multiplies across phases in
order. Recorded ceilings use `k = min(achievable parallelism, ~4–6)` on the
4P+6E M4 (memory-bound sections saturate earlier; state which assumption each
row used — `k` from the measured `rayon` efficiency of comparable phases, not
a guess of 10).

Plan table to fill during implementation:

```
| rank | phase | design/cmd | share | class | plug-in | k | ceiling | ticket |
```

## `rayon` plug-in points (named, pre-registered)

Already parallel (no ticket): `estimate_placement_parasitics`,
`spef_parasitics`, `decompose_design_nets`, `pi_models`, `wire_delays`,
`launch_seeds`, eval level loop ≥32, GP density/wirelength kernels, legalize
Pass-1 net extrema.

Candidates, in the order the classification expects them to matter:

1. **`sta.prepare.clock.slew` / `sta.prepare.clock.walk`** — same levelized
   `par_iter` pattern as `eval_sta`'s walk (the merge discipline: compute per
   level, apply serially). Lock with the existing STA goldens; the pattern is
   proven in the same file.
2. **`sta.eval.endpoints` (check/gating/output-port loops)** — per-check
   `par_iter().map()` collecting `(entries, skip-deltas)` with a serial
   `SkipReport` merge afterward; `graph.checks` is pin-sorted (built from a
   `BTreeMap`), so rayon's ordered `collect` keeps `skipped_endpoints`
   byte-identical.
3. **`power.cells` + `power.net_loads` + `power.switching`** — `par_iter`
   over cells/nets with a per-group fold; f32/f64 reduce order must be fixed
   (chunked deterministic reduce — same discipline as `cong.route`).
4. **`sta.partition.eval` per-class** — `rayon` over signature classes when
   classes > 1 (each eval clones `num_pins` state — note peak-RSS cost in the
   ticket).
5. **`power.slews` slew-only path** — not a rayon plug-in but a phase-removal
   candidate: `sta_pin_slews` currently buys the full eval+endpoint pass;
   whether `propagate_slew`'s unified slew equals `eval_sta`'s state slew
   per pin is a measured parity question, so it is a separate ticket gated on
   that check, not assumed here.

Serial/memory-bound (no parallelism ticket; optimize only if share is large
— and then algorithmically): `place.legal.*` (ordered move semantics),
`sta.spef.read`/`*.load.*` (parsers), `write_def`/`write_spef` (single big
string), `sta.partition.ctx` string maps, `assemble_sta_report` sort
(`par_sort` is legal — entries carry no order-sensitive ties — but the share
is expected trivial).

## Follow-on ticket slugs (recommended order = measured share ranking)

Named here so `docs/roadmap.md` can point at them; each lands only with its
measured share + ceiling filled in:

- `perf-par-slewclock` — parallelize `propagate_slew`/`propagate_clock` per
  level.
- `perf-par-endpoints` — parallelize `collect_endpoints` check loops.
- `perf-par-power` — parallelize `power` per-cell/per-net sums.
- `perf-par-classes` — parallel `eval_sta` across exception classes.
- `perf-slewonly-power` — slew-only path for `power` (parity-gated).
- `perf-legal-*` — only if `place.legal.*` share justifies it; algorithmic
  (cache/scan-order), not rayon.

Each optimization ticket obeys `AGENTS.md` §2: fast path bit-identical (or
tolerance-named) to the serial baseline, regression tests locking
equivalence, numbers logged in `docs/results.md`.

## `docs/roadmap.md` pointer (implementation writes)

Short "Performance" note: the measured baseline exists (link to the
results.md entry), top phases named, and the ticket slug list above as
candidate order — consistent with the file's "candidates, not commitments"
style.
