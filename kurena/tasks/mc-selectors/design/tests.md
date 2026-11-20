# mc-selectors — Test plan

## 1. Golden coverage (`analysis.rs` tests)

`test_sta_clocks_match_opensta` already iterates every scenario in
`two_clock.probe.json`/`two_clock_latch.probe.json` and compares each
endpoint field at `Tol::STRICT`. After regen the new scenarios are
picked up automatically:

- `parse_gated` — `t2_grp` leaves the list; anything the probe showed
  OpenSTA rejecting but our parser still bails on stays (update the
  count assert `evaluated + parse_gated == N`).
- `t2_grp`: assert the 5-endpoint table and `cross_domain == 0` so the
  wns/tns/violation aggregate compare runs.
- Selector scenarios: drop sets come out of the per-endpoint
  comparison itself — no special asserts needed beyond STRICT.
- Waveform scenarios: STRICT on all 8 fields per endpoint.
- `sta_evaluates_two_clock` unchanged (still `t2_int`).

## 2. Unit tests (`analysis.rs`)

- `edge_offset` table: `None → [0, P/2]`, `{0 0.5}`, `{0.3 1.7}`.
- `setup_edge_pair`/`hold_edge_pair` with non-uniform offsets: the
  `t2_wave`-measured pairs as table rows (launch 0.3 on 2.0 vs capture
  rise on 1.0 grid etc.), plus a same-grid identity case reducing to
  the existing rules.
- `resolve_from`/`resolve_to` `ObjSel::Clocks` on a tiny prepared ctx
  (or through `run_sta_detail` on `two_clock.def` + `t2_selfa.sdc` —
  assert the drop set, cheaper to write).

## 3. Parse tests (`sdc.rs`)

- `ObjSel::Clocks` accepted in all four exception commands, multi-name
  and `[all_clocks]`; unknown name → `unknown clock`.
- `set_clock_groups`: `-asynchronous` two-group parse rows; the
  exclusive modes per probe; degenerate/unsupported forms → the
  measured bail or count.
- `create_clock -waveform`: `{0 0.5}` lands `waveform_ns = [0, 0.5]`;
  non-two-instant forms → the named bail; `-waveform` on a virtual
  clock parses too.
- `multi_clock_bails`: drop the `set_clock_groups` and `-waveform`
  cases (now parsed); keep everything still genuinely unsupported.

## 4. Fail-proof mutations (AGENTS.md §7 rule 7)

Run each mutation, confirm the named scenario goes red, restore:

- Groups: make the desugar a no-op → `t2_grp` reports 8 endpoints
  instead of 5.
- Selectors: `resolve_from(ObjSel::Clocks)` returns empty →
  `t2_selfa`'s drop set collapses (or the rule counts unresolved —
  whichever the mutation produces, it must go red, not silent-green).
- Waveform: force `edge_offset` to `p · P/2` → `t2_wave`/`t2_wave05`
  arr/req mismatch.

Record both the red and green runs in `docs/results.md`.
