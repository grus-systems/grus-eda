# activity-propagation — Overview

## Goal

Measure-first ticket for OpenSTA's statistical activity propagation — the last
open item of roadmap N7 ("Still open: matching OpenSTA's activity
propagation"). Today `power.rs` treats every net missing from the VCD/SAIF
annotation as **zero activity** (`PowerCx.net_activity: Vec<Option<SignalActivity>>`,
`unwrap_or_default()`); OpenSTA instead seeds annotated pins and propagates
`{probability, density}` through combinational logic under a per-pin marginal
model (BDD) and gives register outputs `findSeqActivity` values. That model
difference — not a formula defect — is the recorded root cause of the gcd
`report_power` divergence (docs/results.md 2026-11-18 N7).

## Prior evidence this ticket builds on

- `docs/results.md` 2026-11-18 (VCD power): on `mb2.vcd` (clock-edge-aligned
  glitches on `a`) OpenROAD's combinational internal collapsed to 0 while our
  exact counts stayed — OpenSTA's propagated activity filters raw toggles.
- `docs/results.md` 2026-11-19 (SAIF): a `PORT`-group probe dropped annotation
  to `saif 3 / unannotated 5` yet the report was identical — "unannotated pins
  fall back to propagated/default activity rather than zero". Exactly *what*
  they fall back to was never measured.
- `docs/results.md` 2026-11-19 (SAIF `TC` sweep): Sequential internal did not
  follow `clk`'s `TC` — on clock nets the SDC clock rate wins over annotation.
- `docs/results.md` 2026-11-18 (gcd table): Sequential internal ours 1.56e-4
  vs OpenROAD 2.98e-4; Combinational ours 1.35e-4 vs 5.88e-5 — the size of
  the gap this propagation model must explain.
- `report_activity_annotation` prints a per-source pin split
  (`vcd N` / `saif N` / `unannotated M`) — free corroborating data on every
  session; capture it always.

## This ticket

1. A new discriminating fixture `tests/designs/microbench/mba.*` (fixture.md):
   a correlated and/or/inv/xor cone, one plain DFF (D annotated), a toggling
   flop and a 2-bit counter (feedback registers), a `conb_1` tie cell, and an
   undriven net — each propagation mechanism isolated into its own nets.
2. A committed container script `generate_activity_ref.sh` running the probe
   sessions of probe.md and emitting `mba.activity.reference.json` plus one
   filtered transcript — every committed number comes from the tool run
   (AGENTS.md §7 rule 1).
3. A dated `docs/results.md` entry recording the measured semantics verbatim
   (probe.md P0–P5, required content in docs.md).
4. Disposition — one of:
   - **Implement**: only if the measured model is small enough for one ticket
     (~50–150 lines + golden coverage on `mba`). Judged unlikely — the model
     has several independent mechanisms (defaults, combinational propagation,
     sequential defaults, annotation precedence).
   - **Sequenced plan**: ticket-sized steps committed in `docs/roadmap.md`
     (api.md carries the technical basis: where it plugs into `power.rs`'s
     `net_activity` slots, the resolution order, diagnostics deltas).

## Decision structure

- **Branch I (implement)**: only when the measured semantics reduce to one
  commit-sized rule set (e.g. "everything unannotated gets flat defaults, no
  real propagation" — contradicted by the mb2/gcd evidence, kept for
  honesty). Then `power.rs` gains the fill pass, `mba` gets a STRICT golden
  test, and the roadmap records Done.
- **Branch P (plan, expected)**: commit fixture + artifacts + results entry
  + the four-step plan of api.md §Plan in `docs/roadmap.md`. No code change.

Either way the committed artifacts are identical except for `power.rs`/
`main.rs` and the roadmap wording — the measurement is the deliverable.

## Honest scope notes

- **OpenSTA is GPL** (AGENTS.md §3): its source must not be read or copied.
  Everything below is probed inside the pinned container or taken from
  published docs/papers; the probe transcript is the spec.
- `report_power` aggregates may hide per-net activity. Probe step P0 hunts a
  per-pin activity accessor (`info commands`, `sta::` helpers); if one exists,
  P1–P5 become direct reads. If not, all numbers are *implied rates* inverted
  from `report_power` columns on nets whose loads are known — the inversion
  method is recorded beside each number (AGENTS.md §7 rule 2 spirit: measured,
  and the derivation named).
- The fixture's undriven net (`n_float`) and `conb` constants may contribute
  nothing measurable to `report_power` (zero density ⇒ zero switching); their
  provenance is still recorded from the annotation split, and the limitation
  is stated rather than silently dropped.
- VCD authoring pitfalls already learned: `$dumpvars` initials are mandatory
  (mb lesson — missing initials drop first edges), Tcl goes in files never
  stdin (`!` is history expansion — AGENTS.md §7 rule 3), and the committed
  script must regenerate byte-identically before commit (§7 rule 1).

## Files

- `tests/designs/microbench/` — new: `mba.def`, `mba.sdc`,
  `mba_ports.vcd`, `mba_ports_a2x.vcd`, `mba_full.vcd`, `mba_partial.vcd`,
  `mba_xz.vcd`, `generate_activity_ref.sh`, `mba.activity.reference.json`,
  `mba.none.openroad.txt` (filtered transcript, labelled as such)
- `tests/designs/README.md` — `mba` fixture line
- `docs/results.md` — dated probe entry
- `docs/roadmap.md` — N7 disposition (Done, or the sequenced plan)
- `kurena/tasks/activity-propagation/` — spec + these design docs
- `Cargo.toml`, `Cargo.lock` — `0.2.12` → `0.2.13`
- Branch I only:
  `crates/grus-core/src/power.rs`,
  `crates/grus-eda/src/main.rs`,
  optionally `crates/grus-core/src/activity.rs` + `crates/grus-core/src/lib.rs`
