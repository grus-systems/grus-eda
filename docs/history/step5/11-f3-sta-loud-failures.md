# Step 5 ticket 11 — F3: STA stops dropping things silently

## Background

Review `docs/history/review-2026-11-11.md` §F3: our STA "silently drops what it
does not model". Parts of F3 are already fixed by later work (recovery /
removal endpoints in step-4 ticket 06, latch `dlxtn` endpoints with a
warning in ticket 10, the SDC Tcl subset with loud bails in ticket 09,
endpoint counts asserted against OpenSTA for all five designs). What
remains, verified against the current code:

1. `build_graph` (`crates/grus-core/src/sta/analysis.rs`) drops arcs into
   the catch-all `_ => {}` arm with no record: `TimingType::FallingEdge`,
   `SetupFalling`, `HoldFalling`, `Preset`, `Clear`, `Other` (see
   `lib_db.rs` for the variant list). A falling-edge register in a design
   would simply have no endpoint and no message. Arcs whose related/target
   pins do not resolve on the instance are dropped by a silent `continue`
   in the same loop.
2. `collect_endpoints` drops a check silently when it lacks a clock or a
   late/early arc pair (`late_arc()` / `early_arc()`), and drops an
   endpoint silently when no path reached it (either worst is `None`).
3. The clock is ideal, with no notice when the DEF in fact carries clock
   circuitry: a clock net (a net feeding register CLK pins or check clock
   pins) driven by a cell output — a buffer tree or, as in ibex, the
   clock-gating AND — is treated as an ideal source beyond the checks we
   do model.
4. `sta/sdc.rs`: a second `create_clock` with a source port silently
   overwrites `clock_port` / `period_ns` / `clock_name`. The review says
   fail on a second clock.
5. `sta/sdc.rs`: `positionals()` silently skips any `-flag` it does not
   know, so meaning-changing flags are parsed away: `set_input_delay
   -max`, `set_output_delay -min`, `-add_delay`, `set_clock_uncertainty
   -setup/-hold`. Across all five designs' SDC files the only flags used
   are `-clock`, `-name`, `-no_clocks`, `-period` (plus value flags
   `-lib_cell`/`-pin` already in `VALUE_FLAGS`), so a strict rule breaks
   no design.

The review's fix: count what was skipped and print the counts in the
summary line; fail on a second clock and on flags that change meaning.

## Files

- Modify: `crates/grus-core/src/sta/analysis.rs`
- Modify: `crates/grus-core/src/sta/sdc.rs`
- Modify: `crates/grus-eda/src/main.rs` (the STA summary line only)
- Do not touch: golden files, `tests/designs/**`, the tolerance tables in
  the STA test module.

## Work

### Skip accounting in the STA report

Add a `skipped` field to `StaReport` (serialized like the rest of the
report) with these counts, all `usize`:

- `unsupported_arcs`: arcs that reach the catch-all arm in `build_graph`,
  broken down by timing-type name — store as a sorted list of
  `(String, usize)` pairs or a `BTreeMap<String, usize>`; pick whichever
  serializes cleanly and say which you chose. The latch arm (`_ if
  lib_cell.is_latch`) is intentional modelling, not a skip: latch arcs
  that are not the enable-to-Q arc (e.g. D-to-Q) must not be counted.
- `unresolved_arcs`: arcs skipped because the related or target pin did
  not resolve on the instance.
- `incomplete_checks`: checks dropped in `collect_endpoints` because the
  clock or the late/early arc pair is missing.
- `unreached_endpoints`: checks dropped because no polarity reached the
  data pin (either worst is `None`).
- `cell_driven_clock_nets`: nets in the `clock_net` set whose driver is a
  cell output pin rather than a primary input (buffered or gated clock).

When any count is non-zero, `run_sta` prints one `eprintln!` warning line
per non-zero category (the existing latch warning in `build_graph` is the
style precedent). Do not change any arrival/slack arithmetic.

The CLI summary line in `crates/grus-eda/src/main.rs` (the `STA (...):
endpoints=...` line) gains the skip counts when any is non-zero, e.g.
`skipped: unsupported_arcs[clear=12, preset=12] unresolved=0 ...` — exact
format is yours, but every non-zero category must appear.

### SDC strictness

- A second `create_clock` with a source port bails
  (`a second design clock is not supported`). Virtual clocks (no source)
  stay as they are — the ibex SDC has one design clock plus one virtual.
- `positionals()` bails on any `-flag` that is neither in `VALUE_FLAGS`
  nor a negative number, naming the flag. This makes `-min`, `-max`,
  `-add_delay`, `-setup`, `-hold`, `-clock_fall` and friends hard errors
  in every command that routes through `positionals` / `first_number`
  (`create_clock`, `set_clock_uncertainty`, the delay/transition/load
  rules). Add unit tests in `sdc.rs` for: second design clock, and
  `-max` / `-add_delay` on `set_input_delay`, each asserting the parse
  fails. The five designs' SDCs must still parse (existing tests cover
  gcd and ibex).

## Acceptance

- Full test suite passes exactly as before on the designs available to
  you; endpoint counts and all STA comparison results are unchanged (this
  ticket adds reporting, not modelling).
- New `sdc.rs` unit tests pass: second design clock and meaning-changing
  flags are parse errors.
- `cargo clippy --all-targets -- -D warnings` exits 0.
- Report the skip counts you could not measure (this environment has no
  PDK): the maintainer measures them per design on Intel and records them
  in `docs/results.md`. If a category is non-zero on a current design,
  that is a finding to record, not a reason to widen anything.
- State explicitly in your report which `TimingType` variants can reach
  the catch-all arm after your change.

## Follow-up closure (`v0.1.47`, 2026-11-16)

- Added `SkipReport::unmapped_cells` in `crates/grus-core/src/sta/analysis.rs`
  for instances where `design.cell_lib_ids[c]` is `None` (`gcd.placed.def`
  has 93 `sky130_fd_sc_hd__tapvpwrvgnd_1` welltap cells with no Liberty entry).
- Replaced the `first_number` / `.find_map` positional scan in
  `crates/grus-core/src/sta/sdc.rs` with strict slice matching (`single_number`,
  `value_and_ports`) so non-numeric or extra positional words fail to parse.
