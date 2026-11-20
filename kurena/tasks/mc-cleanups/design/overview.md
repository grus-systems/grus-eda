# mc-cleanups — Overview

Cleanup bundle for the low-severity findings left over from the
multi-clock sequence (`mc-model` … `mc-golden`, v0.2.x) plus adjacent
docs/power nits. Every item is mechanical or needs at most one
container run to pin OpenSTA behaviour. No semantics change beyond the
listed items; all existing goldens stay byte-identical.

Current version: workspace `0.2.6`. This ticket bumps to `0.2.7` (patch —
fixes and internal cleanups) in the same commit as the functional change,
per `AGENTS.md` §5.

## Item inventory

| # | Item | Kind | Where |
|---|------|------|-------|
| 1 | `grid_lcm` dead `y <= tol` guard | code | `analysis.rs` ~L5093 |
| 2 | empty clock list on `set_clock_*` | code + probe | `sdc.rs` `plain_names`/`clock_rows`/`set_clock_*` |
| 3 | pin `-from` delay-rule overwrite doc | comment | `analysis.rs` ~L4019, `ResolvedPathDelay::from_pins` ~L3562 |
| 4 | PROBE_ONLY asserts the real parse bail | test | `analysis.rs` ~L9399 |
| 5 | `-waveform` post-wrap validation | code + probe | `sdc.rs` ~L1071 |
| 6 | stale names/comments (test fn, hold-pairing comment, `skipped_endpoints` doc) | comment | `analysis.rs` ~L9171, ~L4600, ~L353 |
| 7 | `-to clk2` dead-but-resolved: warn/count per measurement | code + probe | `analysis.rs` `resolve_to` ~L4700 |
| 8 | port-vs-clock namespace priority for a bare name | code + probe | `analysis.rs` `resolve_from`/`resolve_to`/`resolve_delay_from` |
| 9 | `-clock`-less `set_input_delay` under `-waveform` | code + probe | `analysis.rs` ~L2449, `sdc.rs` `input_clock_row` ~L543 |
| 10 | `timing-exceptions.md` skip counters | doc | `docs/timing-exceptions.md` |
| 11 | `results.md` command/count entries | doc | `docs/results.md` |
| 12 | `driven_nets_without_activity` meaning | doc | `power.rs` ~L49 |
| 13 | README fixture list | doc | `tests/designs/README.md` |
| 14 | `t2_mdx` description in `design/fixture.md` | doc (verify) | `kurena/tasks/mc-xdomain/design/fixture.md` |
| 15 | SAIF nits (DURATION, negative label, Latin-1) | code + doc | `readers/saif.rs` |
| 16 | version bump | manifest | `Cargo.toml`, `Cargo.lock` |

## Probe plan (one container run)

Items 2, 5, 7, 8, 9 share a single measurement run through the existing
`tests/designs/sta_clocks/probe.sh` harness. A new `MEASURE_ONLY` list is
added next to `PROBE_ONLY`: its scenarios run `run_report` only, so no
`ref_*.json` block is folded into `two_clock.reference.json` — the golden
stays byte-identical by construction. New scenario `.sdc` files and their
`probe_*.txt` outputs are committed as evidence. See `probes.md` for the
scenario list, the discriminator for each, and how results get recorded.

## Non-goals

- No new SDC commands, no new selector namespaces, no broadening of
  `resolve_clock` beyond the measured priority rule.
- No change to the LCM pairing model, the deferral conditions, or the
  endpoint table — the probe-driven fixes touch diagnostics, parse
  validation, and at most one launch-anchor constant.
- `t2_wave4`-style >2-instant waveforms stay a named bail (model limit).
- PROBE_ONLY scenarios stay out of the golden; MEASURE_ONLY scenarios
  never enter `SCENARIOS`.

## Spec-vs-reality corrections found during design

- **`sta_evaluates_two_clock_same_domain` is already renamed** — the
  function is `sta_evaluates_two_clock` since v0.2.3 and it does assert
  cross-domain pairing. Item 6's first bullet reduces to an audit: grep
  for the stale name and any "same-domain" misnomer in comments/docs,
  fix or record as a no-op.
- **`[get_clocks {}]` does not silently no-op today** — `plain_names`
  bails on an empty name list, `get_clocks` yields `Value::Opaque`, and
  every `set_clock_*` arm rejects `Opaque` with a shape bail. The silent
  zero-row path is `Value::Clocks([])`, reachable through
  `[remove_from_collection [all_clocks] [get_clocks cka]]` or
  `[all_clocks]` before the first `create_clock`: `set_clock_latency`/
  `-transition`/`-uncertainty` iterate zero rows and write nothing,
  while `set_propagated_clock` bails on the empty name vec. The item is
  to unify this surface per the measured rule.
- **the `t2_mdx` fixture description is already correct** —
  `kurena/tasks/mc-xdomain/design/fixture.md:57` says "pin-startpoint
  `-from`", not "delay-check rewrite on both sides". Item 14 reduces to
  a grep audit (`rewrite`, `both sides`) across `kurena/tasks/` and
  `docs/`, fixing any residual stale phrasing; likely a no-op recorded
  honestly.
- **the saif-power entry already contains a `cargo test --workspace`
  line** (`docs/results.md` ~L3144). Item 11's saif half is an audit
  against sibling entries' "Checks run" convention; add only lines that
  are genuinely absent and were actually run — never fabricate a
  command record.

## Measured rules already in evidence

From `docs/results.md` / `docs/multi-clock.md` / `probe_t2_*.txt`:

- `t2_selport`/`t2_selpto`: a bare `-from`/`-to` name resolves against
  object namespaces first; a clock's *source-port* name resolves to the
  port object, not the clock. `-from clk1` is a resolved-but-dead rule —
  OpenSTA warns nothing and the rule covers nothing. `-to clk2` resolves
  the same way and warns `STA-1551 'clk2' is not a valid endpoint`.
- `[get_clocks nope]` in an exception position warns (STA-1520-class)
  and produces a dead rule; in `set_clock_groups -group` and clock
  commands, an unknown name errors.
- `-waveform` instants past the period are wrapped (`t2_wavebig`
  `{0 2.5}` on 2.0 → `{0 0.5}`, reproducing `t2_int`).
- `t2_mdx` pin `-from` is a path startpoint; last-wins per-kind
  overwrite of same-kind covering rules is measured (`md_overlap*`).

Open questions the probes must settle are enumerated in `probes.md`.

## Verification gates (from `workflow.toml` + repo rules)

1. `cargo test --workspace` — all tests pass, including the tightened
   PROBE_ONLY asserts and any new unit tests.
2. `cargo clippy --workspace --all-targets -- -D warnings` — clean.
3. `git diff` on `tests/designs/` goldens after any `probe.sh` re-run:
   `two_clock.reference.json`, `two_clock_latch.reference.json`, and all
   pre-existing `probe_*.txt` byte-identical; only *new* files appear.
4. Fail-proof: for each behaviour change backed by a test, break it on
   purpose (e.g., revert the post-wrap check) and confirm the test goes
   red; restore. Record in `docs/results.md`.
5. `docs/results.md` gets a dated entry: which probe lines ran, what
   OpenSTA did, which rules were implemented, which items were no-ops.
6. Heavy gate (optional but expected for STA-path changes):
   `cargo test --release -p grus-core sta`.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/sdc.rs`
- `crates/grus-core/src/readers/saif.rs`
- `crates/grus-core/src/power.rs`
- `tests/designs/sta_clocks/probe.sh`
- `tests/designs/sta_clocks/t2_eclk.sdc` (new)
- `tests/designs/sta_clocks/t2_nsfrom.sdc` (new)
- `tests/designs/sta_clocks/t2_nsto.sdc` (new)
- `tests/designs/sta_clocks/t2_toinv.sdc` (new)
- `tests/designs/sta_clocks/t2_noclk.sdc` (new)
- `tests/designs/sta_clocks/t2_waverot.sdc` (new)
- `tests/designs/sta_clocks/t2_wavez.sdc` (new)
- `tests/designs/sta_clocks/probe_t2_*.txt` (new outputs)
- `docs/timing-exceptions.md`
- `docs/results.md`
- `docs/multi-clock.md`
- `tests/designs/README.md`
- `Cargo.toml`
- `Cargo.lock`
