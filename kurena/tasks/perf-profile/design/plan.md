# perf-profile — optimization plan (measured 2026-11-22, v0.3.2)

Measured baseline: `docs/results.md` 2026-11-22 "perf-profile baseline".
Shares are medians of 5 warm-cache reps on the M4 mini; `measure.sh`
produced them. Expectations from the pre-registered skeleton are kept in
the rightmost column where the measurement confirmed or contradicted
them.

## Phase classification (measured shares)

| phase | class | parallel? | measured share |
|---|---|---|---|
| `sta.prepare.clock.slew` | compute, **serial** | same levelized pattern as `eval.walk` — `par_iter` per level, apply serially | **dominant**: 77–81% of `sta` wall on parasitics modes for jpeg/aes/picorv32; 50% ibex; 41% gcd. 4.84 s of jpeg's 6.0 s |
| `sta.eval.walk` | compute | already per-level `par_iter` (≥32 pins/level) | second: 14% jpeg, 18% aes, 15% picorv32, 35% ibex |
| `sta.prepare.clock.walk` | compute | same levelized pattern | trivial: ≤15 ms jpeg (0.3%) — clock-net-only pass |
| `sta.partition.*` | compute | `rayon` across classes | **not exercised**: none of the five committed SDCs partitions |
| `sta.eval.endpoints` | compute | per-check `par_iter` + ordered collect | trivial: ≤0.5% every design — the skeleton's "moderate" was wrong, drop the ticket |
| `sta.parasitics` | compute | already `par_iter` ≥512 | real but small: 1.6% jpeg (95 ms), 3.1% ibex |
| `sta.spef.read` | I/O + parse | serial text parse | 4.8% jpeg (297 ms / 35 MB), 16.4% picorv32-rcx (167 ms / 8.4 MB) |
| `sta.prepare.*` rest | mixed | serial graph/levels | ≤0.5% each, ~1.5% aggregate on jpeg |
| `power.slews` | = full `sta` prepare+eval | inherits `sta.*` fixes | **94% of `power` wall** on jpeg (5.75 s); inside it: clock.slew 4.82 s + eval.walk 0.80 s |
| `power.activity.load` (VCD) | I/O + parse | serial VCD scan | 94.5% of the `--vcd` cell (860 ms / 18 MB dump) |
| `power.cells` / `.switching` / `.net_loads` / `.activity.resolve` | compute | per-cell/per-net `par_iter` + group reduce | ≤1.0% each on jpeg — the skeleton's ticket under it is de-ranked |
| `place.gp.density` | compute | already rayon inside | 20.3% of `place` jpeg (849 ms) |
| `place.gp.wl` | compute | already rayon | 16.4% (685 ms) |
| `place.gp.step` + `gp.init` | compute | fused scalar loop / serial init | 14.1% combined (588 ms) |
| `place.legal.pass1/2/3` | **serial by construction** | ordered trial/evaluate/accept on live positions | **43.5% combined** on jpeg (pass3 18.7%, pass2 14.8%, pass1 10.0%); abacus 1.0% |
| `cong.load` (DEF parse) | I/O + parse | serial | 56.8% of `congestion` jpeg (119 ms of 210 ms) |
| `cong.decompose` | compute | already `par_iter` ≥512 | 24.6% (52 ms) |
| `cong.route` | compute | deterministic chunked reduce needed | 7.0% (15 ms) |
| `*.load.*` rest / `*.report.json` | I/O + parse | serial | ≤2% each; DEF 2.0–4.9% on big DEFs |

## Amdahl method

For phase share `s` and achieved speedup `k`, command ceiling
`T' = T·(1 − s + s/k)`; end-to-end the table multiplies across phases in
order. Recorded ceilings use `k = min(achievable parallelism, ~4–6)` on the
4P+6E M4 (memory-bound sections saturate earlier; state which assumption each
row used — `k` from the measured `rayon` efficiency of comparable phases, not
a guess of 10).

Measured `k` inputs: whole-command user/real on jpeg is 1.67–1.73
(`sta`, `power`, `place`) — today's wall is serial-dominated. The
comparable already-parallel phase (`sta.eval.walk`, same levelized
`par_iter` pattern) compresses ~4.8 s-equivalent serial work to 0.85 s,
i.e. `k` ≈ 4–5 for pure compute on the 4P+6E mini; planning uses
`k = 3` (conservative — DMP table lookups are memory-bound and E-cores
share the tail). Parser/memory phases get `k = 1` unless restructured.

| rank | phase | design/cmd | share | class | plug-in | k | ceiling | ticket |
|---|---|---|---|---|---|---|---|---|
| 1 | `sta.prepare.clock.slew` | jpeg `sta` both parasitics modes | 0.77–0.81 | serial → per-level `par_iter` | `propagate_slew` level loop, `eval_sta`-style apply-after-collect | 3 | jpeg sta 6.0 s → ~2.7 s (×2.2) | `perf-par-slew` |
| 2 | `power.slews` = `sta_pin_slews` | jpeg `power` | 0.94 | inherits rank 1 + phase removal | slew-only mode skips `eval_sta`+endpoints inside `sta_pin_slews` (parity check first) | — | after rank 1: power 6.1 s → ~1.6 s (×3.8 combined) | `perf-slewonly-power` |
| 3 | `place.legal.pass1/2/3` | jpeg `place` | 0.44 | serial, ordered-move | **not rayon** — algorithmic (candidate-set pruning, incremental HPWL deltas, band scan order) | 1.5–2 | place 4.2 s → ~3.2 s (×1.3) | `perf-legal-passes` |
| 4 | `sta.eval.walk` | ibex/jpeg `sta` | 0.14–0.35 | already parallel | finer granularity inside levels; NLDM/DMP table cache | — | second-order once rank 1 lands | deferred (re-measure) |
| 5 | `sta.spef.read` | picorv32-rcx/jpeg `sta` | 0.05–0.16 | serial parse | faster tokenizer (measured 297 ms / 35 MB ≈ 118 MB/s) | 2 | sta.spef.jpeg −2.4% | `perf-spef-parse` |
| 6 | `cong.load` = DEF parse | jpeg `congestion` | 0.57 | serial parse | shares a `perf-def-parse` ticket with `sta.load.def` (2–5% of sta) | 2 | congestion 210 → ~150 ms | `perf-def-parse` |
| 7 | `power.activity.load` (VCD) | `power --vcd` | 0.95 | serial parse | chunked VCD scan / mmap | 2 | vcd cell 910 → ~530 ms | `perf-vcd-parse` |
| — | `sta.eval.endpoints`, `power.cells/.switching`, `sta.prepare.*` rest, `cong.route` | — | ≤1% each | — | measured trivial: no ticket | — | — | dropped |

## `rayon` plug-in points (named, pre-registered)

Already parallel (no ticket): `estimate_placement_parasitics`,
`spef_parasitics`, `decompose_design_nets`, `pi_models`, `wire_delays`,
`launch_seeds`, eval level loop ≥32, GP density/wirelength kernels, legalize
Pass-1 net extrema.

Candidates, in measured order:

1. **`sta.prepare.clock.slew` (`propagate_slew` level loop)** — the
   skeleton's expectation held and exceeded it: not ~2× eval cost but
   ~5.7× on jpeg (4.84 s serial vs 0.85 s parallel). Same
   compute-per-level/apply-serially discipline as `eval_sta`'s walk —
   `sample` puts ~80% of main-thread samples in `sta::dmp`
   (`rc_crossing`, `fit_ramp_duration`, `solve_dmp_with`,
   `pi_waveform_slew`) reached through it. `sta.prepare.clock.walk` rides
   the same pattern but its measured share (0.3%) does not justify its
   own ticket; fold it in only if it is free.
2. **`power.slews` slew-only path** — phase removal, not rayon:
   `sta_pin_slews` buys the full prepare+eval+endpoint pass for slews.
   The `propagate_slew`-unified-slew ≡ `eval_sta`-state-slew parity check
   is the gate; measured, it removes ~13% of jpeg `power` wall on top of
   rank-1's inheritance.
3. **`place.legal.pass1/2/3`** — serial by construction (ordered
   trial/evaluate/accept); at 43.5% of jpeg `place` the algorithmic
   ticket is justified: candidate-set pruning and incremental HPWL
   deltas, not rayon.
4. **`sta.spef.read` / `cong.load` / `sta.load.def` / VCD scan** —
   parser tickets at their measured shares; batch as one `perf-*-parse`
   sequence if the tokenizer work is shared.
5. **`sta.eval.endpoints`, `power.cells/.switching/.net_loads`,
   `sta.partition.eval`, `cong.route`, `assemble` sort** — measured ≤1%
   each (endpoints ≤0.5%, partition path never taken on these SDCs).
   No ticket until a design exercises them above noise.

## Follow-on ticket slugs (recommended order = measured share ranking)

Named here so `docs/roadmap.md` can point at them; each lands with the
measured share + ceiling above:

- `perf-par-slew` — parallelize `propagate_slew` per level (rank 1,
  ×2.2 jpeg `sta`, ×~2.9 jpeg `power` via inheritance).
- `perf-slewonly-power` — slew-only `sta_pin_slews` path (rank 2,
  parity-gated).
- `perf-legal-passes` — algorithmic detailed-placement pass work
  (rank 3, ×~1.3 jpeg `place`).
- `perf-spef-parse`, `perf-def-parse`, `perf-vcd-parse` — parser
  speedups at measured shares (ranks 5–7).
- `perf-par-endpoints`, `perf-par-power`, `perf-par-classes` —
  **dropped/deferred**: measured ≤1% or unexercised.

Each optimization ticket obeys `AGENTS.md` §2: fast path bit-identical (or
tolerance-named) to the serial baseline, regression tests locking
equivalence, numbers logged in `docs/results.md`.

## `docs/roadmap.md` pointer (implementation writes)

Short "Performance" note: the measured baseline exists (link to the
results.md entry), top phases named, and the ticket slug list above as
candidate order — consistent with the file's "candidates, not commitments"
style.
