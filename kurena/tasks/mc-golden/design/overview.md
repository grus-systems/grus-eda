# mc-golden — Overview

Final ticket of the multi-clock sequence (`docs/multi-clock.md` §3
item 5). The semantics are implemented and compared since v0.2.5:
`test_sta_clocks_match_opensta` evaluates all 40 `sta_clocks`
scenarios (39 `two_clock` + `t2_latch`) against OpenSTA at STRICT
tolerance, reading `two_clock.probe.json` /
`two_clock_latch.probe.json`. This ticket is the formal promotion
only — no eval or parser changes.

## Decision: rename `.probe.json` → `.reference.json`

`AGENTS.md` §2 reserves `*.reference.json` for golden files a test
deserializes and asserts against, emitted by a committed script
inside the reference tool. `*.probe.json` was evidence naming from
the `multi-clock-probe` ticket; now that the file *is* the golden,
it takes the golden name. The alternatives are worse:

- Keep `.probe.json`: breaks the repo's golden naming convention and
  leaves the promotion implicit.
- Extraction step in `probe.sh`: pointless — the committed schema
  (`{"design": ..., "scenarios": {<name>: <ref block>}}`, ref blocks
  identical in shape to `exceptions.reference.json`) is exactly what
  the test deserializes (`reference["scenarios"]` per scenario name).
  Rename verbatim, per the goal.

So: `probe.sh` keeps its name and keeps emitting (it is the
committed generator the acceptance names), but its consolidated
outputs are renamed to `two_clock.reference.json` and
`two_clock_latch.reference.json`. The script is re-run whole inside
`ghcr.io/librelane/librelane:3.0.16`; the emitted files must diff
byte-identical against the current `.probe.json` — that diff is the
proof the rename is a rename, not a hand-edit.

## What stays

- Schema, scenario list, ref-block contents: byte-identical.
- `probe_<name>.txt` report files: keep their names — they are
  evidence text, not deserialized goldens.
- `PROBE_ONLY` keeps exactly the four bail-pinned forms
  (`t2_grpn`, `t2_wave3`, `t2_wave4`, `t2_waverev`): no `ref_*.json`
  block is folded for them; their committed `.sdc` files stay out of
  the golden and keep their parse-gate pins (`test.md`).
- `sta_evaluates_two_clock` and `one_clock_sdc_on_two_clock_def`:
  read no JSON; untouched.
- Historical records (`docs/results.md` past entries,
  `kurena/tasks/*/design/*.md`) keep the `.probe.json` names they
  recorded.

## Docs affected

`docs/path-delays.md` §5 item 1 closes (its text already anticipates
the promotion). `docs/multi-clock.md` gets the `mc-golden` tick, the
evidence block's file names updated, and §2 status brought current.
`docs/results.md` gets the dated promotion entry naming the emitting
`probe.sh` commit. `docs/roadmap.md` N2 closes the multi-clock line.
`tests/designs/README.md`'s `sta_clocks` paragraph is renamed to the
golden names.

## Out of scope

- CI: the `generate` job regenerates `tests/designs/sta_edge/*/` only
  (`git diff --exit-code` reproduce check). Extending it to
  `sta_clocks/probe.sh` is a plausible follow-up but is not in this
  spec's Files; parked.
- Any change to eval, pairing, selectors, tolerances, or the
  `multi_clock_bails` pins.
- Merging the two golden files into one — the latch fixture is a
  separate DEF and stays a separate file (acceptance allows it).

## Risks

- Regen not byte-identical: stop, do not proceed — a drifted probe
  means the committed script no longer reproduces its output
  (E0 lesson 1); investigate before committing anything.
- Container unavailable on the runner: the ticket blocks at Step 0;
  record exactly what ran vs. not (`AGENTS.md` §2 last bullet).

## Files

- `tests/designs/sta_clocks/probe.sh`
- `tests/designs/sta_clocks/two_clock.probe.json` → `two_clock.reference.json`
- `tests/designs/sta_clocks/two_clock_latch.probe.json` → `two_clock_latch.reference.json`
- `tests/designs/README.md`
- `crates/grus-core/src/sta/analysis.rs`
- `docs/path-delays.md`
- `docs/multi-clock.md`
- `docs/results.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
