# mc-cleanups — Documentation and record fixes (revision 3)

The D-pass and revision-2 doc fixes landed (`e6bd1a7`, `25f9700`).
This file covers the second review memo's doc work: recording the
`t2_clkdup` measurement honestly, reattributing the scenario count
(L1), and noting the `set_clock_groups` unknown-name posture (L2).
Rule stays the same: claims must match what the committed artifacts
show.

## RD6 — `docs/results.md` revision entry (continuation of 2026-11-20)

A dated subsection appended to the same entry (the ticket's record
stays contiguous):

- **Probe**: `t2_clkdup` — the verbatim arr/req lines for
  `u_fa0/D`/`u_fb0/D` (the `d_in` seed), `ya`, `yb`, plus
  `wns_max`/`wns_min` and the `Endpoint: yb` row count; the
  discriminator read-off naming which hypothesis won.
- **Rule implemented**: which of H_acc / H_slot / H_overlap the
  container showed and the `delay_at` change that followed — or, under
  the contingency, the explicit statement that the same-anchor
  same-sense fold remains *inferred/unmeasured* (marking path applied)
  with `t2_clkdup` parked in the design doc.
- **Amendment note**: the earlier revision bullet says plain rules
  "`retain` only same-anchor entries then append" — state which half
  of that sentence is now measured and which (if any) remains
  inferred, so the log's earlier claim is not left standing
  uncorrected.
- **Test updates**: the four fold-asserting unit tests per the api.md
  table; `evaluated` 45 → 46.
- **Lows**: the README reattribution and the `set_clock_groups`
  posture note.
- **Checks actually run**: probe.sh whole run, `git diff
  tests/designs/` byte-identical audit, `cargo test --workspace`
  count, clippy, the fail-proof mutation. Version `0.2.8 → 0.2.9`.

## RD7 — `docs/multi-clock.md` sync

- §1.7 anchor block (~L254–278): the "prunes the surviving entries
  anchored to a *different* clock" sentence gets the same-anchor
  clause — per the measured rule (H_acc: same-clock entries
  accumulate, cite `t2_clkdup`; H_slot: the covered corner(s)
  overwrite per (clock, corner) slot; H_overlap: overlapping entries
  drop whole) — or, under the contingency, an explicit
  *inferred/unmeasured* marker.
- `set_clock_groups` block (~L182–195): append the unknown-name bullet
  (L2, see api.md): collection `-group` args filter to empty on the
  `clock_rows`/`t2_selunk` posture (unprobed for this command); bare
  unknown names bail `unknown clock` on the `resolve_clock` posture.
- Evidence parenthetical (~L24): "45 golden scenarios" → "46" (45
  `two_clock` + `t2_latch`), still attributed to the `mc-cleanups`
  revision.

## RD8 — `tests/designs/README.md` (L1 + count)

- The `sta_clocks` bullet's "Since `mc-selectors` (v0.2.5) all 45
  golden scenarios evaluate" is misattributed — v0.2.5 had 40.
  Restructure so `mc-selectors` keeps its feature list and the
  evaluate-count is stated against the version that owns it: 46 (45
  `two_clock` + `t2_latch`) at v0.2.9. Name `t2_clkdup` (same-anchor
  duplicate-rule measurement) alongside the v0.2.8 additions.
- No change to the `PROBE_ONLY`/`MEASURE_ONLY` lists (unchanged this
  revision).

## RD9 — `docs/timing-exceptions.md`

No change expected — the `unreached_endpoints` restatement from
revision 2 already covers single-corner `null` emission. Re-check only
if the `t2_clkdup` outcome changes what "unreached" can mean (it does
not under any of the three hypotheses).

## Files

- `docs/results.md`
- `docs/multi-clock.md`
- `tests/designs/README.md`
