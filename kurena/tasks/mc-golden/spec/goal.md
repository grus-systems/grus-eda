# Goal

Final ticket of the multi-clock sequence (`docs/multi-clock.md`
§3 item 5): promote the `sta_clocks` evidence to the formal
golden reference and close `docs/path-delays.md` §5 item 1.

The semantics are already implemented and compared
(`test_sta_clocks_match_opensta`, 40 scenarios at STRICT since
v0.2.5). This ticket is the formal promotion: the reference
data must be emitted by the committed generator script running
inside the LibreLane container (`probe.sh` already is that
script — verify the produced JSON is exactly what the test
reads, or rename/split cleanly per repo golden rules), the
test must deserialize the committed `two_clock.reference.json`
and assert against it (fail loudly if missing — no silent
skip), and the docs must record the close.

## Work

1. Emit `tests/designs/sta_clocks/two_clock.reference.json`
   (and `two_clock_latch.reference.json` if the latch probe is
   separate) from the committed generator in the container —
   `two_clock.probe.json` can become the reference verbatim if
   its schema is what the test deserializes; otherwise produce
   a thin extraction step in `probe.sh`. No hand-editing.
2. `test_sta_clocks_match_opensta` reads the reference file(s)
   — missing file = loud failure. Keep the per-scenario STRICT
   comparison; the four `PROBE_ONLY` bail-pinned forms
   (`t2_grpn`, `t2_wave3`, `t2_wave4`, `t2_waverev`) keep
   their parse-gate pins.
3. `docs/path-delays.md` §5 item 1 → closed (point at
   `t2_md`/`t2_mdl`/`t2_mdx`/`t2_selmd` + the reference file).
   `docs/multi-clock.md` §3 → tick `mc-golden`; §2 status
   updated to "implemented" where true.
4. `docs/results.md`: dated entry — reference emitted from
   `probe.sh` commit <hash>, scenario count, strict-tolerance
   pass, any remaining `CrossDomain` deferrals still named.
5. `docs/roadmap.md`: multi-clock line closed.
6. Version bump.

## Files

- `tests/designs/sta_clocks/` (reference json + probe.sh)
- `crates/grus-core/tests/` or wherever the golden test lives
- `docs/path-delays.md`, `docs/multi-clock.md`,
  `docs/results.md`, `docs/roadmap.md`
- `Cargo.toml` / `Cargo.lock`
