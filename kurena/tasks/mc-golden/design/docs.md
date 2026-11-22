# mc-golden — Documentation edits

All doc text in English; edits are small and local.

## `docs/path-delays.md` — §5 item 1 closes

Item 1 under "Still open:" (~L627–644) already narrates the answer
(`t2_md`/`t2_mdl`/`t2_mdx` + `t2_selmd`, `docs/multi-clock.md`
§1.5/§1.7). It moves out of "Still open" into the resolved record:

- Drop the "Still open:" list (item 1 was its only member) and add a
  `Resolved by the multi-clock promotion (`mc-golden`, v0.2.6,
  2026-11-20)` group — keep the existing prose, updating its tail:
  "formal promotion to `two_clock.reference.json` stays with
  `mc-golden`" becomes the done state: `two_clock.reference.json` +
  `two_clock_latch.reference.json` emitted by the committed
  `probe.sh`, deserialized by `test_sta_clocks_match_opensta`.
- Header (~L15): "the remaining follow-ups in section 5 are open" →
  section 5 has no open items; reword to that effect (the three
  "Known gaps … ungoldened" bullets stay listed — they are recorded
  gaps, not open follow-ups).

## `docs/multi-clock.md`

- Status header (~L3–17): add "`mc-golden` landed (2026-11-20)";
  replace "This document remains the target spec for the remaining
  tickets." with a record sentence — the five-ticket sequence is
  complete, the document is the measured-rules record.
- Evidence block (~L19–28): `two_clock.probe.json` →
  `two_clock.reference.json`, `two_clock_latch.probe.json` →
  `two_clock_latch.reference.json` ("`probe_*.txt`" stays).
- §2 heading (~L213): `(after `mc-selectors`, 2026-11-20)` →
  `(after `mc-golden`, 2026-11-20)`. Inside §2 the "Now accepted" /
  "Now evaluated" / "Still unsupported" lists are all true as
  written; where a bullet names `two_clock.probe.json` or says the
  golden run is pending, bring it current (the "Verified per-endpoint
  at STRICT" bullet names the reference files).
- §3 item 5 (~L343–345): tick like items 1–4 — `**done 2026-11-20**
  (`docs/results.md` entry of the same date).` then the summary:
  `two_clock.probe.json`/`two_clock_latch.probe.json` renamed to
  `*.reference.json` by `probe.sh` as committed, byte-identical
  regen, `test_sta_clocks_match_opensta` deserializes them;
  `docs/path-delays.md` §5 item 1 closed.
- Open sub-questions tail (~L347–353): unchanged — those deferrals
  remain parked.

## `docs/results.md` — new dated entry

`### 2026-11-20 — `mc-golden`: `sta_clocks` promoted to
`two_clock.reference.json` (v0.2.6)`

Contents (this is the acceptance's "records the promotion" line):

- `probe.sh` output rename only (`two_clock.probe.json` →
  `two_clock.reference.json`, latch likewise); whole script re-run in
  `ghcr.io/librelane/librelane:3.0.16`; new files `diff`-verified
  byte-identical to the superseded `.probe.json`; every
  `probe_t2_*.txt` byte-identical. Record the emitting `probe.sh`
  commit hash (the functional commit of this ticket).
- `test_sta_clocks_match_opensta`: 40 scenarios (39 `two_clock` +
  `t2_latch`) at STRICT; `cross_domain_endpoints == 0` everywhere;
  `t2_selac`/`t2_selunion` empty-table aggregates still skipped by
  construction.
- Remaining `CrossDomain` deferrals still named (from
  `docs/multi-clock.md` §2): undeclared-launch zero mask
  (`launch <undeclared>`), latch-D foreign-launch borrow pairing,
  unpairable hold tying parity, defensive `grid_lcm` non-convergence
  — none reachable on this fixture.
- `docs/path-delays.md` §5 item 1 closed; sequence done.
- Commands run + fail-proof mutations (both restored), per
  `test.md`.

## `docs/roadmap.md`

N2 "Remaining:" bullet (~L300–313): "Remaining: golden promotion
(`mc-golden`)." → sequence complete — `mc-golden` (v0.2.6) promotes
the `sta_clocks` probe to `two_clock.reference.json`; all 40
scenarios are golden at STRICT; the four parked deferrals in
`docs/multi-clock.md` stay named. Also update the lead-in phrase
"The path-delay follow-ups in `docs/path-delays.md` section 5
(two-clock and latch fixtures)" — §5 has no open items now.

## `tests/designs/README.md`

`sta_clocks` paragraph (~L63): `two_clock.probe.json` →
`two_clock.reference.json`, `two_clock_latch.probe.json` →
`two_clock_latch.reference.json`; "committed evidence" → committed
golden reference. The `PROBE_ONLY` description stays accurate.

## Files

- `docs/path-delays.md`
- `docs/multi-clock.md`
- `docs/results.md`
- `docs/roadmap.md`
- `tests/designs/README.md`
