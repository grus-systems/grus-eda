# mc-cleanups — Overview (revision 2)

Cleanup bundle for the low-severity findings left over from the
multi-clock sequence (`mc-model` … `mc-golden`, v0.2.x) plus adjacent
docs/power nits.

**Status**: the first implementation landed as `e6bd1a7` (v0.2.7) — all
spec items and the seven `MEASURE_ONLY` probes are in. The human review
returned a memo with two medium findings and three cheap lows; this
revision is the follow-up work.

Version: `e6bd1a7` bumped `0.2.6 → 0.2.7`; the revision is another
functional change on the same branch → `0.2.7 → 0.2.8` in the same
commit (`[workspace.package]`, `Cargo.lock` refreshed).

## Review-memo inventory (the remaining work)

| # | Item | Kind | Where |
|---|------|------|-------|
| R1 | `delay_at` collapses the `-clock` anchor to the last matching rule — `-max -clock cka` then `-min` (no `-clock`) declocks the port for **both** corners | code + probe | `sdc.rs` `delay_at`, `analysis.rs` seed/collect paths |
| R2 | `[get_clocks]` no-arg all-clocks rule recorded via an **uncommitted** ad-hoc stdin check — commit a probe scenario per E0 rules | probe artifact | `t2_eclk2.sdc` (new), `sdc.rs` comment cite |
| R3 | `t2_waverot`/`t2_wavez` only ran `run_report` — run them through `run_sta` so the wrapped-order/`[0,0]`-collapse semantics get endpoint-level evidence | probe artifact + test | `probe.sh` `SCENARIOS` |
| R4 | `t2_eclk` overclaim: results/code cite a "`t2_eclk` family" for forms the file never exercises (empty `-from`/`-to` collections, `-group [get_clocks {}]`, `set_propagated_clock` name drop) | probe + wording | `t2_eclk.sdc` extension, `sdc.rs` comments, `results.md` |
| R5 | `set_propagated_clock` non-clock-name drop (collection names now drop silently where literal names error — the `literal` flag in `propagated_clock`) is unprobed and unrecorded | probe + record | `t2_eclk.sdc`, `docs/` |

## Landed items (e6bd1a7, v0.2.7) — reference only

A1 `grid_lcm` guard; A2 empty clock collections + `[get_clocks]` forms;
A3 pin `-from` last-wins docs; A4 PROBE_ONLY expected-substring asserts;
A5 post-wrap `-waveform` keep; A6 stale names/comments; A7 `-to`
resolved-but-invalid counting (5× `STA-1551`); A8 clock-wins namespace
priority; A9 `-clock`-less I/O delays → no timed path; A10 SAIF nits;
D1–D7 docs pass. All recorded in the 2026-11-20 `docs/results.md`
entry. R-items below build on that landed state, not on the spec text.

## Probe plan (one container run)

`tests/designs/sta_clocks/probe.sh`, same container
(`ghcr.io/librelane/librelane:3.0.16`):

- Extend `t2_eclk.sdc` with the claimed-but-unprobed forms (R4, R5).
- New scenarios: `t2_eclk2` (`[get_clocks]` no-arg, R2), `t2_clkcor` /
  `t2_clkcor2` (mixed `-clock` rule sequences, both orderings +
  `-add_delay` anchor discrimination, R1).
- `t2_eclk2`, `t2_clkcor`, `t2_clkcor2`, `t2_waverot`, `t2_wavez` join
  `SCENARIOS` — their `ref_*.json` blocks fold into
  `two_clock.reference.json` so `test_sta_clocks_match_opensta` covers
  them per-endpoint at STRICT (append-only: existing entries stay
  byte-identical).
- `collect_endpoints_json` in `run_sta` needs hardening first:
  a mixed-anchor scenario produces corner-only endpoints
  (`u_fa0/D` timed on max but absent from the min pass) — the current
  collector reads `pins` from `paths_max` only and dereferences
  `min_*($name)` unconditionally (unset var → Tcl error / silent
  min-only loss). Collect the union of both passes and emit `null` for
  absent corners.
- `MEASURE_ONLY` shrinks to `t2_eclk t2_nsfrom t2_nsto t2_toinv
  t2_noclk`; `t2_waverot`/`t2_wavez` move to `SCENARIOS`.

See `probes.md` for scenario contents and discriminators.

## Non-goals

- No new SDC commands or selector namespaces.
- No change to the LCM pairing model or the deferral conditions.
- `set_input_delay`/`set_output_delay` value folding (`-add_delay`
  min-widen/max-widen) is unchanged — only the *anchor* fold is in
  scope.
- No attempt to model OpenSTA's warning text; counted diagnostics stay
  in `SkipReport`/`ignored_commands`.
- Split-anchor × `-from`/`-to` clock-selector interaction is a
  pin-granular approximation (the cut cannot be corner-granular) —
  record the limitation, do not probe it.

## Verification gates (repo rules + `workflow.toml`)

1. `cargo test --workspace` — includes the reworked golden compare
   (`null` corner fields) and `evaluated == 45` (44 `two_clock` +
   `t2_latch`).
2. `cargo clippy --workspace --all-targets -- -D warnings` — clean.
3. `git diff tests/designs/`: existing `two_clock{,_latch}.reference.json`
   entries and pre-existing `probe_t2_*.txt` byte-identical; new
   golden entries append; `probe_t2_eclk.txt` regenerates from the
   extended `.sdc` (recorded).
4. Fail-proof: break each new code path (whole-port declock restore,
   emit-rule revert to `(Some, Some)`), confirm red, restore. Record.
5. `docs/results.md` dated continuation of the 2026-11-20 entry: probe
   lines verbatim, discriminators, rules implemented, wordings fixed.
6. Optional heavy: `cargo test --release -p grus-core sta`.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/sdc.rs`
- `crates/grus-core/src/sta/parasitics.rs` (test asserts only)
- `tests/designs/sta_clocks/probe.sh`
- `tests/designs/sta_clocks/t2_eclk.sdc`
- `tests/designs/sta_clocks/t2_eclk2.sdc` (new)
- `tests/designs/sta_clocks/t2_clkcor.sdc` (new)
- `tests/designs/sta_clocks/t2_clkcor2.sdc` (new)
- `tests/designs/sta_clocks/t2_waverot.sdc`
- `tests/designs/sta_clocks/t2_wavez.sdc`
- `tests/designs/sta_clocks/two_clock.reference.json`
- `tests/designs/sta_clocks/probe_t2_*.txt`
- `docs/results.md`
- `docs/multi-clock.md`
- `docs/timing-exceptions.md`
- `tests/designs/README.md`
- `Cargo.toml`
- `Cargo.lock`
