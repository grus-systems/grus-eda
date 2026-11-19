# mc-model — Overview

First implementation ticket of the multi-clock sequence
(`docs/multi-clock.md` §3, measured by `multi-clock-probe`). Replace the
single-clock `Sdc` model with a per-domain clock table; eval stays
single-domain and refuses a multi-clock SDC loudly.

## In scope

- `Sdc` keeps `clocks: Vec<Clock>` — one row per `create_clock`, in file
  order. Port-bearing rows are design clocks; `port: None` rows are
  virtual I/O clocks. Every clock attribute (period, network + source
  latency, setup/hold uncertainty, transition, propagated flag) lives on
  the row — the shared `io_clock_*` / single-slot fields go away.
- `ClockRef { Design, Virtual }` is replaced by `ClockId` (a row index).
  `DelayRule.clock` becomes `Option<ClockId>`; `None` keeps today's
  `-clock`-absent meaning "the design clock" (first port-bearing row).
- `create_clock` accepts N design clocks (the
  `a second design clock is not supported` guard is removed) and any
  number of virtual clocks, each with its own period — the `finish()`
  "virtual period must equal design period" check goes away.
- `set_clock_uncertainty` / `set_clock_latency` /
  `set_clock_transition` / `set_propagated_clock` resolve names to rows
  and write the rows. Multi-name `get_clocks {a b}` lists are accepted
  and apply to every named row.
- **Single-domain gate**: `run_sta`, `run_sta_quiet`, `sta_pin_slews`
  (everything through `run_sta_detail`) bail when the SDC declares more
  than one design clock, before `prepare_sta` runs. Named error; the
  verbatim message goes into `docs/results.md`.
- `PortSel::matches` takes the `Sdc` (or an equivalent clock-port
  predicate) so `-no_clocks` excludes every design-clock source port,
  not just the first.
- Tests: rewrite `multi_clock_bails` for the new accepted/still-bailing
  surface, replace the bug-pinning
  `uncertainty_on_virtual_clock_writes_design_fields` with a regression
  test that the value lands on the virtual clock's row, add a
  two-clock `sta` bail test, update the field reads in existing tests.

## Out of scope (later tickets)

- `mc-eval`: domain tagging, per-port clock-net seeding, cross-domain
  pairing. Eval in this ticket stays single-domain.
- `mc-xdomain`: inter-clock `-from`/`-to` `set_clock_uncertainty`
  (still bails), edge-pair search.
- `mc-selectors`: clock-object selectors in exceptions (still counted
  in `ignored_commands`), `set_clock_groups` (still an unsupported
  command), `create_clock -waveform` (still unsupported option),
  `set_clock_latency -early`/`-late` (still unsupported option),
  multi-pin `create_clock` source lists (still
  `create_clock takes one source port`).
- The single-clock-SDC-on-multi-clock-DEF silent fold (a register on a
  net the declared clock never reaches still gets the domain-0 seed).
  Detecting it needs domain tags — `mc-eval` owns it. This ticket only
  gates the SDC side (declared second domain). It must stay recorded as
  a remaining hazard under `docs/multi-clock.md` §2's still-unsupported
  list — one explicit line (wording in steps.md §8). The pre-`mc-model`
  §2 carried this bullet; do not drop it when rewriting the section.

## Golden-identity invariants

Every existing golden must be bit-identical. On a single-design-clock
SDC the table holds exactly the values the old fields held, so all
accessors must return identical numbers:

- `-clock`-absent delay rules → design row; `-clock <name>` → that
  row. `latency_ns(id)` = `source_latency` when the row is propagated,
  else `latency + source_latency` — identical to the old
  `ClockRef::Design`/`Virtual` arms (virtual rows are never propagated).
- `set_clock_latency`/`set_clock_uncertainty`/`set_clock_transition`
  without a clock argument keep writing the design clock (first
  port-bearing row).
- `resolve_clock` keeps its matching rule: `name` first, and the
  port-name fallback only inside `propagated_clock` (matching
  `get_ports {clk}` to a design row) — it must stay absent from
  `resolve_clock` itself so `set_clock_latency 1.0 clk` on a `-name`d
  clock still fails `unknown clock`, as today.
- `set_propagated_clock` on a virtual row still bails;
  `unset_propagated_clock` on a virtual row stays a no-op.

Two intended behaviour changes (no fixture SDC exercises them; record
in `docs/results.md`):

- `set_clock_uncertainty … [get_clocks vclk]` now writes the virtual
  row instead of the design fields — the probe's silent wrong-target
  bug, fixed structurally.
- `set_clock_transition` on `{design, virtual}` now stores the
  transition on both rows instead of counting
  `ignored_options["set_clock_transition (virtual clock)"]`. The entry
  disappears from the skip summary; `--strict` on such an SDC now
  passes (the command is modelled, not dropped). No fixture SDC sets
  transition on a virtual clock — the `ignored_options` is empty today
  on every golden.

## Risks

- `design_clock()` must be total (return `Option` / `map_or` fallbacks)
  so `Sdc::default()` or mid-parse states never panic; `finish()` is
  the only production constructor and guarantees ≥ 1 design row.
- `all_clock_names()` order changes to file order (was: virtuals then
  design). All consumers are order-agnostic (per-name writes,
  `remove_from_collection` by name). Note it in the commit message.
- A `set_clock_latency`/`set_clock_uncertainty` with no clock target can
  run before the `create_clock` it means — today it writes the scalar
  fields unconditionally. With the table there may be no row yet: apply
  to the first port-bearing row *at apply time*; if none exists yet,
  bail `no design clock declared yet` (ordering-dependent SDCs that
  `finish()` would reject anyway). Record the new ordering constraint
  in `docs/results.md` and pin it with a test (tests.md §4).

## Files

- `crates/grus-core/src/sta/sdc.rs`
- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-eda/src/main.rs`
- `docs/multi-clock.md`
- `docs/results.md`
- `Cargo.toml`
- `Cargo.lock`
