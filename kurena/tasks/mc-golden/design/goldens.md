# mc-golden — Reference emission

`probe.sh` stays the committed generator (the acceptance names it);
only its consolidated output names change. The file layout of the
emitted JSON is untouched — top-level `design` + `scenarios` map —
because `test_sta_clocks_match_opensta` already deserializes exactly
that.

## `probe.sh` edits (four spots, comments + redirects)

- Header comment (lines ~9–10): "consolidated into
  `two_clock.probe.json`" → `two_clock.reference.json`; adjust the
  `PROBE_ONLY` comment (~line 199, "without folding a `ref_*.json`
  block into `two_clock.probe.json`") and the final `echo` the same
  way.
- Line ~216: `> two_clock.probe.json` → `> two_clock.reference.json`.
- Line ~228: `> two_clock_latch.probe.json` →
  `> two_clock_latch.reference.json`.
- Line ~231: `echo` message updated.

No Tcl changes, no `SCENARIOS`/`PROBE_ONLY` changes, no schema
changes. `run_sta`/`run_report` and the `ref_*.json` intermediates
(`rm -f ref_*.json` at the end) stay as committed.

## Regeneration procedure

1. Bring the container host up per `AGENTS.md` §2
   (`mise exec docker-cli@latest colima@latest lima@latest -- colima
   start --memory 12 --cpu 8` if needed). `probe.sh` re-execs itself
   into `ghcr.io/librelane/librelane:3.0.16` when not already inside.
2. Run `bash tests/designs/sta_clocks/probe.sh` **whole** — all 39
   `run_sta` + `run_report` pairs, the 4 `PROBE_ONLY` reports, the
   latch pair. A partial re-run is recorded as partial (E0 lesson 9).
3. `diff` the new `two_clock.reference.json` against the committed
   `two_clock.probe.json`, and `two_clock_latch.reference.json`
   against `two_clock_latch.probe.json`. **Required: byte-identical.**
   Any diff means the committed script no longer reproduces its
   output — stop and investigate; never hand-edit the golden into
   place (E0 lessons 1–2).
4. `git status`/`git diff` on `tests/designs/sta_clocks/`: every
   `probe_t2_*.txt` must be byte-identical (regenerated, unchanged).
5. `git rm` the two `.probe.json` files, `git add` the two
   `.reference.json` files — git records the rename with zero content
   diff.

## Size

`two_clock.probe.json` is ~25 KB and `two_clock_latch.probe.json`
~2 KB — far under the 1 MB commit threshold; the renamed files
commit the same way.

## CI note (parked)

`.github/workflows/ci.yml`'s reproduce-check loops
`tests/designs/sta_edge/*/generate_ref.sh` only. `sta_clocks` is not
in that glob; adding it is a reasonable follow-up (the same
`git diff --exit-code` argument applies to the renamed goldens) but
`ci.yml` is outside this spec's Files list, so it stays untouched.

## Files

- `tests/designs/sta_clocks/probe.sh`
- `tests/designs/sta_clocks/two_clock.probe.json` → `two_clock.reference.json`
- `tests/designs/sta_clocks/two_clock_latch.probe.json` → `two_clock_latch.reference.json`
