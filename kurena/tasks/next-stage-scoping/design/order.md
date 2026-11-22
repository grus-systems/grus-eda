# Recommended Order + Sizing

## Order: E4 → E5 → E8 → (E6, E7 decision-gated)

```
E4 global routing ──> E5 CTS ──> E8 IR drop ──> [gate] E6 detailed routing ──> E7 DRC
```

### Why E4 first

- Highest reuse of any stage: grid + capacity (`gcell.rs`), Steiner
  decomposition (`rsmt.rs`), the committing router (`route.rs` already
  computes the paths and discards them), and both consumers of the result
  (RC model in `sta/parasitics.rs`, SPEF writer) are verified against the
  reference today.
- It upgrades the timing answer end-to-end: `sta` currently stops at
  placement PD parasitics (documented ~150 ps median off routed truth);
  guide parasitics close the gap toward `gcd.rcx.spef`-class realism without
  a detailed router.
- Dependency-free: nothing else needs it first, but everything routed-side
  benefits (E6 would consume our guides exactly as DRT consumes GRT's).

### E5 second

- Independent of E4; verification stands entirely on the already-measured
  propagated-clock STA mode.
- Its real prerequisite is infrastructure: `DesignDb` cell/net insertion —
  the same mutation API any future buffering/repair step needs, so it pays
  off beyond the stage itself.
- Physical-flow order puts CTS before routing; the committed reference
  scripts route gcd without CTS, so E4's comparison is unaffected by which
  lands first.

### E8 third

- Independent of both routing stages (IR drop reads the PDN, not signal
  routes) and of a smaller build: via parsing + resistor mesh + sparse solve.
- P-IRD resolved the gate: PDNSim ships in the pinned image
  (`analyze_power_grid`/`check_power_grid`/`set_pdnsim_*`), so E8 is
  schedulable with a measured reference (gcd VPWR: worst drop 0.591 mV,
  per-instance voltage CSV via `-voltage_file`).

### E6/E7 at the end, behind a decision gate

- E6's missing surface (LEF `VIA`/`SPACING`, DEF `VIAS`/`NONDEFAULTRULES`,
  signal-net wire storage, rule-aware maze routing) is an order of magnitude
  larger than anything done so far, and `README.md` still lists detailed
  routing and DRC as out of scope — pursuing it is a scope decision for the
  user, not an implicit extension of the estimation pipeline. P-GR removed
  the "potentially": `read_guides` + `detailed_route` on a foreign guide file
  produces a routed DEF with 0 DRC on gcd — once E4 emits guides, OpenROAD's
  own DRT is a usable flow without writing a DRT. Recommend: re-evaluate
  after E4 lands, when guide quality is measured.
- E7 only checks what E6 emits (paired) or shrinks to a LEF-subset checker on
  the committed routed DEFs; P-DRC confirmed its reference exists (Magic
  8.3.623 `def read` + `drc count total` → 0 errors on `gcd.routed.def`), so
  the standalone scoping stays viable either way.

## Rough sizing (repo convention: 1 ticket ≈ 1 commit ≈ 50–150 lines)

| Stage | Reuse level | New surface | Tickets | Class |
|---|---|---|---|---|
| E4 global routing | Very high — router, grid, RC, writers all verified | committed-path return + net mapping, layer assignment, guide format I/O, guide→RC bridge, golden | ~6–8 | M |
| E5 CTS | Medium — legalizer + propagated STA verified | DesignDb mutation API, clock-net split, balance-driven tree build, skew golden | ~6–8 | M–L |
| E8 IR drop | Medium — power estimates + PDN rects | LEF via resistance, SPECIALNETS via extraction, resistor mesh, sparse solve, golden | ~4–6 | M |
| E7 DRC | Low–medium | LEF-rule subset parse + geometry checker (standalone), or full pairing with E6 | ~4 (subset) / ~8+ (paired) | M–L |
| E6 detailed routing | Low | rule parsing, via model, track assignment, DRC-aware maze routing, wire storage, output | 15+ (multi-sequence) | XL |

Sizing is reuse-driven, matching the goal's instruction; no date estimates.

## Ticket sketch

- **E4**: `e4-guide-paths` (return committed paths + net mapping) →
  `e4-layer-assign` → `e4-guide-io` (writer + reader for round-trip) →
  `e4-guide-rc` (guide → `NetParasitics` bridge) → `e4-golden` (probe →
  golden + tests) → `e4-cli` (`grus-eda route`/`--guide-out`) → docs.
- **E5**: `e5-db-mutate` (cell/net insertion) → `e5-clock-split` →
  `e5-tree-build` → `e5-legalize-inserted` → `e5-skew-report` →
  `e5-golden` → docs.
- **E8**: `e8-via-parse` → `e8-pdn-graph` (mesh from SPECIALNETS + vias) →
  `e8-solve` → `e8-golden` → docs.
- **E7** (if standalone): `e7-rule-parse` → `e7-checker` → `e7-golden`.
- **E6**: sequence gated on the post-E4 scope decision.

## Roadmap text

`docs/roadmap.md` gains a "Candidate next stages" section mirroring the
existing N-item style: one entry per stage (E4–E8) with the done-when table
compressed to a sentence each, the recommended order, and the explicit
E6/E7 gating note. The README's "out of scope" sentence stays untouched
(this ticket is doc-only under `docs/` + `kurena/`); the roadmap entry names
the tension instead.
