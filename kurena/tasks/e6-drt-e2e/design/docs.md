# e6-drt-e2e — docs updates (r2)

## `docs/e6-drt.md`

- **§7 recorded-looseness inventory**: update the host-cell OBS
  carve entry — on li1 the carve survives (masked, `!open`) scoped
  as shipped; on met1 it was *removed* — the host-open layer carries
  legality through `Obs(cell)` marks instead. Record the shipped
  hull model: masked layers {li1, met1}, host-open {met1}, the
  `PinCover` corridor growth, `transit_extend`, `ObsPin`, the
  `pin_obs_rects` addition, and which `pin-access.md` rungs landed
  (P1/P2/P3) with their measured unrouted deltas.
- **New e2e section**: the container gate result — per-rule
  `lef_expressible` counts on ours vs placed vs delegated
  (all-zero table), `magic_only` decomposition verdict, the two
  unclassified names with per-input counts, antenna surface,
  `hull_masked`/`unrouted_kinds` census.
- **Claim boundary**: claimed — Magic `drc(full)` clean on the
  `lef_expressible` subset for our routed DEF under committed
  guides; NOT claimed — `count_flat` (insensitive counter),
  `drt_report` (n/a for the native router), any `magic_only`
  residual names, unrouted residual classes by name.

## `docs/results.md`

One dated entry, four measurements in order:

- The RED first-contact measurement (7 rules, ~3025 fragments, 2
  unclassified names, self-DRC=0 proof) — the gate's demonstrated
  red arm, kept as failure log per convention.
- **Mask-verification run** (`check_drt_ours.sh gcd` on the
  checkpoint DEF, unrouted=67): per-rule `lef_expressible` +
  unclassified verdict *before* connectivity work — proves the
  emitted wires were already clean (or names the residual rules and
  their root cause). Measurement-only; JSON not committed.
- The pin-access landing: which rungs shipped, `unrouted_kinds`
  progression per rung, residual classes by name + blocking
  `Occupant` attribution.
- The green table: per-rule `lef_expressible` `{ours, placed,
  routed_ref}` rows (all-zero), `magic_only` `inherited` verdicts,
  unclassified counts; then the `delta` block as rows — wirelength
  µm + %, via total + per-cut-layer, antenna,
  `signal_nets_routed` — vs baseline and vs delegated, with named
  explanations for outliers (wirelength divergence from hull
  detours, via-count deltas from access-stack placement).
- Which verify arms ran: `generate_drt_ours.sh --verify`
  (`OURS-DETERMINISTIC <sha>`), `check_drt_ours.sh` per design,
  `cargo test` legs (including the ignored picorv32 legs run
  manually), clippy.

## `docs/status.md` / `docs/roadmap.md`

E7-style done line for E6's native-DRT subset scope: "routed under
committed guides, Magic `drc(full)` clean on the `lef_expressible`
subset" + named residuals if any remain. Honest wording: if unrouted
residuals exist the line names them with their blocking class, same
as E8's convention.

## Files

- `docs/e6-drt.md`, `docs/results.md`, `docs/status.md`,
  `docs/roadmap.md`
