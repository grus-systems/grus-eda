# mc-selectors — Probes for the revision

One question is left open by the first probe round: what a bare clock
*source-port* name resolves to inside `-from`/`-to`. Everything else in
this revision is a golden-coverage change, not a new measurement — the
moved `PROBE_ONLY` scenarios were already run; folding them into
`SCENARIOS` emits their `ref_*.json` blocks.

All runs go through `tests/designs/sta_clocks/probe.sh` in
`ghcr.io/librelane/librelane:3.0.16`, exactly as committed (AGENTS.md §7
rule 1). Tcl on stdin: `!` is history expansion — write `[...] == 0`.

## 1. Bare port names in `-from`/`-to` (`t2_selport`, `t2_selpto`)

The review defect: `ExceptionCtx::clock_row` matches `c.port`, so
`set_false_path -from clk1` — `clk1` is `cka`'s source port, not its
`-name` — currently cuts every cka launch. OpenSTA was never asked what
`-from clk1` means.

New scenarios (fixture boilerplate per `fixture.md`):

- `t2_selport.sdc` — `set_false_path -from clk1`.
- `t2_selpto.sdc` — `set_false_path -to clk2` (input port on the `-to`
  side; `resolve_to` shares `clock_row`, so both arms are pinned).

Questions, in priority order:

1. Does the endpoint table change at all? Expected: `clk1` resolves to
   the *port* object — a non-empty collection, so no "no valid objects"
   warning — but it starts no data path (it only feeds the clock
   buffers), so the rule covers nothing: the full 8-endpoint table,
   silent.
2. Or does OpenSTA resolve `clk1` to the `cka` clock (the `t2_selbare`
   drop set)?
3. Does `read_sdc` warn — and is the warning the per-name "not found"
   form (`t2_selunk`'s `clock 'nope' not found` + `no valid objects
   specified for -from`) or nothing?
4. Same two questions on the `-to` side for `clk2`.

Record the full report text (`probe_t2_selport.txt`,
`probe_t2_selpto.txt`) — the `Startpoint:`/`Endpoint:` headers and any
`STA-` warning — plus the ref JSON blocks, and write the answer in
`docs/results.md` and `docs/multi-clock.md` §1.7.

Decision table for the eval change (`api.md` §1):

| Measured | `clock_row` | bare `clk1`/`clk2` in `ObjSel::Names` |
|---|---|---|
| resolves to port, dead rule, silent | `c.name` only | resolves as port object (no coverage, no count) |
| resolves to port, dead rule, warns | `c.name` only | unresolved name-miss count stays |
| resolves to clock | unchanged | `t2_selbare` rule — comes back to the card |
| parse error | `c.name` only | bare names never resolve to clocks; document |

The expected outcome (name-only + silent port resolution) matches the
already-measured `resolve_clock` rule: `set_clock_latency 1.0 clk1` on a
`-name`d clock is `unknown clock` in OpenSTA — port names are not clock
handles — while exception selectors resolve names against the *object*
namespaces, where `clk1` is a legitimate port object.

## 2. Golden coverage re-fold

Move from `PROBE_ONLY` to `SCENARIOS` — all four implement and all
produce complete endpoint tables in OpenSTA (`run_sta` emits ref JSON
since `read_sdc` only warns):

- `t2_selbare` — `-from cka` = `-from [get_clocks cka]` (measured).
- `t2_selac` — `-from [all_clocks]` = union over all rows.
- `t2_selunion` — `-from [get_clocks {cka ckb}]` = union.
- `t2_selunk` — `-from [get_clocks nope]` warns (`clock 'nope' not
  found`, `no valid objects`) and drops nothing — a dead rule, still a
  complete table.

**Empty-table hazard** (measured, `probe_t2_selac.txt`):
`t2_selac`/`t2_selunion` cut every launch — OpenSTA prints `No paths
found.` and the endpoint list is empty. `run_sta`'s
`format "%.6f" [sta::worst_slack -max]` on a path-less design emits a
non-numeric value (`Inf`), which is invalid JSON and would corrupt
`two_clock.probe.json`. Guard in `run_sta` before folding: when
`$endpoints` comes back empty, emit `null` for the four
`wns/tns` fields and `0` for the violation counts — the endpoint list
itself is the measurement. The golden test skips the aggregate compare
on an empty `endpoints` array (`tests.md` §1).

Same discipline for the other implemented forms OpenSTA accepted:

- `t2_grp1` — single `-group cka` cuts vs every other clock (same 8→5).
- `t2_grpe` — `-group {} -group ckb` — pins the empty-group arm of the
  desugar.
- `t2_grpb` — `-group cka -group ckb` bare names = `t2_grp`.
- `t2_wavebig` — `-waveform {0 2.5}` on 2.0 — instants past the period,
  wrapped (`rem_euclid`).

`PROBE_ONLY` keeps exactly the error/divergence forms, which emit no
usable endpoint table:

- `t2_grpn` — missing mode flag, STA-0400 (ours: named bail).
- `t2_wave3` — odd instant count, STA-0371 (ours: named bail).
- `t2_wave4` — `>2` instants, **accepted** by OpenSTA; ours named-bails
  deliberately — the documented divergence.
- `t2_waverev` — non-increasing instants, STA-0372 (ours: named bail).

The two new port-name scenarios join `SCENARIOS` directly — they
measure and golden in the same run (whatever the answer is, it lands
as a table or a recorded error block).

## Method

- `probe.sh` `SCENARIOS` gains the 8 moved names + `t2_selport` +
  `t2_selpto` (39 scenarios on `two_clock.def`, plus `t2_latch`);
  `PROBE_ONLY` shrinks to 4. No logic changes to the script.
- `git diff` audit after regen (AGENTS.md §7 rule 1): the 29 existing
  scenario blocks in `two_clock.probe.json`, the `t2_latch` block, and
  every existing `probe_t2_*.txt` **byte-identical**; only new blocks
  appear. Name the steps run in `docs/results.md` (rule 9).
- `tests/designs/README.md`: scenario count and the `PROBE_ONLY`
  description updated in the same commit.
