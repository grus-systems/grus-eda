# mc-selectors — Overview (revision 2)

Fourth ticket of the multi-clock sequence (`docs/multi-clock.md` §3).
The first pass landed in `c69cd99` (v0.2.4): clock-object selectors in
exceptions, `set_clock_groups` desugared to row-keyed false-path pairs,
and `create_clock -waveform` driving the edge lattice via
`Clock::edge_offset`. This revision is the review fix-up — the feature
shape stands; four defects and two cleanups get corrected, measured,
and golden-covered. All line numbers reference the worktree HEAD
(`c69cd99`).

## Review findings this revision resolves

1. **`ExceptionCtx::clock_row` over-matches** (`analysis.rs` ~L3243).
   It accepts `c.port` as well as `c.name`, so a bare `-from clk1` — a
   clock's *source-port* name — resolves to the `cka` row and cuts every
   cka launch. That arm was never measured: `t2_selbare` exercises
   `-from cka` (the `-name`), not `-from clk1`. Both parse-side lookups
   (`resolve_clock`, `clock_rows`) are already name-only and the
   `resolve_clock` comment records the measured rule (`set_clock_latency
   1.0 clk1` on a `-name`d clock is `unknown clock` in OpenSTA). Fix:
   `clock_row` matches `c.name` only, plus a probe (`t2_selport`,
   `t2_selpto`) pinning what `-from`/`-to <port-name>` actually resolves
   to in OpenSTA (`probe.md` §1, `api.md` §1).
2. **Implemented-but-ungoldened scenarios.** `t2_selbare`, `t2_selac`,
   `t2_selunion`, `t2_selunk` run under `probe.sh`'s `PROBE_ONLY` list —
   report text only, no `ref_*.json` block, so
   `test_sta_clocks_match_opensta` never evaluates them even though the
   eval handles all four (bare clock name, `all_clocks`, union, and the
   warn-plus-dead-rule unknown clock). The same gap covers `t2_grp1`,
   `t2_grpe`, `t2_grpb`, `t2_wavebig` — all accepted by OpenSTA and all
   implemented (`single -group`, empty `-group {}`, bare `-group` names,
   instants past the period). All eight move to `SCENARIOS` and fold
   into `two_clock.probe.json`; `PROBE_ONLY` keeps only the four
   error/divergence forms (`probe.md` §2, `fixture.md`).
3. **Unconditional `dead_ends` fanout traversal** (`analysis.rs`
   ~L4364–4393). Every `run_sta_partitioned` call walks the fanout cone
   of every launch key, but the result is only consulted when a
   signature class carries a `ResolvedRule::Delay`. Gate per class on a
   delay rule in `sigma` and traverse only that class's keys — the
   all-keys `HashMap` goes away (`api.md` §3).

Cleanups: the hold-side block comment (~L5498, plus three sibling spots
at ~L4285, ~L5100, ~L5336) still describes the hold pair as
*coincident* — `hold_edge_pair` is min-gap since `t2_wave` measured a
non-coincident pair — and the `edge_offset` / non-uniform
`setup_edge_pair`/`hold_edge_pair` table rows the first design promised
never landed (`tests.md` §2).

## What stays

The feature surface is unchanged: `ObjSel::Clocks(Vec<usize>)` resolved
at parse via `clock_rows`; `key_dom`/`ep_cap` maps in
`run_sta_partitioned`; `set_clock_groups` desugaring to pairwise
false-path rules (all three modes share the path; a lone group cuts
against every other clock; `-allow_paths` stores nothing);
`Clock::waveform_ns` + `edge_offset` feeding seeds, pairing and phase
comparisons. `t2_grp`'s 8→5 shrink, the selector drop sets, and the
waveform numbers are all still governed by their goldens.

## Golden-identity invariants

- Every scenario already in `SCENARIOS` stays byte-identical in
  `two_clock.probe.json`; moved `PROBE_ONLY` names gain `ref_*.json`
  blocks and their `probe_*.txt` is regenerated identically.
- `clock_row` name-only changes nothing on committed SDCs: no committed
  scenario passes a clock *port* name to `-from`/`-to`, and `t2_selbare`
  uses the `-name` (`cka`). The `t2_selport` golden pins the measured
  port-name rule from OpenSTA directly.
- `dead_ends` gating is behaviour-preserving: the leaf pass only ever
  ran under a `Delay` rule; every non-delay scenario is untouched.
- `unresolved_exception_selectors` recounting (resolved-object misses,
  `api.md` §1) keeps `exc_diag`'s `== 4` assertion and every golden —
  skip counts are not golden-compared, and per-name plus all-miss-side
  counting is preserved.

## Out of scope

- `mc-golden`: promotion of `sta_clocks` to `two_clock.reference.json`.
- `-through` and the other `*_UNSUPPORTED_FLAGS` qualifiers.
- `>2`-instant `-waveform` lists (OpenSTA accepts — `t2_wave4` — here a
  named bail, `multi_clock_bails`-pinned), odd/non-increasing lists.
- `launch_domain` folding a virtual row onto a design row with the same
  period but a different `-waveform` — the fold compares periods only.
  Parked; the fixture has no such SDC.
- `set_clock_groups` precedence below the measured `t2_grpd`/`t2_grpd2`
  surface.

## Risks

- `t2_selport`'s measurement decides between three worlds: `clk1`
  resolves to the *port* object (dead rule, no warning — the expected
  OpenSTA behaviour), to the *clock* (drops cka launches), or errors.
  The `api.md` §1 implementation plan targets the first; either of the
  other two comes back to the card before coding.
- Folding `t2_grpe` (empty `-group {}` + `-group ckb`) into the golden
  verifies the empty-group arm of the desugar against OpenSTA's table —
  if OpenSTA's empty group cuts differently than "contributes nothing",
  the desugar needs a real rule, not a hypothesis.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `tests/designs/sta_clocks/`
- `tests/designs/README.md`
- `docs/multi-clock.md`
- `docs/timing-exceptions.md`
- `docs/results.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
