# Goal

Measure-first ticket for statistical activity propagation —
the last open power item (recorded as intentional divergence:
we use exact VCD/SAIF per-net activity, OpenSTA propagates
statistically via BDD/`findSeqActivity` when annotation is
partial or absent).

## Probe OpenSTA in the container (committed script + artifacts)

1. **No annotation at all**: `report_power` on a fixture with
   no `read_vcd`/`read_saif` — what activity does OpenSTA
   assume (default input toggle rate? which value? check
   `set_power_analysis`/default activity options in the build)?
2. **Combinational propagation**: annotate only input ports —
   how do probabilities/toggle rates propagate through
   AND/OR/XOR/inverter chains (exact boolean algebra or an
   approximation)?
3. **Sequential propagation** (`findSeqActivity`): annotate
   only clock + inputs — what rate does a register output get
   (does Q follow D's rate, a fixed default, or
   BDD-computed)? Feedback registers (counters, toggling
   flops) — does the BDD model capture the divide-by-2 rate?
4. **Partial annotation**: VCD covering only some nets —
   measured earlier ('back to propagated/default activity
   rather than zero'): confirm which nets fall back and to
   exactly what.
5. **x/z and no-change**: how unannotated constant nets
   (tie-hi/lo, undriven) are treated.

## Deliverable

`docs/results.md` entry + a sequenced implementation plan
(what to implement first: default rates → combinational
propagation → sequential rates; where it plugs into
`power.rs`'s `net_activity` slots). If the whole thing is
small enough for one ticket, implement it with golden
coverage; otherwise the plan lands as ticket-sized steps in
`docs/` and this ticket is measurement + plan only.

## Files

- `tests/designs/microbench/` or `tests/designs/gcd/` (probe
  scripts + committed artifacts)
- `crates/grus-core/src/power.rs` (only if implementation
  lands)
- `docs/results.md`, `docs/roadmap.md`
- `kurena/tasks/activity-propagation/` (design docs)
- `Cargo.toml` / `Cargo.lock`
