# mc-selectors — Implementation steps

Ordered so every step keeps the tree green; the version bump and doc
updates land in the same commit as the functional change (`AGENTS.md`
§5). One ticket ≈ one commit — this ticket is at the upper end of the
size budget; the parse/model step and the eval step are the natural
seams if review demands a split.

## Step 0 — Probe (gate, not a code step)

- Write the new `t2_*.sdc` scenarios (`fixture.md`), extend `probe.sh`
  `SCENARIOS`, run the container probe, audit `git diff` (additive
  only), commit the fixture+artifacts on their own.
- Record the measured answers to `probe.md` §1–§3 in
  `docs/results.md` and `docs/multi-clock.md` §1.6 **before** writing
  eval code. Any answer that contradicts this design's hypothesis goes
  back to the card — do not code to the hypothesis.

## Step 1 — `sdc.rs`: parse + model

- `ObjSel::Clocks(Vec<usize>)`; `Value::Clocks` accepted in
  `timing_exception`/`path_delay` `-from`/`-to` (resolution per
  `api.md` §1); `-through` untouched.
- `Clock::waveform_ns: Option<[f32; 2]>`; `-waveform` in
  `CLOCK_VALUE_FLAGS`, two-instant parse, named bail otherwise.
- `ClockGroup`/`ClockGroupKind`/`Sdc::clock_groups`; `set_clock_groups`
  parser per `api.md` §3 (modes, multi `-group`, `order` stamp);
  unsupported forms bail or count per the probe.
- `multi_clock_bails`: remove the `set_clock_groups` and `-waveform`
  cases; add positive parse tests (`tests.md` §3).
- Gate: `cargo test -p grus-core sdc` green; all goldens byte-identical
  (nothing eval-side reads the new fields yet).

## Step 2 — `analysis.rs`: selector resolution + group desugar

- `key_dom`/`ep_cap` maps in `run_sta_partitioned` (`api.md` §2);
  `resolve_from`/`resolve_delay_from`/`resolve_to` `ObjSel::Clocks`
  arms.
- Clock-group desugar → synthesized `FalsePath` rules at `order`
  (`api.md` §3); `run_sta_detail` partition gate widened.
- Gate: selector + group scenarios match their fresh goldens at
  STRICT; all existing tests green.

## Step 3 — `analysis.rs`: waveform edge lattice

- `Clock::edge_offset` accessor; the five `parity · P/2` sites moved
  (`api.md` §4); `setup_edge_pair`/`hold_edge_pair` signatures gain
  offsets; call sites updated including `xdomain_defer`.
- Gate: waveform scenarios at STRICT; every single-clock golden
  byte-identical (`edge_offset` degenerates on the uniform grid).

## Step 4 — Test updates + fail-proof + docs + version

- `test_sta_clocks_match_opensta`: `parse_gated` shrinks (probably to
  empty or the scenarios that stay bail-gated); update the
  `evaluated + parse_gated == N` count; `cross_domain == 0` claim
  re-verified.
- Fail-proof mutations per `tests.md` §4 (one per feature); red, then
  restore, record both runs.
- `docs/multi-clock.md`: tick §3 item 4; §1.6 addendums (selector
  anchoring rule, group modes, waveform instants); §2 remove
  `create_clock -waveform`/`set_clock_groups` from the unsupported
  list, add whatever stays (e.g. `>2`-instant waveforms, group forms
  the probe rejected).
- `docs/timing-exceptions.md`: the "Clock-object selectors → v1
  ignored" line and the out-of-scope bullet updated to implemented +
  measured rule.
- `docs/results.md`: dated entry — probe command, per-scenario table,
  group drop counts, waveform numbers, fail-proof runs, what ran vs.
  not (bare-name clocks, `-through`, `>2` instants, unexercised fall
  instant if applicable).
- `docs/roadmap.md`: `mc-selectors` done, `mc-golden` next.
- `tests/designs/README.md`: scenario list/count.
- `Cargo.toml` `0.2.3 → 0.2.4` + `Cargo.lock` in the functional commit.
- Gate: `cargo test --workspace` + `cargo clippy --workspace
  --all-targets -- -D warnings` green.

## Files

- `crates/grus-core/src/sta/sdc.rs`
- `crates/grus-core/src/sta/analysis.rs`
- `tests/designs/sta_clocks/`
- `tests/designs/README.md`
- `docs/multi-clock.md`
- `docs/timing-exceptions.md`
- `docs/results.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
