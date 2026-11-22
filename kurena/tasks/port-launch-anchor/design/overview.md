# port-launch-anchor — Overview

## Goal

Close `docs/path-delays.md` section 5 item 3: the propagated-clock launch
anchor for input-port delay sources is unverified. `md_port` /
`md_minport` (`set_max_delay` / `set_min_delay` `-from [get_ports d_in]`)
cover the ideal-clock anchor only. Add `pmp_*` scenarios — the `md_port`
set plus `set_propagated_clock` — to `tests/designs/sta_exceptions/`,
regenerate the golden, and reconcile with our anchor.

## The question

A delay check keeps the covered candidate's ordinary arrival
(`analysis.rs:3251-3255`, `arr_reported = the class's ordinary max
arrival`). For a port source that arrival is the port seed
(`analysis.rs:2139-2151`): `latency + in_max/min + drv`, where
`latency = sdc.input_clock_latency_ns(port)` resolves through
`Sdc::latency_ns` (`sdc.rs:324-334`). Under `propagated_clock` the
`ClockRef::Design` arm returns `clock_source_latency_ns` (0.1) — the
ideal network latency (0.6) is dropped. Under `set_propagated_clock`,
what does OpenSTA anchor the input-port launch to?

## Hypotheses and predicted numbers

Preamble constants: `set_clock_latency -source 0.1`,
`set_clock_latency 0.6`, `set_input_delay -max 0.7`/`-min 0.0` on `d_in`,
`-max 0.3`/`-min 0.1` on `sel`. Under propagation the FF clock-pin early
arrivals measured in `base_prop` are `u_ff0` 0.169918, `u_ff1` 0.183785,
`u_ff2` 0.240998 (derived: `min_req - unc_hold - t_hold`,
`t_hold = -0.035445` from `base`).

| hypothesis | anchor value | predicted `d_in` launch (max / min) |
|---|---|---|
| H-src — SDC `-source` latency (our impl) | 0.1 | 0.8 / 0.1 |
| H-prop — propagated arrival at the `clk` port pin | 0.1 | 0.8 / 0.1 |
| H-ideal — source + network latency | 0.7 | 1.4 / 0.7 |
| H-cap — propagated arrival at the capture FF's clock pin | ~0.17 (u_ff0) | ~0.87 / ~0.17 |

**H-src and H-prop are numerically identical on this fixture**: `clk` is
the propagation root, and a propagated clock's arrival at its root pin is
the source latency plus zero insertion delay. The golden therefore cannot
separate the two named candidates — state this honestly in
`docs/path-delays.md` (they are the same anchor under two names). What
the `pmp_*` golden *does* discriminate: the delay check sharing the
ordinary propagated port seed (H-src/H-prop, arr 0.8/0.1) versus the
ideal-latency anchor (H-ideal, 1.4/0.7) or a capture-pin-propagated
anchor (H-cap, ~0.87/~0.17). The `base_prop` golden already pins the
ordinary check to 0.8/0.1; `pmp_*` extends that proof to delay checks.

`pmp_port2` sharpens the separation: it reruns `pmp_port` with only the
network `set_clock_latency` changed (0.6 -> 1.1). Under H-src/H-prop
every row is byte-identical to `pmp_port` (network latency is inert under
propagation — `base_prop` already showed it absent from clock paths);
under any network-inclusive anchor `u_ff0/D.max_arr` jumps to 1.9. A
null-diff probe: identical predictions, maximal sensitivity. See
`scenarios.md` for per-scenario predicted rows.

## Scenario set

- `pmp_port` — `set_propagated_clock` + `set_max_delay 1.0 -from
  [get_ports d_in] -to [get_cells u_ff0]` (the `md_port` pair).
- `pmp_minport` — `set_propagated_clock` + `set_min_delay 0.5` on the
  same pair (the `md_minport` pair; predicted hold violation
  `-0.634473` under H-src/H-prop vs `-0.034` under H-ideal).
- `pmp_port2` — `pmp_port` with `set_clock_latency 1.1` (network-latency
  variant; predicted byte-identical to `pmp_port` under propagated
  anchors).
- `pmp_alls` — `set_propagated_clock` + `set_max_delay 1.0 -from
  [all_inputs]`. Covers `d_in` (delay checks at `u_ff0/D`, `u_ff2/D`) and
  `sel` (at `u_ff1/D` and `y2`); `clk` is inert — never a launch source
  in our model (`resolve_from` matches only `port_seeds`,
  `analysis.rs:2731-2739`; in OpenSTA a clock-root port is not a data
  startpoint). `y` stays uncovered. This exercises the port anchor at a
  skewed capture pin (`u_ff2/CLK` on `c3`, two buffers deep), the `sel`
  launch, and the edge-referenced output-port formula under propagation.

## Reconciliation

Expected outcome: `pmp_*` arrivals equal the `base_prop` port seeds —
our `input_clock_latency_ns` anchor is already right and no production
code changes. If the golden shows a different anchor:

- `Sdc::latency_ns` (sdc.rs:330) is the shared anchor for ordinary port
  checks too — a fix there would break `base_prop`. A divergence means
  the delay check needs its own anchor path (e.g. re-anchor the covered
  port candidate in the composition loop, `analysis.rs:3197-3255`), or a
  measured model limit recorded in `docs/results.md`. Fix in
  `analysis.rs`/`sdc.rs`, never by loosening the test.
- A result that matches neither hypothesis table above is a real finding:
  stop, decompose it per-transition, and record it.

## Non-goals

- No fixture changes: `exceptions.def` untouched, every `.sdc` preamble
  identical (except `pmp_port2`'s deliberate one-line variant).
- No tolerance changes: STRICT rel 1 % / abs 5 ps stays.
- No production-code changes unless the golden diverges.
- Truly separating H-src from H-prop needs nonzero insertion upstream of
  the clock root (a generated-clock / multi-root topology) — out of
  scope; document the degeneracy.
- The other section-5 items stay open (two-clock-domain min_delay,
  async-reset/latch fixtures, CRPR credit).

## Files

- `tests/designs/sta_exceptions/*.sdc`
- `tests/designs/sta_exceptions/generate_ref.sh`
- `tests/designs/sta_exceptions/exceptions.reference.json`
- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/sdc.rs`
- `docs/path-delays.md`
- `docs/results.md`
- `Cargo.toml`
- `Cargo.lock`
