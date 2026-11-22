# e4-sta-e2e — Tests

All in `sta/analysis.rs`'s `#[cfg(test)] mod tests`, reusing
`compare_design_files` (endpoint-count assertion, `resolve_tied_polarity`,
per-field `within_tolerance`, WNS/TNS checks, max-diff print) unchanged —
the same harness every `*_spef_matches_opensta` test uses.

## New `Rc` variant

```rust
Rc::GuideOursSpef => {
    let spef = crate::readers::load_spef(
        &gcd.join(format!("{design_name}.guide.ours.spef")))
        .expect("SPEF");
    Some(crate::sta::spef_parasitics(&design, &spef).expect("SPEF parasitics"))
}
```

One `match` arm next to `Rc::Spef`/`Rc::RoutedSpef` — the `sta --spef`
consumer path in-process. `picorv32.guide.ours.spef` is gitignored and
regenerated; `.expect` on it fails loudly (no silent skip), the kurena
worktree seed covers it, and `generate_guide_ours_spef.sh` is the
documented producer.

## Gate tests (criterion 4)

```rust
#[test]
fn test_gcd_guide_sta_matches_opensta() {
    compare_design_files("gcd", "gcd.sdc", "gcd.guide.sta.reference.json",
        "guide_spef", Rc::GuideOursSpef, Tol::GCD_SPEF);
}

#[test]
fn test_picorv32_guide_sta_matches_opensta() {
    compare_design_files("picorv32", "picorv32.sdc",
        "picorv32.guide.sta.reference.json",
        "guide_spef", Rc::GuideOursSpef, Tol::PICORV32_SPEF);
}
```

- `Tol::GCD_SPEF` = arrival/req/slack `max(5 ps, 1%)`, slew 3% — the
  criterion bounds arrival and slack; slew inherits the recorded
  cell-stage residual and every field is measured anyway.
- `Tol::PICORV32_SPEF` = arrival floor 6 ps, slack 40 ps, slew 10%, TNS
  2.5% — the `routed_spef` floors from `docs/status.md`, as the goal
  prescribes.

## Engine-isolation tests (diagnostic, asserted)

```rust
#[test]
fn test_gcd_guide_spef_engine_residual() {
    compare_design_files("gcd", "gcd.sdc", "gcd.guide.sta.reference.json",
        "guide_ours_spef", Rc::GuideOursSpef, Tol::GCD_SPEF);
}
// picorv32 sibling with Tol::PICORV32_SPEF
```

Same SPEF on both sides → the residual is the pure STA-engine gap, which
should land inside the same band as `routed_spef`'s. If this test fails,
the divergence is in our STA engine, not the guide/RC chain — the
diagnostic pays for itself exactly there. Asserted, not printed: a
same-parasitics cell that drifts outside the shared engine band is a real
regression.

## Decomposition prints (recorded, not gated)

```rust
#[test]
fn test_gcd_guide_sta_decomposition() { ... }
// picorv32 sibling
```

Loads `<d>.guide.sta.reference.json` once and prints, per endpoint and
per field, the diffs that attribute residual to a named cause
(`print_scores`-style, like the routed-SPEF scoring tests):

- `guide_ours_readback` − `guide_spef`: **guide topology** consequence
  (e4-1 residual — coverage, strip merging, layer band).
- `guide_ours_spef` − `guide_ours_readback`: **parasitics model**
  residual (our guide RC vs OpenROAD's on identical guides, incl. island
  stubs and the `EST-0026` warned-pin treatment: OpenROAD leaves them
  dangling, we attach to the nearest node).
- our sta − `guide_ours_spef`: **STA engine** residual (duplicate of the
  engine test's own numbers; printed so the table is complete).
- our sta − `guide_spef`: the gated total.

No assertions — the output feeds the `docs/results.md` decomposition
table. When a gate endpoint is outside bound, this table is what names
the cause per §7.5.

## Fail-proof (AGENTS.md §7.7)

After the gate tests pass, mutate and confirm red, then restore and
record:

- Point `Rc::GuideOursSpef` at `gcd.routed.spef` (wrong parasitics,
  plausible mixup) → the gcd gate test must go red on arrival/slack.
  Restore.
- If the mutation stays green that is a finding (the bound is too loose
  to discriminate parasitics) — record it, do not weaken the check.

## Out-of-band verification (recorded, not cargo tests)

- `generate_guide_ours_spef.sh` on both designs; rerun → `git diff` on
  `gcd.guide.ours.spef` empty (determinism record, E0 lesson 1).
- `generate_ref.sh` step 9c on both designs in the pinned image; rerun
  step 9c → `git diff` on `gcd.guide.sta.reference.json` empty; every
  other golden byte-identical (full-script reruns must not perturb steps
  0–9b outputs — investigate, never commit drift).
- `cargo test --workspace`, `cargo clippy --workspace --all-targets --
  -D warnings`, `cargo test --release -p grus-core sta`.
