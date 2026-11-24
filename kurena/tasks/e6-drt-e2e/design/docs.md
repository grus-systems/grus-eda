# e6-drt-e2e — doc updates (r5)

All in `docs/`, written at the end of implementation with the final
measured numbers — not pre-written claims.

## `docs/results.md` — dated r3 entry

Structure (E7-style, plus the fix trajectory):

- **Trajectory line**: `lef_expressible` violations on our DEF
  ~3025 (first contact) → 847 (`d8d0c6c` mask) → 514 (`4c5e2cb`
  confinement WIP) → **317 total / `met1.2=32`** (`fba5f85`
  self-spacing closure + residual fixes, measured in the pinned
  image on the committed DEF `1534c323855a6029`). Each hop names
  what landed and what was measured; the `fba5f85` emit census
  (`routed=187 unrouted=82 notch_fills=221` dedup'd) recorded
  verbatim.
- **met1.2 site table** (`met12-attribution.md` §1 transcribed):
  32 fragments → 7 sites — `_099_` (4, the named `notch_residual`
  census site) + `resp_val`/`_084_`/`_131_`/`_122_`/`_070_`/`_066_`
  (28, all adjacent to emitted met1 `RECT` fills). Probe record:
  which sites `drc why` ran on, partner-edge class found
  (X1–X4), the fix-or-pin outcome per site. Probe files named
  (`runs/drt-ours/who6.tcl`+ — gitignored).
- **Gate table** (final): per-rule `lef_expressible` ours vs placed
  vs routed_ref — `PINNED_LEF_RESIDUALS` entries named per site;
  `magic_only` decomposition (nwell rows all inherited);
  `unclassified_fired` **empty** (the two first-contact names
  closed by the fixes — record that, don't just omit);
  `sanity_control` result (`met2.2` new-vs-clean).
- **Comparison table**: `delta.vs_baseline`/`vs_delegated` —
  wirelength (−48.5%/−49.4% restated "on the 187 routed nets", the
  82 unrouted emit conns-only), per-cut via census, antenna counts,
  signal nets routed; named outlier explanations.
- **Connectivity record**: `unrouted` final list (82, named via the
  golden's `ours_router.unrouted`) + `unrouted_kinds` (NoPath=44
  Unattachable=58) + `unrouted_blockers` census +
  `stub_outside_cover` state; `notch_fills=221` (dedup'd from 296 —
  the dedup is recorded, the count is never a target);
  `notch_residuals=1`/`hole_residuals=0`; `area_patches=1
  area_residuals=0` (the `_007_`/`_055_` classes closed — name what
  the §2/§3 fixes did).
- **picorv32 deferral — explicit** (memo): the leg is un-run;
  `picorv32.drt.ours.reference.json` is not committed; the ignored
  gate test carries the schema joins for when the leg runs.
- **What ran** (E0 rule 9): which script steps, which legs, which
  container digest (`image_id` recorded), which probes, any partial
  reruns named — record honestly that the post-closure measurement
  ran at `fba5f85` (20:07, transcripts in `runs/drt-ours/`) but its
  step-E output was lost to the driver timeout, so the committed
  golden is emitted by a **fresh full `check_drt_ours.sh gcd` run**
  on the committed DEF (or the post-fix run if attribution lands
  X3/X4). `generate_drt_ours.sh gcd --verify` already demonstrated
  byte-stability at `fba5f85` (`rerun.def` == committed DEF) — name
  that executed arm.

## `docs/e6-drt.md` — e2e section + self-DRC scope correction

- e2e section: gate outcome on gcd — the measured trajectory and
  the named residual set at summary level; mechanisms point at
  `kurena/tasks/e6-drt-e2e/design/` docs, results.md has the numbers;
  picorv32 deferral stated.
- **Self-DRC scope — corrected**: §8's inventory gets precision.
  Post-fix measurement: `findings_total=0` while Magic reports
  `met1.2=32` — the residual class is whatever `drc why` attributes
  (X1/X2 = geometry outside our LEF+DEF model; X3/X4 would mean a
  checker-model gap to name). Amend the scope sentence to exactly
  the measured gap; the claim boundary stands: self-DRC is a fast
  pre-gate, not the deck.

## `docs/status.md` + `docs/roadmap.md`

E7-style done line scoped honestly: "native DRT under committed
guides — Magic `drc(full)` `lef_expressible` on gcd: clean except
`met1.2` = N named sites (per-site in results.md); 82 unrouted nets
named; deltas vs baseline/delegated in results.md; picorv32 leg
deferred". No "fully routed" claim — the line carries the unrouted
count and the residual count.

## Files

- `docs/results.md`
- `docs/e6-drt.md`
- `docs/status.md`
- `docs/roadmap.md`
