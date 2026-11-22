# output-port-delays — Overview

## Goal

Land `set_max_delay`/`set_min_delay` checks whose `-to` selects an output
port — `docs/path-delays.md` section 5 item 2. Today those endpoints resolve
(`ExceptionEndpointKind::OutputPort`, `analysis.rs:2632`) but are counted in
`unsupported_path_delay_endpoints` and keep their ordinary output-delay clock
check (`analysis.rs:3182-3190`). This task measures the exact OpenSTA
semantics on the committed `sta_exceptions` fixture, then composes the delay
check on the covered side the same way FF endpoints already do.

## Correction to the spec's fixture premise

The goal text says "`y`'s fanin cone reaches both `u_ff0`/`u_ff2` through
`u_mux`/`u_ff1`". That is not how the timing graph works: `u_ff0 -> u_mux ->
u_ff1/D` terminates at the `u_ff1/D` check endpoint — a flip-flop breaks the
path. Net `y` is driven by `u_ff1/Q` alone, so **`y` has exactly one launch
source (`u_ff1/Q`)** and two per-transition candidates. Consequences:

- `-from [get_cells u_ff0]` / `-from [get_cells {u_ff0 u_ff2}]` cover
  **nothing at `y`** — the delay rule is not in `u_ff1`'s signature class.
  The spec's `{u_ff0 u_ff2}` probe is a pure negative, redundant with the
  single `-from u_ff0` negative probe. The design replaces the two-source
  probe with `-from [get_cells {u_ff0 u_ff1}]`: a multi-object `-from` where
  one object covers `y`'s launch and one does not — the case that actually
  exercises mixed `-from` coverage at a port.
- The spec's "which candidate does OpenSTA report if an output port has
  multiple launch candidates" question **cannot be exercised on this
  fixture** without a DEF change (a second driver of `y`). With one launch,
  the worst-covered-candidate rule is exercised only per-transition —
  recorded honestly in `docs/path-delays.md`/`docs/results.md` like the
  unexercised CRPR case, not claimed as verified.

## Current code shape (what changes)

`run_sta_partitioned` composition loop (`analysis.rs:3129-3306`) iterates
`ctx.ep_kinds` — which already contains `y -> OutputPort` — and per
(class, endpoint) collects `delay_max_n`/`delay_min_n` from the class
signature's rules whose `to` covers `ep`. For `y`, the class carrying
`u_ff1/Q` yields `EndpointPolarities` from `collect_endpoints`' port branch
(`analysis.rs:3914-3960`):

```
req_max = output_clock_latency + period - output_delay_max - setup_unc
req_min = output_clock_latency - output_delay_min + hold_unc
arr     = covered class's ordinary arrival_min/max at the port pin
```

With `output_clock_latency = 0.7` ideal (0.1 source + 0.6 network) / `0.1`
propagated (source only, `sdc.rs:330`), and `output_delay = (0.1, 0.7)`,
`setup_unc = 0.05`, `hold_unc = 0.1` from the shared preamble.

The two guarded spots to change:

1. `delay_supported` (`analysis.rs:3182-3185`) — currently
   `Check { latch: false }` + a setup/hold arc. Extend to `OutputPort`.
   Clock gates, latch-D, and arc-less checks stay unsupported.
2. The `delay_max_n`/`delay_min_n` blocks (`analysis.rs:3194-3247`) —
   `check_by_pin.get(&ep)` returns `None` for ports today, so the max block
   silently no-ops and the min block adds `n` to the *ordinary* port req
   (never reached — `delay_supported` clears both first). Restructure to
   branch on `ExceptionEndpointKind`: the `Check` arm keeps the existing
   capture-anchored formula; the new `OutputPort` arm replaces `req`/`slack`
   per the measured port formulas while keeping the candidate's `arr`/`slew`.

No signature/partitioning changes: `-from` coverage, file-order last-wins,
false-path drop and the `delay_*_n.is_none()` MCP guards already work per
(class, ep) and are endpoint-kind agnostic.

## What the probes must pin down (before touching code)

Measured ad hoc (`md_toy*`, `docs/path-delays.md` section 1): max side
`req = N - output_delay_max - setup_uncertainty` (edge reference 0);
min side `req ~= N` with the additive constant unknown. On this fixture
`output_delay_min = hold_uncertainty = 0.1`, so plausible constants
**cancel** — the raw `N = 0` reading cannot distinguish
`req = N` from `req = N - output_delay_min + hold_unc` (both give 0.0).
`probes.md` includes two attribution probes (`mdo_mindl`, `mdo_minhu`) that
vary one term at a time, plus `pmo_*` variants to decide whether the
propagated-clock anchor stays at edge 0, moves to `-source` latency (0.1),
or moves to the propagated `clk` port-pin arrival.

## Non-goals

- No DEF or preamble changes — `probes.md` only adds standalone scenario
  files and preamble *values* local to two attribution files.
- No tolerance changes; strict rel 1 % / abs 5 ps stays.
- No multi-driver `y` fixture; the multi-launch winner question is recorded
  as unexercised, not solved.
- No changes to `resolve_to`/`PortSel` (`all_outputs` already resolves,
  `sdc.rs:26`, `2794-2800`) or the signature machinery.
- `test_sta_exception_diagnostics` needs no new asserts; `fp_to_y` already
  covers port removal and the delay-not-unsupported count is exercised by
  the golden match itself. Only its doc-comment scenario count is stale.

## Files

- `tests/designs/sta_exceptions/mdo_max05.sdc` (and the other `mdo_*.sdc` /
  `pmo_*.sdc` probes listed in `probes.md` — 23 new files)
- `tests/designs/sta_exceptions/generate_ref.sh`
- `tests/designs/sta_exceptions/exceptions.reference.json`
- `crates/grus-core/src/sta/analysis.rs`
- `docs/path-delays.md`
- `docs/results.md`
- `docs/timing-exceptions.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
