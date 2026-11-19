# multi-clock-probe — Overview

## Goal

Measure-first ticket for multi-clock SDC support. Two outputs:

1. A dated `docs/results.md` entry recording (a) exactly where a
   two-clock SDC breaks in grus-eda today — command, behaviour, site —
   and (b) OpenSTA's two-clock rules measured inside the LibreLane
   container: launch/capture pairing across domains, per-clock
   uncertainty/latency, the `set_min_delay`/`set_max_delay` capture
   anchor on a second-domain endpoint (the last open item in
   `docs/path-delays.md` §5), and the capture-edge search horizon for
   non-integer period ratios.
2. A sequenced, ticket-sized implementation plan — unless the probe
   shows the gap is small enough to close inside this ticket (it is
   not expected to be; see below).

No functional change to `sdc.rs`/`analysis.rs` is planned. The
committed diff is fixture + probe script + docs (+ optionally one
regression test that locks the documented break).

## Context

- The timing model is single-clock: `Sdc` carries one
  `clock_name`/`clock_port`/`period_ns` triple and one
  uncertainty/latency/transition set (`sdc.rs:241-289`), and eval
  reads `sdc.period_ns`/`sdc.clock_port` directly at every
  capture-edge computation (`analysis.rs` — the audit in
  `breakpoints.md` lists all sites).
- The spec text says a second `create_clock` "silently overwrites the
  first (`sdc.rs:863`)". That was true before `e921217` (F3); the
  guard added there now `bail!`s with `a second design clock is not
  supported` (`sdc.rs:863-866`). The probe records the *actual* break
  at HEAD — a loud parse bail — not the stale description. A scratch
  build with the guard removed is still worth one run to document the
  overwrite shape the guard prevents (see `steps.md`).
- `docs/path-delays.md` §5 item 1 is a literal two-clock-domain
  `set_min_delay` fixture — blocked exactly on this capability.
  `docs/roadmap.md` already lists "a second `create_clock`" as out of
  scope (R1 area) and `docs/timing-exceptions.md` notes clock-object
  selectors go to `ignored_commands` because launch keys carry no
  clock attribution.

## Scope decision rule

Close the gap inside this ticket only if the probe shows a fix of the
ticket's size budget (~50-150 lines, one commit) — e.g. the eval
already pairs domains correctly and only the parse guard stands in
the way. The audit in `breakpoints.md` makes that outcome implausible
(domain attribution is absent from `Sdc`, launch seeds, parity
tables, and required-time composition), so the expected deliverable
is the sequenced plan. If the probe surprises us, the fallback is
allowed but must come with golden coverage.

## Deliverables

- `tests/designs/sta_clocks/` — new hand-written fixture:
  `two_clock.def` (two clock ports, two FF groups, cross-domain paths
  in both directions), `t2_*.sdc` scenario files, `probe.sh`
  (container harness mirroring `sta_exceptions/generate_ref.sh`), and
  the generated probe artifacts (`two_clock.probe.json`,
  `probe_*.txt`), all small enough to commit.
- `tests/designs/README.md` — catalog entry for the new fixture.
- `docs/results.md` — dated entry: the break-point table and the
  measured-rules table, each number traced to a committed file.
- `docs/multi-clock.md` (new) — the measured rules restated as a spec
  plus the sequenced implementation tickets; `docs/roadmap.md` gets a
  pointer line under the path-delay "Remaining" item. (If the plan
  turns out short, it may live in `roadmap.md` directly and the new
  doc is skipped — decide after the numbers are in.)
- `docs/path-delays.md` §5 — item 1 updated with the measured
  capture-anchor answer and a pointer to the plan; it stays
  implementation-open.
- Optional `sdc.rs` parse test locking the documented bail (golden
  proof the break is where we say it is; cheap).
- Patch version bump in the same commit as the docs (spec: bump patch
  when doc/code changes land).

## Non-goals

- No multi-clock eval implementation, no `Sdc` model change (unless
  the small-gap case materialises).
- No changes to `sta_exceptions` — its DEF and goldens stay
  untouched; a second clock port needs a new DEF anyway.
- No `create_generated_clock`, `set_clock_groups` implementation, or
  `-waveform` support — measured only, where cheap (`t2_grp` informs
  the plan).
- No scratch/probe edits committed to `sdc.rs`/`analysis.rs`.

## Files

- `tests/designs/sta_clocks/` (new directory)
- `tests/designs/README.md`
- `crates/grus-core/src/sta/sdc.rs` (regression test only; no production change expected)
- `docs/results.md`
- `docs/path-delays.md`
- `docs/roadmap.md`
- `docs/multi-clock.md` (new)
- `kurena/tasks/multi-clock-probe/`
- `Cargo.toml`
- `Cargo.lock`
