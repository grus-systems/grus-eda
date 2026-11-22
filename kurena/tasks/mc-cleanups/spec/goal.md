# Goal

Cleanup bundle — the accumulated low-severity findings from the
multi-clock sequence cards plus adjacent docs/power nits. All
items are small, mechanical, or need at most a single OpenSTA
probe to pin behaviour. No semantics change beyond the listed
items; all existing goldens stay byte-identical.

## Code nits (no probe needed)

1. `grid_lcm` dead code: the `y <= tol` branch is unreachable
   (the previous remainder was already checked `> tol`) —
   remove or restate the guard honestly.
2. `set_clock_*` with `[get_clocks {}]` resolves to zero rows
   and writes nothing silently — count it in the existing
   unresolved-selector diagnostic (or bail per the measured
   rule — check what OpenSTA does with an empty object list;
   follow it).
3. `delay_max_pins`/`delay_min_pins`: a covering pin `-from`
   rule followed by an ordinary covering rule silently
   overwrites — record the last-wins semantics in the field
   docs (or keep both if that is the measured rule).
4. PROBE_ONLY test loop asserts `load_sdc(...).is_err()` — it
   also passes on a missing `.sdc` file. Assert the error is
   the *expected parse bail*, not just any error.
5. `-waveform` post-`rem_euclid` instants can reorder or
   collapse (`{1.9 2.1}` → rise 1.9/fall 0.1; `{0 2.0}` →
   `{0,0}`). The strictly-increasing check runs pre-wrap only —
   validate post-wrap too, matching the probed OpenSTA
   behaviour for the wrap cases.
6. Rename remaining stale identifiers/comments:
   `sta_evaluates_two_clock_same_domain` name (asserts
   cross-domain now), the stale hold-pairing comment ~L5498,
   `skipped_endpoints` field doc.

## Small probes first (one container run each)

7. `-to clk2` resolves dead silently; OpenSTA warns
   `STA-1551 'not a valid endpoint'` — count or warn per the
   measured surface.
8. Unnamed clock (name = source port): bare `-from clk1`
   resolves in both port and clock namespaces — probe the
   priority OpenSTA uses and implement it.
9. `set_input_delay` without `-clock` under `-waveform`: the
   launch edge currently falls back to the design row — probe
   which instant OpenSTA picks (design clock rise vs other)
   and match it.

## Docs/record fixes

10. `docs/timing-exceptions.md`: the new skip counters from
    load-selectors are not reflected.
11. `docs/results.md`: add the missing `cargo test
    --workspace` lines to the saif-power entry, the literal
    `awk` command for when-power, and fix the "15 parseable"
    count (14 + `t2_grp` parse-gated).
12. `driven_nets_without_activity`'s meaning change
    (when-power) — record it.
13. `tests/designs/README.md` fixture list is missing
    `sta_clocks`-era entries; `design/fixture.md` `t2_mdx`
    description still says 'delay-check rewrite on both sides'
    (the measured semantic is pin-startpoint).
14. saif nits: fractional `DURATION` leniency (either accept
    per grammar or fix the error path), negative-value error
    mislabel, Latin-1 tokenizer — fix or document as intended.

Version bump; `docs/results.md` entry listing what was done
vs. deferred.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/sdc.rs`
- `crates/grus-core/src/readers/saif.rs`
- `crates/grus-core/src/power.rs` (doc only if needed)
- `docs/timing-exceptions.md`, `docs/results.md`,
  `docs/multi-clock.md`, `tests/designs/README.md`
- `Cargo.toml` / `Cargo.lock`
