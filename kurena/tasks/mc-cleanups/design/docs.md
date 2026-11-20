# mc-cleanups — Documentation and record fixes (revision 2)

The D1–D7 doc pass of the original design is **landed** (`e6bd1a7`).
This file covers the revision's doc work: correcting overclaims the
review found, recording the new measurements, and keeping counts in
step with the promoted scenarios. Rule: claims must match what the
committed artifacts show — audit before editing, record honestly.

## RD1 — `docs/results.md` `t2_eclk` bullet rewording (R4/R5)

The 2026-11-20 entry's `t2_eclk` bullet overclaims:

- "`[get_clocks {}]` on `set_clock_latency`/`-uncertainty`/
  `-transition`/`set_propagated_clock`: clean output, no warning, the
  full `t2_int` table (no accidental apply-to-all)" — true for
  `set_clock_latency`/`-uncertainty` (latency +0.5 and setup −0.2 are
  both visible in the table) and largely for `-transition` (clock-pin
  slews). For `set_propagated_clock` the table only proves *no warning*
  and *no propagated-mode clock lines* — the apply-to-all exclusion is
  weaker; say so per command instead of lumping.
- "`[get_clocks]` with no argument returns every declared clock
  (ad-hoc stdin check in the same container)" — replace the
  parenthetical with the `t2_eclk2` citation once the committed
  scenario exists (`probe_t2_eclk2.txt` + the golden block).
- Append a sentence listing the newly-measured forms from the extended
  file (empty `-from`/`-to` collections, `-group [get_clocks {}]`,
  `set_propagated_clock`/`unset_propagated_clock` collection forms)
  with their observed warnings/errors verbatim.

## RD2 — `docs/results.md` revision entry (continuation of 2026-11-20)

A dated subsection appended to the same entry (the ticket's record
stays contiguous):

- **Probes**: `t2_clkcor`/`t2_clkcor2` tables and discriminators —
  verbatim `clock …` lines and `Endpoint:`/`unconstrained` evidence;
  `t2_eclk` extension warnings/errors verbatim; `t2_eclk2` shift;
  `t2_waverot`/`t2_wavez` `run_sta` numbers.
- **Rules implemented**: the per-corner anchor (or whichever measured
  outcome), the `-add_delay` anchor rule, the collector union/`null`
  fix (a latent crash — `can't read "min_arr(…)"` — independently of
  R1), the `set_propagated_clock` name-drop record.
- **Wordings fixed**: the `t2_eclk` overclaim bullets, the removed
  ad-hoc clause, the `(measured)` comment now citing `t2_eclk2`.
- **Schema note**: `EndpointReport` corner fields become `Option` —
  `--sta-json` emits `null` for an absent corner; the golden JSON
  carries the same convention.
- **Checks actually run**: probe.sh whole run, `cargo test
  --workspace` count, clippy, `git diff` audit result, fail-proof
  mutations.
- Version bump `0.2.7 → 0.2.8`.

## RD3 — `docs/multi-clock.md` sync

- ~L23–26: "40 golden scenarios" → 45 (44 `two_clock` + `t2_latch`);
  the `MEASURE_ONLY` parenthetical names the shrunken list.
- §1.7 empty-collections bullet: extend with the `t2_eclk` extension
  evidence (`-from`/`-to` empties, `-group [get_clocks {}]`,
  `set_propagated_clock [get_clocks no_such]` → the measured warn/drop
  posture — **record the non-clock-name drop change explicitly**, R5)
  and cite `t2_eclk2` for the no-arg rule.
- §1.7 new bullet: per-corner `-clock` anchor rule for
  `set_input_delay`/`set_output_delay` — each corner keeps the `-clock`
  of the last rule that wrote it; an unanchored corner produces no
  timed path on that corner only; `-add_delay` anchor rule as
  measured; the split-anchor × clock-selector pin-granular
  approximation recorded as a limitation.
- §1.7 waveform bullet: `t2_waverot`/`t2_wavez` are now golden
  scenarios (not just report probes) — adjust the parenthetical.
- `skipped_endpoints`/`unreached_endpoints` doc mentions (if any in
  this file): an endpoint with candidates on only one corner is now
  emitted with `null` on the missing corner — `unreached` means no
  candidates at all.

## RD4 — `docs/timing-exceptions.md`

- The skip-counter section's `unreached_endpoints` bullet: restate —
  counts endpoints with zero candidates on **both** corners; a
  single-corner endpoint is emitted with `null` fields (measured
  `t2_clkcor`).

## RD5 — `tests/designs/README.md`

The `sta_clocks` entry: "all 40 golden scenarios evaluate (39
`two_clock` + `t2_latch`)" → 45 (44 + `t2_latch`); mention the
mixed-anchor (`t2_clkcor*`), no-arg `get_clocks` (`t2_eclk2`) and
wrapped-waveform (`t2_waverot`/`t2_wavez`) coverage, and that golden
endpoint fields may be `null` for a corner the SDC does not time.

## Files

- `docs/results.md`
- `docs/multi-clock.md`
- `docs/timing-exceptions.md`
- `tests/designs/README.md`
