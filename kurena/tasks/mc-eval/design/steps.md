# mc-eval — Implementation steps

Order chosen so each step compiles and keeps `cargo test --workspace`
green before the next lands. Verify between steps with
`cargo test -p grus-core sta` (fast loop) and the workspace gate at the
end.

## Step 1 — domain model scaffolding (no behaviour change)

- `PinTimingState` + `dom_max`/`dom_min`; `merge()`/`merge_all` signature
  updates at every call site (contribute `0` for now where no tag
  exists yet — ports and launches still untagged).
- `LaunchSeed.dom`, `PortSeed.dom` fields; `StaPrepared.clock_domain`;
  `clock_parity` Option → Vec.
- `sdc.rs`: `input_clock_row`, `output_clock_row` (public, wrap
  `delay_at`'s `Option<ClockId>`; absent → first design row index).
- `ideal_clock_parity` returns `(parity, domain)`, seeds every design
  row's port; `propagate_clock` seeds every design row; `build_graph`
  clock-net marking from every design port net.
- `prepare_sta`: `clocks.len() > 8` bail; consumer seeding switched to
  per-row (`dom`/`propagated` gate, `clocks[dom]` latency/period/
  transition); `cell_driven_clock_nets` moves here (api.md §5);
  `clock_parity` per-pin merge.
- `SkipReport.cross_domain_endpoints` + `SkipReason::CrossDomain` +
  `nonzero`/`endpoints_skipped`/`is_clean`/`merge_skip_reports`.

At this point single-domain goldens must still be byte-identical —
run the full sta test suite before continuing.

## Step 2 — tagging through eval

- `eval_sta`: port seeds contribute `1 << dom` (fold via
  `launch_domain_tag(input_clock_row)`); launch seeds tag
  `clock_domain[clk]`; `dly`/`arr` merges propagate `src.dom_*`.
- Latch borrow: `clocks[clock_domain[info.enable]]` for period +
  setup uncertainty.
- `collect_endpoints`: `cap` from `clock_domain[check.clk]`; per-domain
  `capture_extra`, latch wrap, uncertainties, `credit_for` gate,
  opening-edge req; `same_domain` predicate + whole-endpoint `CrossDomain`
  deferral; `NoClock` short-circuit on `u8::MAX` tags.
- Output ports: anchor row via `output_clock_row` → anchor-row req
  terms; `same_domain` vs `launch_domain_tag(anchor)`.
- Gating enables: `clock_domain[g.clk_pin]` capture + predicate.
- `eval_pulse_width_checks`: `clocks[clock_domain[pin]].period_ns`.
- `run_sta_partitioned`: delay-check anchors per capture row (incl.
  latch-D `close` wrap, `credit` gate), MCP `cap.period_ns` (Check vs
  OutputPort anchor), output-port `-to` delay checks on anchor row;
  skip-pass `CrossDomain` re-diagnosis on merged `dom` masks.
- Delete the `design_clock_count() > 1` bail in `run_sta_detail`.

## Step 3 — fixture & reference regeneration

- Add `t2_mcps.sdc`, `t2_mds.sdc`, `t2_latch.sdc`,
  `two_clock_latch.def` (fixture.md §2–3).
- Extend `probe.sh` (fixture.md §4); run it as committed inside the
  container; `git diff` — existing artifacts byte-identical, new
  entries additive.
- `tests/designs/README.md` entry update.

## Step 4 — tests

- `compare_sta_clocks` + the 12-scenario matrix (tests.md §2).
- Parse-bail integration pins (`t2_uncx`, `t2_grp`).
- `one_clock_on_two_clock_def` hazard pin.
- Delete `sta_bails_on_a_two_clock_sdc`.

## Step 5 — fail-proof

- Mutation: `cap` read forced to `design_clock()` → `two_clock_int`
  red (req `0.885446` → `1.885446`); restore; record both runs.
- Optionally also `same_domain == true` mutation → count assertion red.

## Step 6 — docs & version

- `docs/multi-clock.md`: status line `mc-eval` landed; §3 item 2 ticked;
  §2 rewritten (supported surface + still-open: xdomain pairing
  `mc-xdomain`, `-waveform`, `set_clock_groups` `mc-selectors`,
  inter-clock uncertainty `mc-xdomain`).
- `docs/results.md`: dated entry — commands run, per-scenario
  same-domain match table (endpoint counts + worst diff), deferral
  counts verbatim (`cross_domain_endpoints: 3` + named pins from CLI
  `--json` skip summary), parse-bail verbatim strings, fail-proof
  evidence, the two intended non-golden-visible changes (overview
  §"Golden-identity invariants").
- `docs/roadmap.md`: sequence progress note (mc-model, mc-eval done).
- `Cargo.toml` `0.2.0` → `0.2.1` (0.x feature → patch per repo rule;
  `cargo build` refreshes `Cargo.lock`).

## Verify

- `cargo test --workspace` — all existing goldens byte-identical.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- `sta --strict` on `two_clock.def`/`t2_int.sdc` fails loudly
  (`cross_domain_endpoints` nonzero → `is_clean` false) — record the
  output verbatim in results.md.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/sdc.rs`
- `tests/designs/sta_clocks/`
- `tests/designs/README.md`
- `docs/multi-clock.md`
- `docs/results.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
