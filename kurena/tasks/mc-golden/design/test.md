# mc-golden — Test changes

`test_sta_clocks_match_opensta` lives in the `analysis.rs` test
module (~lines 9229–9400), not `crates/grus-core/tests/`.

## Edits

In `crates/grus-core/src/sta/analysis.rs`:

- Doc comment (~L9229): "`tests/designs/sta_clocks/*.probe.json` was
  emitted by `probe.sh`" → `*.reference.json`; the rest of the
  comment stays accurate.
- Filename pairs (~L9255–56):
  `("two_clock.def", "two_clock.probe.json")` →
  `("two_clock.def", "two_clock.reference.json")`, same for the
  latch pair.
- `.expect("probe file")` / `.expect("probe JSON")` →
  `.expect("golden file")` / `.expect("golden JSON")` — matching the
  wording `compare_sta_edge` / `test_sta_exceptions_match_opensta`
  use. `std::fs::read` + `expect` already panics on a missing file:
  the loud-failure requirement is met by construction, no skip path
  exists.

Everything else is unchanged: STRICT tolerances, the polarity
tie-resolution (`resolve_tied_polarity`, `SLACK_TIE_NS`), the
`CrossDomain` accounting both directions, the empty-golden-table
aggregate skip (`t2_selac`/`t2_selunion`), the `t2_selport`
`unresolved_exception_selectors == 0` pin, and `evaluated == 40`.

## PROBE_ONLY parse-gate pins

The four bail-pinned forms have no golden block; their gate is
`multi_clock_bails` in `sdc.rs` (~L3354), which pins each bail on
inline snippets:

| committed file | bail | pinned message |
|---|---|---|
| `t2_grpn.sdc` (`set_clock_groups`, no mode flag) | parse error | `needs -asynchronous, -logically_exclusive or -physically_exclusive` |
| `t2_wave3.sdc` (`-waveform {0 0.5 1.0}`) | parse error | `even number of edge instants` |
| `t2_wave4.sdc` (`-waveform {0 0.5 1.0 1.5}`) | named bail (OpenSTA accepts) | `more than two instants is unsupported` |
| `t2_waverev.sdc` (`-waveform {1.7 0.3}`) | parse error | `strictly increasing` |

Those pins stay untouched. One small addition inside
`test_sta_clocks_match_opensta` pins the **committed files**
themselves rather than only the inline snippets — ~8 lines:

```rust
for name in ["t2_grpn", "t2_wave3", "t2_wave4", "t2_waverev"] {
    assert!(
        load_sdc(&dir.join(format!("{name}.sdc"))).is_err(),
        "{name}: PROBE_ONLY form must stay a named bail"
    );
}
```

This keeps the acceptance's "PROBE_ONLY forms keep parse-gate pins"
honest against fixture drift; it is separable if the reviewer
objects.

## Fail-proof (AGENTS.md §7 rule 7)

Two mutations, both restored afterwards and both recorded in
`docs/results.md`:

1. `git mv tests/designs/sta_clocks/two_clock.reference.json /tmp/`
   → `cargo test -p grus-core sta_clocks_match` must panic naming the
   missing file (proves loud failure, no silent skip). Restore.
2. Perturb one golden number (`+0.1` on `t2_int` `u_fx/D`
   `max_req_ns` in `two_clock.reference.json`) → test must go red on
   that field. `git checkout --` restore, confirm green.

## Files

- `crates/grus-core/src/sta/analysis.rs`
