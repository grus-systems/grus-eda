# e5-cts-tree — Tests

All in `cts/mod.rs` `#[cfg(test)]` (plus the CLI parse additions in
`main.rs`). Loaders via `test_support::{design_dir, sky130_hd, require}`;
`picorv32.placed.def` is gitignored but worktree-seeded — `require`
panics loudly if absent (no silent skips).

## T1 — Clock-root discovery (gcd)

`clock_roots` on `gcd.placed.def` + `gcd.cts.sdc`: exactly one root,
`clock_name == "core_clk"`, `root_net` resolves to `clk`, driver is
`( PIN clk )`, `sinks.len() == 35` and every sink pin is `_*_/CLK`.

## T2 — Tree structure + census (gcd)

`run_cts` on the same inputs:

- `sinks == 35`; `tree_depth == 2` or `3`; `inserted_instances` single
  digits (`<= 12`, and `!= sinks` — the "not one buffer per sink" gate).
- Walk the emitted topology: `clk` net is `{PIN clk, root_buf.A}`; every
  leaf-level net's non-driver members are original `CLK` sink pins; every
  buffer output is a net driver, every buffer input a net member —
  driver-first invariant I4 per net.
- `tree_buffers` + `clock_subnets` consistency: every created net's
  driver is an inserted buffer.
- Census printed for `docs/results.md` (the ours column of the
  reference-comparison table).

## T3 — DEF roundtrip + legality (gcd, picorv32)

`run_cts` → `write_def_full` to temp → `parse_def` (must not error) →
`verify_legality` (must pass). Assert `num_cells == 384 + inserted` /
`num_nets == 269 + clock_subnets` on gcd; cell/net counts on picorv32
analogous. Re-parse preserves `+ PLACED` status and positions for every
pre-existing cell (name-keyed compare).

## T4 — Byte-identity of non-clock DEF content (gcd)

Write the DEF **before** `run_cts` (same `write_def_full`), then after.
In the post-CTS file: (a) every COMPONENT line whose cell existed before
is byte-identical (pin-and-place legalization guarantees it); (b) every
NET line whose name is not the root net or a `clknet_*` is
byte-identical; (c) PINS section byte-identical. Control-net HPWL
unchanged follows from (a)+(b) since no existing pin moved or re-netted
— assert it anyway by re-computing the touched-nets HPWL on a name-keyed
basis. This is the acceptance's mutated/unmutated boundary, asserted on
the emitted text (the criterion's exact wording).

## T5 — Propagated-clock STA on the CTS DEF (gcd + picorv32)

Re-parse the emitted DEF → `estimate_placement_parasitics` with
`WireRc::from_layer(&tech, "met2")` → `load_sdc(gcd.cts.sdc)` (the
`set_propagated_clock` variant) → `run_sta_with_state` (the accessor
from `api.md`).

- Every original clock sink pin has a recorded `arrival_max` — i.e. the
  tree kept all 35 / 1,597 clock consumers reachable on the clock
  network. This is a hard assert: a disconnected sink would be an
  unreported functional break.
- Print (informational, not hard-gated): `latency_max − latency_min`
  (clock-pin arrival spread) and `latency_max` (insertion delay) vs the
  reference's 0.313–0.315 ns / −1.37 ps skew (gcd) and
  0.757–0.793 ns / −34.16 ps (picorv32). `docs/results.md` carries the
  measured numbers beside them.
- Soft bound assert: spread ≤ 0.5 ns on gcd — the "tens-of-ps band" is
  the expectation, but the strict `max(5 ps, 1%)` gate is ticket 4; a
  loose bound still catches a broken tree (e.g. one buffer per sink
  would land ~ns-scale skew). If the measured spread exceeds it, the
  bound is tightened to measured + margin and the value recorded —
  per §7.5, never silently loosened.
- Endpoint WNS/TNS printed for the log, not asserted (same deferred
  status as the reference run's e2e comparison).

## T6 — picorv32 census

`run_cts` on `picorv32.placed.def`: `sinks == 1,597`,
`tree_depth` in `3..=6`, `inserted_instances` in `100..=450` (the
reference's 253 is the center of the band; the band asserts the
league — depth-balanced with ~8 fanout — not a golden match). T3's
roundtrip covers legality on the same emitted DEF.

## T7 — Multi-root and error paths (synthetic or gcd)

- Two `create_clock` rows on the same port → one tree (dedupe by NetId).
- `clock.port` naming a non-existent io pin → `Err` mentioning the port.
- SDC with no design clock → `Err`.
- Root net whose driver is a cell pin (if a fixture exists cheaply;
  otherwise covered by code review + named as untested in results.md —
  §7.4: claim only what a test exercises).

## Fail-proof mutation (§7.7, recorded in results.md)

Corrupt the constructor in isolation and confirm the suite goes red —
at minimum: (a) move **all** sinks to group 0 (drop the partition loop's
grouping) → T2's buffer-count gate and T4's per-net member assertions go
red; (b) drop `connect_pin(b.out, m)` → every leaf net loses its driver,
`verify_legality`-adjacent net walk in T2 goes red. Record the observed
red lines, then restore.

## CLI tests (`main.rs`)

Parse tests: `cts` happy path; missing `--def-out`, `--sdc`, `--lib`
each error. E2E arm: `cts` on gcd produces `--def-out` that
`parse_def`+`verify_legality` accept and `--report` JSON that parses
with the census fields — acceptance criterion 1 at the binary surface.
