# mc-cleanups — Implementation steps (revision 3)

Revisions 1–2 landed (`e6bd1a7`, `25f9700`). This sequence covers the
second review memo: R6 (probe + fold rule) and the optional lows L1/L2.
One commit; the probe run precedes the measured-rule implementation.

## Step 1 — Probe scenario + harness

- New `tests/designs/sta_clocks/t2_clkdup.sdc` per probes.md P5 —
  copy `t2_int.sdc` verbatim, replace the `d_in`/`ya`/`yb` delay lines.
- `probe.sh`: append `t2_clkdup` to `SCENARIOS` (45 names on
  `two_clock.def`); `MEASURE_ONLY`/`PROBE_ONLY` unchanged. No
  collector change — the revision-2 union/`null` hardening covers it.

## Step 2 — Container run

`bash tests/designs/sta_clocks/probe.sh` (Colima/docker per
`AGENTS.md` §2). Then `git status` / `git diff tests/designs/`:

- Every pre-existing `probe_t2_*.txt` and golden block: byte-identical.
  Drift means a committed scenario exercises the fold — stop, diagnose.
- `two_clock.reference.json` gains `ref_t2_clkdup` (may carry `null`
  min fields on `yb` under H_overlap); `probe_t2_clkdup.txt` is new.
- Read the discriminator table (probes.md P5): which delay each row
  reflects; `yb` min `−0.5`-class vs `null` separates H_slot from
  H_overlap; `yb`/`ya` max req `0.5`- vs `0.7`-delay separates H_acc.

Gate: artifacts in place. `cargo test --workspace` runs next — green
means the probe confirmed H_acc *and* the numbers were actually read;
red is expected under H_slot/H_overlap until Step 3 lands. Do not take
a green suite as evidence without checking the `t2_clkdup` numbers.

## Step 3 — Measured-rule implementation (R6)

Per `api.md` R6, driven by the measured outcome:

- **H_acc**: comments only — `delay_at`/`DelayEntry` docs and the
  fold-asserting test comments cite `t2_clkdup`.
- **H_slot**: `delay_at` fold gains `DelaySense::without`;
  `entries.into_iter().filter_map(…)` rebuild per api.md.
- **H_overlap**: `retain` predicate gains the `covers_overlap` clause.
- Update the four fold-asserting unit tests per the api.md table;
  extend `delay_anchor_prune_is_port_wide` with the same-sense cases.
- `analysis.rs`: `evaluated == 46`.
- If the observed shape matches none of the three, implement what the
  data shows — do not force a model.

Contingency (container genuinely cannot run): skip Steps 1–2, apply
`api.md`'s marking path (inferred/unmeasured markers), keep
`evaluated == 45`, and say so in `docs/results.md`.

Gate: `cargo test --workspace`, clippy `-D warnings`. Fail-proof:
implement the *other* predicate (`retain` with/without the
overlap clause), confirm `t2_clkdup` red on the discriminating fields
(`yb` min-null / `yb` max-req / `d_in`-seeded arr), restore.

## Step 4 — Docs + version bump

- `docs/results.md`: dated continuation of the 2026-11-20 entry —
  verbatim probe lines, discriminator read-off, the rule implemented
  (or the inferred-marking record), test updates, checks run. Amend
  the earlier "`retain` only same-anchor entries" bullet reference:
  state what is now measured vs still inferred.
- `docs/multi-clock.md`: §1.7 anchor bullet — same-anchor same-sense
  clause per the measured rule (or the inferred marker); the
  `set_clock_groups` unknown-name bullet (L2); scenario count 45 → 46
  in the L24 parenthetical.
- `tests/designs/README.md`: the v0.2.5→current count reattribution +
  `t2_clkdup` named (L1).
- `[workspace.package] version` `0.2.8 → 0.2.9`; `cargo check`
  refreshes `Cargo.lock`.

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --stat tests/designs/   # golden: +1 block, rest byte-identical
```

## Files

- `crates/grus-core/src/sta/sdc.rs`
- `crates/grus-core/src/sta/analysis.rs`
- `tests/designs/sta_clocks/probe.sh`
- `tests/designs/sta_clocks/t2_clkdup.sdc` (new)
- `tests/designs/sta_clocks/probe_t2_clkdup.txt` (new)
- `tests/designs/sta_clocks/two_clock.reference.json`
- `docs/results.md`
- `docs/multi-clock.md`
- `tests/designs/README.md`
- `Cargo.toml`
- `Cargo.lock`
