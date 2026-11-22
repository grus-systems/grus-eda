# xdomain-deferrals — Implementation design (per item, per outcome)

Item 3 implements unconditionally — the divergence is already measured.
Items 1/2/4 branch on the probe outcome (`probe.md`).

## Item 3 — boundary-inclusive hold pairing (always)

### Measured rule

From `probe_t2_waverot.txt` + `two_clock.reference.json`
(`t2_waverot.u_fy/D`): OpenSTA's hold pair is **capture-edge-driven** — for
each capture edge `tc` in the LCM window, pair with the smallest launch
edge `≥ tc`; the winner minimises `launch − capture`. The launch edge may
sit at-or-past `lcm` (`u_fy/D`: capture 1.9 → launch 2.0, gap 0.1). This
subsumes the old launch-driven rule (`t2_wave`: capture 0.3 → launch 1.0)
and makes the pairing total: a pair exists whenever `grid_lcm` converges.

### Change (`analysis.rs`)

Rewrite `hold_edge_pair` (~5392) as a capture-driven search, keeping the
`(m·PL, j·PC)` return contract:

```rust
/// Hold pair (measured `probe_t2_waverot.txt` `u_fy/D` min): for each
/// capture edge `phase_c + j·PC` inside the LCM window, the launch edge is
/// the smallest `launch0 + m·PL` at-or-after it — including the first edge
/// past the boundary, which the launch-driven window excluded (OpenSTA
/// pairs launch 2.0 vs capture 1.9 there). `None` only when `grid_lcm`
/// fails — the search always pairs on converging grids.
fn hold_edge_pair(lclk: &Clock, cclk: &Clock, g: usize, c0: usize) -> Option<(f32, f32)> {
    let lcm = grid_lcm(lclk.period_ns, cclk.period_ns)?;
    let (pl, pc) = (lclk.period_ns as f64, cclk.period_ns as f64);
    let launch0 = lclk.edge_offset(g) as f64;
    let phase_c = cclk.edge_offset(c0) as f64;
    let eps = 1e-9 * pc.max(pl);
    let mut best: Option<(f64, f64, f64)> = None;
    // Capture edges in [phase_c, lcm): distinct residues; a boundary-edge
    // duplicate of tc == phase_c adds nothing to the winner.
    let mut j = 0i64;
    loop {
        let tc = phase_c + j as f64 * pc;
        if tc >= lcm * (1.0 - 1e-6) { break; }
        let m = ((tc - launch0 - eps) / pl).ceil().max(0.0) as i64;
        let tl = launch0 + m as f64 * pl;
        let gap = tl - tc; // >= -eps
        if best.is_none_or(|(g0, _, _)| gap < g0 - 1e-9) {
            best = Some((gap, m as f64 * pl, j as f64 * pc));
        }
        j += 1;
    }
    best.map(|(_, lsh, csh)| (lsh as f32, csh as f32))
}
```

Notes:

- Equal gaps keep the first found (smallest capture edge) — same tie-break
  direction as the launch-driven version on every committed case; the
  exact OpenSTA tie order is unmeasured (same parked sub-question as the
  setup tie-break).
- `xdomain_defer`'s min-side `pair_ok` then fails only on `grid_lcm`
  `None` — the "no capture edge at-or-before any launch edge" deferral
  becomes unreachable for commensurate grids. Keep the code path as the
  defensive guard; the doc's deferral list drops the item.
- `setup_edge_pair` is untouched — its launch-driven window needs no
  boundary edge (a boundary launch duplicates edge 0's residue and can
  never win the min-gap race).

### Verification

- `t2_waverot.u_fy/D` now reports `min_arr 2.287085`, `min_req 1.862415` at
  STRICT against the existing golden — no probe rerun needed.
- `cross_domain_endpoints` becomes 0 on every committed scenario, so the
  golden's wns/tns/violation aggregates compare on `t2_waverot` too.
- **Pin**: the golden test currently tolerates a deferral on a golden pin
  (`is_deferred` exempts it and aggregates skip) — a regression back to the
  deferral would stay green. Add an assertion in
  `test_sta_clocks_match_opensta` that `cross_domain_endpoints == 0` per
  scenario (after item 3 every committed scenario evaluates).
- Fail-proof (E0 rule 7): revert the boundary inclusion → `u_fy/D` defers
  → the new assert goes red; restore.

## Item 1 — undeclared-launch zero mask (branch)

- **OpenSTA drops (expected)**: `t2_u1`/`t2_u2` show `u_fy/D` unconstrained
  or absent. Keep the `<undeclared>` deferral — record it as a defensive,
  unreachable guard in `docs/multi-clock.md` §2 (same status as
  `grid_lcm`), with the probe artifacts as the "OpenSTA produces no value
  either" evidence. Pin with a test on the one-clock SDC asserting
  `u_fy/D`'s skip outcome (no arrival → a `skipped_endpoints` entry or
  plain absence — pin whichever lands) rather than the unreachable reason.
- **OpenSTA reports a value** (e.g. the `-to` delay check in `t2_u2` times
  `u_fy/D` from the unclocked `u_fb0/Q`): implement the measured shape —
  likely an unclocked `LaunchSeed` (clk→Q from edge 0, `dom` bit 0 — the
  literal `<undeclared>` mask) emitted only where the arrival reaches an
  endpoint the SDC constrains. The deferral then covers the *unconstrained*
  residue, not the reported one. Details follow the measured anchor; the
  probe's reported arr/req lines fix the edge-time model.

## Item 2 — latch-D foreign launch (branch)

Deferral site: `xdomain_defer`'s `latch && foreign != 0` early return
(~5474) plus the `(d,g)` candidate loop in `collect_endpoints`, which has
no latch branch on the foreign side.

- **OpenSTA borrows across domains**: remove the `latch && foreign`
  early-return; let `pair_ok` gate setup (`setup_edge_pair`) and hold
  (`hold_edge_pair` post-item-3). The foreign `(d,g)` candidate gains the
  borrow evaluation: opening/closing edges on the **capture** grid, launch
  on the `(d,g)` grid — the probe's `clock cka`/`clock ckb` edge times and
  `time borrowed` lines fix whether the opening edge pairs as "largest
  open-polarity edge ≤ launch edge" (hold-shaped) and the closing edge as
  "smallest check edge > launch edge" (setup-shaped). `t2_xlat2`'s deeper
  arrival discriminates `req = opening` vs `req = min(arr, closing)`.
  Mirror the same-domain structure (arr ≤ opening → `opening_max` req;
  inside the window → `min(arr, closing_max)`).
  `probe_t2_xlat*.txt` decides the exact edge rule before any code lands.
  If implemented: `t2_xlat`/`t2_xlat2` move to the latch-fixture
  `run_sta`/`run_report` pair and fold into
  `two_clock_latch.reference.json`.
- **OpenSTA checks the latch hard-edged (no borrow)**: same un-gating; the
  foreign candidate uses only the setup/hold pairs already implemented.
  Golden the probe scenario.
- **OpenSTA drops/errors**: keep the deferral; pin `u_ln/D`'s
  `CrossDomain` skip on `t2_xlat` in a unit test; `docs/multi-clock.md` §2
  keeps the item with the evidence pointer.

## Item 4 — `grid_lcm` non-convergence (branch)

- **OpenSTA evaluates within a bounded horizon** (a table appears; the
  edge times reveal the bound — e.g. a fixed edge-count cap): bound
  `grid_lcm`/`launch_window` to match the measured horizon and golden the
  probed scenario.
- **OpenSTA warns/errors/times out**: keep the deferral, pinned by the
  artifact (a timeout is recorded verbatim in `probe_t2_lcm*.txt`).
  Regardless of branch: add a unit test pinning `grid_lcm`'s
  converge/diverge verdict on the two probed period pairs, and bound
  `launch_window`'s trip count — a converged sub-resolution gcd currently
  yields an LCM of ~1e7 ns and ~1e7 loop iterations per `(d,g)` candidate.
