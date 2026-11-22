# First large-design run — feedback (2026-11-22, v0.3.8)

Context: first end-to-end exercise of `floorplan` → `place` → `sta`
on a real block rather than the bundled test designs — a ~132k-cell
gate-level netlist (Yosys JSON, `sky130_fd_sc_hd`) with multiple
asynchronous clock domains. Design details are intentionally
omitted; the numbers below are what mattered for tool feedback.

## What worked

- `floorplan` accepted the Yosys JSON directly (mangled
  `$paramod$...\top` name passed via `--top` worked unchanged) and
  produced a DEF in seconds.
- `place` converged and legalized 132,382 movable cells in ~7 s
  (final HPWL ~4.68M µm, `verify_legality` clean by construction).
- `sta` evaluated 49,491 endpoints / ~371k arcs with placement-RC
  parasitics and returned a clean skip summary (107 unreached
  endpoints — output ports with no timed path; 64 `false_path`
  endpoints from the async clock grouping; `unmapped_cells`,
  `unresolved_arcs`, `incomplete_checks` all 0).
- Pulse-width checks ran: 26,450 checks, 0 violations.

Result shape: a single deep setup violation on **one** endpoint out
of 22,870 timed endpoints (arrival more than 3× the required time);
every other endpoint positive with comfortable margin; hold clean.
That single long path is a deep logic cone, i.e. exactly the kind
of finding this stage exists to surface.

## Improvement requests

Ordered by value for this kind of use:

1. **Critical-path detail in `sta` output.** `--json` reports
   per-endpoint arrival/required/slack but no path. For the
   deep-slack outlier I could not tell which logic cone it belongs
   to without external netlist spelunking. A `--paths N` (or
   `critical_paths` in the JSON) dumping startpoint→endpoint with
   cell/pin names and per-arc delays would make timing findings
   actionable.
2. **Per-clock-domain slack summary.** With multiple async clocks, a
   WNS/TNS/violation-count table per `create_clock` domain would
   show at a glance which domain broke. (The run mixed both
   domains' endpoints in one number.)
3. **Slack histogram.** Bucketed endpoint counts (e.g.
   <0, 0–20%, 20–50%, >50% of period) give an instant
   timing-closure read without parsing 22k rows.
4. **`--top` ergonomics.** Requiring the exact mangled
   `$paramod$hash\name` string is fragile-looking; auto-detecting
   a single top module or accepting a suffix match would help.
   (It worked as-is — cosmetic.)
5. **SDC examples/templates.** A short doc section with minimal
   SDC for common topologies — single clock; N async clocks +
   `set_clock_groups`; FF clock-enable gating (where the "gated"
   clocks are enables, not clock nets) — would have saved
   guessing at the right constraint shape for this design.

## Caveats on the numbers themselves

- This was one run on one design at `tt_025C_1v80` with
  placement-RC parasitics and zero input delays — an estimate,
  not sign-off, consistent with the project's stated scope.
- The failing path was **not** cross-checked against OpenSTA on
  the same netlist. For anyone citing grus-eda STA numbers as
  evidence, running the same DEF/netlist+SDC through the
  LibreLane `sta`/`openroad` binaries and recording the delta in
  `docs/results.md` would be the honest comparison; this run's
  inputs are not public, so the check needs either private access
  or a comparable public design.

## Untested here

`congestion`, `power`, `check-e0`, `--spef`, other corners, and
the new 0.3.8 timing-exception commands (`set_max_delay` /
`set_min_delay` / pin-level selectors) were not exercised.
