# e6-drt-e2e — doc updates (r6, landed at 0d113ba)

All docs below landed with the **final measured numbers** — the X3
fix superseded the r5 pinned-residual draft (no `met1.2=32` entry
exists in the committed docs).

## `docs/results.md` — dated 2026-11-24 entry (landed)

Structure as designed (E7-style, plus the fix trajectory):

- **Trajectory**: `lef_expressible` findings on our DEF
  ~3025 → 847 (`d8d0c6c` mask) → 514 (`4c5e2cb` confinement) →
  317 merged regions / `met1.2`=32 (`fba5f85` closure + residuals,
  DEF `1534c323855a6029`) → **305 merged regions / `lef_expressible`
  = 0 on all 50 rules** (`0d113ba`, DEF `635f19a9aae86a44`). Emit
  censuses recorded verbatim per hop.
- **met1.2 attribution record**: the 7-site table transcribed
  (`_099_` + `resp_val`/`_084_`/`_131_`/`_122_`/`_070_`/`_066_`), the
  `who6.tcl`/`who7.tcl` probe outcome — all X3, `try_fill`'s
  host-open carve bug — and the fix (fills spanning the facing band,
  `notch_fills` 221 → 343, `_099_` closed, `routed` 187 → 188).
  Magic-unit → DBU conversions documented (`×5`, `box` takes lambda).
- **Gate table**: all 50 rules `ours=placed=routed_ref=0`;
  `magic_only` 3 nwell rows all inherited; `unclassified_fired`
  **empty** (the two first-contact names closed — recorded, not
  omitted); `sanity_control` `met2.2` new-vs-clean, control live.
- **Self-DRC scope correction**: why the subset missed the class
  (spacing family compares connectivity islands — the too-short fill
  bridged each site's prongs into one island; `drc(full)` measures
  edge-to-edge inside the island).
- **Connectivity record**: `unrouted=81` named via the golden's
  `ours_router.unrouted` (the test joins it — no inline count);
  `unrouted_kinds NoPath=46 Unattachable=52` (per-pin failures, sum >
  net count explained); `unrouted_blockers`; `stub_outside_cover` =
  142 records accumulated across iterations — `_165_` re-attached, so
  the gate join is `stub_lines ≥ Unattachable`, not `⊆ unrouted`;
  `notch_fills=343` recorded, never a target.
- **Comparison table**: wirelength 4081.929 µm (−32.49% vs delegated,
  −33.68% vs baseline — restated "on the 188 routed nets"), per-cut
  via census 327/500/144/10 with the via2/via3 positive deltas,
  antenna 0 everywhere, `signal_nets_routed` −81. Named outlier:
  wirelength rose vs the r4 emit (3114 → 4082) because un-vetoed
  fills changed occupancy for later nets (+1 net routed); fills are
  `RECT` paint, not wire.
- **picorv32 deferral — explicit**: leg un-run; no golden commits;
  the `#[ignore]`d test carries the joins.
- **What ran** (E0 rule 9): probe names, the `--verify` arm's
  `OURS-DETERMINISTIC` sha, the one fresh `check_drt_ours.sh gcd`
  that emitted the golden (superseding the timeout-lost 20:07
  transcripts), the test list, `0.12.6 → 0.12.7`.

## `docs/e6-drt.md` — §9 e2e section + §8 scope correction (landed)

- §9: script → golden schema → gcd outcome (trajectory, all-0 gate,
  inherited nwell rows, unrouted=81, mechanisms pointer to
  `kurena/tasks/e6-drt-e2e/design/`) → picorv32 deferral → the
  `tests/drt.rs` gate description.
- §8: self-DRC scope sentence corrected to the measured gap — the
  spacing family's island-merge blind spot, named as the r5 `met1.2`
  class closed at emit. Claim boundary stands: subset on our DEF,
  not the deck.

## `docs/status.md` + `docs/roadmap.md` (landed)

E7-style done line scoped honestly: native DRT under committed
guides — Magic `drc(full)` `lef_expressible` on gcd **0 on all 50
rules**, `magic_only` inherited, unclassified empty, antenna 0,
sanity live; `routed=188 unrouted=81` carried (named via the golden
join — no "fully routed" claim); wirelength deltas stated;
**picorv32 leg deferred** named in the same line.

## Files

- `docs/results.md`
- `docs/e6-drt.md`
- `docs/status.md`
- `docs/roadmap.md`
