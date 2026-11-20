# mc-cleanups — Code changes (revision 3)

Revisions 1–2 landed in `e6bd1a7`/`25f9700` (A-items, D-items, R1–R5 —
see the 2026-11-20 `docs/results.md` entry). This file specifies the
second review memo's work: **R6** (the same-anchor same-sense fold,
medium) and the two optional lows. `sdc.rs` is
`crates/grus-core/src/sta/sdc.rs`.

## R6 — same-anchor same-sense rule in `delay_at` (medium)

### Current state

`sdc.rs:384–397`:

```rust
if !r.add {
    entries.retain(|e: &DelayEntry| e.clock == r.clock);
}
entries.push(DelayEntry { value_ns, clock, sense });
```

Measured: different-anchor prune (`t2_clkcor`), `-add_delay` append
(`t2_clkcor2` `yb`). **Inferred**: what `retain` does to a *same-clock
same-sense* entry — currently keeps it, producing the worst-fold.
`t2_clkdup` (probes.md P5) pins which of H_acc / H_slot / H_overlap is
OpenSTA's rule.

### Implementation per measured outcome

**H_acc confirmed** — no code change. Comments gain the `t2_clkdup`
citation (`delay_at` doc, `DelayEntry` doc, the fold-asserting test
comments): the same-anchor retention is now measured, not inferred.

**H_slot — per-corner-slot overwrite.** A plain rule prunes
different-clock entries *and* strips the covered corner(s) off
same-clock entries; a `Both` entry degrades to the uncovered corner
rather than dropping:

```rust
if !r.add {
    entries = entries
        .into_iter()
        .filter_map(|e| {
            if e.clock != r.clock {
                return None; // different anchor: pruned port-wide (t2_clkcor)
            }
            e.sense.without(r.sense).map(|sense| DelayEntry { sense, ..e })
        })
        .collect();
}
```

with a `DelaySense` helper:

```rust
/// The corners `self` keeps after a rule covering `covered` lands on
/// the same anchor — `None` when nothing remains.
fn without(self, covered: DelaySense) -> Option<DelaySense> {
    match (self, covered) {
        (DelaySense::Both, DelaySense::Min) => Some(DelaySense::Max),
        (DelaySense::Both, DelaySense::Max) => Some(DelaySense::Min),
        (s, c) if s == c || c == DelaySense::Both => None,
        (s, _) => Some(s),
    }
}
```

**H_overlap — entry-overlap removal.** Same-clock entries whose senses
overlap the rule's covered corners drop whole; disjoint-sense entries
survive:

```rust
entries.retain(|e| {
    e.clock != r.clock || !covers_overlap(e.sense, r.sense)
});
// covers_overlap: every pair except (Min, Max) / (Max, Min)
```

All three are a `delay_at`-local change — `corner_entries`,
`corner_values`, `declared_*_clock_rows` and every `analysis.rs` call
site are untouched (the entry list already supports zero or several
entries per corner; `corner_values` still folds `-add_delay`
accumulation). `DelayEntry.sense` stays a field; `into_iter` +
`filter_map` avoids mutation.

### Unit-test updates

Under H_slot or H_overlap, the four tests that assert the inferred
fold change expectations (comments re-cite `t2_clkdup`):

| test | asserted now (H_acc) | under H_slot/H_overlap |
|---|---|---|
| `counts_ignored_lib_cell_and_pin_options` (~L2856) | `output_delay_ns == (1.0, 2.0)` | `(2.0, 2.0)` — the `-pin` rule overwrites GCD's same-clock entry |
| `add_delay_widens_instead_of_summing`, 2nd block (~L2984) | `(0.4, 1.0)` | `(0.8, 0.8)` — plain `0.8` removes both `-add` entries |
| `remove_from_collection_subtracts_selections` (~L3289) | `(1.0, 2.0)` in / `(1.0, 3.0)` out | `(2.0, 2.0)` / `(3.0, 3.0)` |
| `keeps_negative_values_and_the_known_flags` (~L3344) | `(-0.5, 1.0)` | `(-0.5, -0.5)` |

Under H_acc these stay; their comments cite `t2_clkdup` instead of
reasoning from the prune. `delay_anchor_prune_is_port_wide`
(~L2999–3048) keeps all asserts under every outcome (its cases are
different-clock or different-sense); extend it with the `t2_clkdup`
same-sense cases matching the measured rule. Scenario coverage is the
`t2_clkdup` golden block at STRICT.

### Marking path (contingency only — container cannot run)

If P5 is skipped, the fold stays H_acc and every place that asserts or
describes it gets an explicit *inferred/unmeasured* marker — the E0
"claim coverage only for what an input exercises" rule:

- `delay_at` doc (`sdc.rs:375–383`): append — same-anchor same-sense
  retention is *inferred* from the different-clock prune measurement,
  not measured; `t2_clkdup` (design `probes.md` P5) is the pinned
  probe.
- `DelayEntry` doc (`sdc.rs:205–209`): same marker.
- The four asserting tests' comments: restate "asserts the inferred
  fold — unmeasured" instead of implying OpenSTA semantics.
- `docs/multi-clock.md` §1.7 anchor bullet and `docs/results.md`:
  record the inferred status and the parked probe.

## L1 — `tests/designs/README.md` count attribution

The `sta_clocks` bullet reads "Since `mc-selectors` (v0.2.5) all 45
golden scenarios evaluate (44 `two_clock` + `t2_latch`)". Wrong
attribution: 40 at v0.2.5, 45 at v0.2.8, **46 at v0.2.9**
(`t2_clkdup`). Reword so the v0.2.5 clause lists only what
`mc-selectors` added (clock selectors, `set_clock_groups`,
`-waveform`, `t2_selmd`) and the current count is stated once, against
the version that owns it — e.g. "all 46 golden scenarios evaluate (45
`two_clock` + `t2_latch`) since v0.2.9", with the `t2_clkdup`
same-anchor measurement named in the revision list. Under the
no-probe contingency the count stays 45 and the attribution is
"since v0.2.8" instead. Keep it a one-sentence edit inside the
existing bullet.

## L2 — `set_clock_groups` unknown-name posture

`docs/multi-clock.md` §1.7 `set_clock_groups` block (after ~L195) —
one bullet recording what `sdc.rs:1186–1190, 1227–1231` already
implements, with honest evidence status:

- `-group [get_clocks no_such]` filters to the empty group silently —
  the `clock_rows`/`t2_selunk` posture; `-group [get_clocks {}]` is the
  empty group (measured `t2_grpe`). Neither unknown-name form is
  probed *for this command* — the posture is adopted from the measured
  selector/`set_clock_*` cases.
- A bare `-group` name that resolves no clock bails `unknown clock` —
  the `resolve_clock` posture (measured on `set_clock_latency`).

## Golden-impact analysis

| Change | Endpoint table | Golden diff |
|---|---|---|
| R6 (any outcome) | `t2_clkdup` only — no committed scenario carries a same-clock same-sense duplicate | append-only `ref_t2_clkdup` |
| L1/L2 | — | none |

Verify the "no committed scenario" claim empirically: `git diff
tests/designs/` after the probe run must show pre-existing blocks
byte-identical; drift = a scenario that exercises the fold — diagnose,
do not regenerate silently.

## Files

- `crates/grus-core/src/sta/sdc.rs`
- `crates/grus-core/src/sta/analysis.rs` (`evaluated` assert only)
- `tests/designs/sta_clocks/probe.sh`
- `tests/designs/sta_clocks/t2_clkdup.sdc` (new)
- `tests/designs/sta_clocks/probe_t2_clkdup.txt` (new)
- `tests/designs/sta_clocks/two_clock.reference.json`
- `docs/results.md`
- `docs/multi-clock.md`
- `tests/designs/README.md`
- `Cargo.toml`
- `Cargo.lock`
