# e6-drt-e2e — doc updates (r3)

All in `docs/`, written at the end of implementation with the final
measured numbers — not pre-written claims.

## `docs/results.md` — dated entry

Structure (E7-style, plus the fix trajectory):

- **Trajectory line**: `lef_expressible` violations on our DEF
  ~3025 (first contact) → 847 (`d8d0c6c` mask) → 514 (`4c5e2cb`
  confinement WIP) → final. Each hop names what landed and what was
  measured, including the rung-by-rung `unrouted` series from the
  access ladder (`pin-access.md`).
- **Root-cause record** (the r3 measurement, evidence-attached):
  `met1.2=740`/`met2.2=56` decomposed into the two same-net classes
  (45 plate-pair sites ≈665 tiles; ~75 wire-end tiles; 15 met2 jog
  sites); the `drc why` + flattened-dump anatomy; the `who5.tcl`
  move-via causality proof (135→600 DBU gap clears the site); the
  `.mag` inspection refuting subcell-internal met1 for the sampled
  cells; the census tables. Probe files named
  (`runs/drt-ours/who*.tcl`, `region.mag`, `gcd.whofix.def` —
  gitignored).
- **Gate table** (final): per-rule `lef_expressible` ours vs placed
  vs routed_ref, `magic_only` decomposition, `unclassified_fired`
  status, `sanity_control` result.
- **Comparison table**: `delta.vs_baseline`/`vs_delegated` —
  wirelength, vias per cut layer, antenna counts, signal nets routed;
  named outlier explanations (unrouted nets carry zero geometry —
  the wirelength −35.55% reading is restated as "on the 196 routed
  nets" with the unrouted names listed, per the honest-delta
  convention).
- **Connectivity record**: `unrouted` final list + `FailKind` +
  `unrouted_blockers` census + `stub_outside_cover` final state;
  `notch_fills` count (the repair cost, recorded not hidden).
- **What ran** (E0 rule 9): which script steps, which legs, which
  container digest; any partial reruns named.

## `docs/e6-drt.md` — e2e section + §7 correction

- e2e section: gate outcome, both designs, the fix narrative at
  summary level (self-spacing closure + access model — mechanisms
  point at `kurena/tasks/e6-drt-e2e/design/` docs, results has the
  numbers).
- **§7 self-DRC scope — corrected and expanded**: the inventory
  already records "checks foreign-spacing only; same-net paint is
  free-for-adjacency". Amend: the same-net blind spot is a *measured*
  gap — it reported `findings_total=0` on geometry Magic flags at
  796 fragments; the emit-side closure pass owns that class, not
  self-DRC (or, if self-DRC is extended to catch it, the record
  states the new scope). Claim boundary stays honest: self-DRC
  remains a fast pre-gate, not the deck.
- Emit-grammar note: `RECT` fill pieces in the NETS section — what
  they are and why they exist.

## `docs/status.md` + `docs/roadmap.md`

E7-style done line scoped honestly: "native DRT under committed
guides — clean on the Magic `drc(full)` `lef_expressible` subset on
gcd[/picorv32], N unrouted residuals named (if any), deltas vs
baseline/delegated in results.md". If `unrouted` survives, the line
carries the count — no "fully routed" claim.

## Files

- `docs/results.md`
- `docs/e6-drt.md`
- `docs/status.md`
- `docs/roadmap.md`
