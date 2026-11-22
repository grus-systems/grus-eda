# mc-selectors — Test plan (revision)

## 1. Golden coverage

`test_sta_clocks_match_opensta` iterates every scenario block in
`two_clock.probe.json`/`two_clock_latch.probe.json` — folding the eight
promoted names and two new ones into `SCENARIOS` puts them under the
STRICT per-endpoint comparison with no test-code changes beyond the
count:

- `assert_eq!(evaluated, 40)` — 39 `two_clock` scenarios + `t2_latch`.
- The folded selector scenarios (`t2_selbare`/`selac`/`selunion`/
  `selunk`) pin drop sets and the dead-rule table; the folded group
  scenarios (`t2_grp1`/`grpe`/`grpb`) pin the 5-endpoint table
  including the empty-group arm; `t2_wavebig` pins the wrapped
  waveform; `t2_selport`/`t2_selpto` pin the measured port-name rule.
- `cross_domain == 0` on the fixture → wns/tns/violations compare on
  every new block automatically.
- Empty-table scenarios: `t2_selac`/`t2_selunion` produce
  `"endpoints": []` with `null` wns/tns (`fixture.md` guard). The
  compare skips the aggregate block when `golden_eps` is empty — our
  run must also report zero endpoints, which it does only if every
  candidate drops into `fp_endpoints`.

If the probe shows `t2_selport` resolving to the port object silently
(the expected reading), add a diagnostic assert on the `t2_selport`
run: `report.skipped.unresolved_exception_selectors == 0` — OpenSTA
warns nothing, and the golden cannot compare the count.

## 2. Unit tests (`analysis.rs`) — the missing waveform rows

Extend `test_edge_pairing_unit_cases` — the `clk` helper gains a
waveform field (`Clock { period_ns, waveform_ns, ..Default }`).

`Clock::edge_offset` table:

- `period 2.0, waveform None` → `edge_offset(0) == 0.0`,
  `edge_offset(1) == 1.0` (the `p · P/2` default).
- `waveform [0.0, 0.5]` → `(0.0, 0.5)`; `[0.3, 1.7]` → `(0.3, 1.7)`.

Non-uniform `setup_edge_pair`/`hold_edge_pair` rows (recompute against
the golden before committing the literals — these are the `t2_wave`
mechanics by hand):

- Setup, `cka{2.0, [0.3,1.7]} → ckb{1.0, default}`, `g=0, c0=0`:
  launch edges `0.3, 2.3, …` (LCM window keeps `m=0`); first ckb edge
  strictly after `0.3` is `1.0` → `Some((0.0, 1.0))`.
- Setup, same clocks, `g=1` (fall launch at `1.7`): first capture
  edge `2.0` → `Some((0.0, 2.0))`.
- Hold, `ckb{1.0} → cka{2.0, [0.3,1.7]}`, `g=0, c0=0` — the `t2_wave`
  measurement: launch edge `1.0` pairs with capture `0.3` →
  `Some((1.0, 0.0))`. This is the row that proves hold pairing is
  min-gap, not coincidence.
- Hold, `cka{2.0, [0.3,1.7]} → ckb{1.0}`, `g=0, c0=0`: launch `0.3`,
  largest ckb edge at-or-before is `0` → `Some((0.0, 0.0))` — shifts
  are zero even though the instants differ.
- Identity: `cka{2.0, [0.3,1.7]}` vs an identical waveform clock →
  the same-domain pair `(0.0, 2.0)`, degenerating to the uniform-grid
  rule; and `waveform [0, 1.0]` on `2.0` equals `None` on `2.0`
  (`(0.0, 1.0)` vs `ckb`).

## 3. Parse tests (`sdc.rs`)

No new parse surface in the revision. Existing pins keep governing:
`multi_clock_bails` (waveform odd/>2/non-increasing, missing group
mode, unknown group clock), `clock_selectors_parse_to_rows`
(all-miss → empty rows), `set_clock_groups_parses` (empty `-group {}`,
bare names, `-allow_paths` no-store), `create_clock_waveform_parses`
(including the `rem_euclid` wrap).

## 4. Fail-proof mutations (AGENTS.md §7 rule 7)

- `clock_row`: restore `|| c.port.as_deref() == Some(name)` → under the
  expected port-object measurement, `t2_selport` goes red (the port arm
  drops cka-anchored checks OpenSTA keeps). Red, restore, green —
  record both runs in `docs/results.md`.
- `resolved_any`: revert `resolve_sides` to the `is_empty` bump → the
  `t2_selport` diagnostic assert (`== 0`) goes red if §1's assert is
  added; otherwise record that the mutation is invisible on goldens and
  keep the assert as the justification for the recount.
- `dead_ends` gating is behaviour-preserving — no mutation needed; the
  delay-rule scenarios (`t2_selmd`, `t2_mdx`) still produce their
  `internal path endpoint` rows under the gate.

## 5. Existing pins that must not move

- `test_sta_exception_diagnostics`: `unresolved_exception_selectors
  == 4` on `exc_diag` — the `resolved_any` recount preserves it
  (`api.md` §1 arithmetic check); if it moves, the recount is wrong.
- Every other `sta_clocks` scenario block, `two_clock_latch`, and all
  non-`sta_clocks` goldens: byte-identical.
