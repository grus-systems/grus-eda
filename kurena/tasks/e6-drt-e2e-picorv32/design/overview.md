# e6-drt-e2e-picorv32 — overview (r2, post-diffreview docs recount)

**r1 status: landed.** Commits `8496158` + `9788e53` (v0.12.9):
`picorv32.drt.ours.reference.json` committed (step-E output of the
committed `check_drt_ours.sh` in the pinned image),
`test_e2e_ours_reference_picorv32` un-`#[ignore]`d and green, the
card-#44 `unrouted_kinds` fail arm landed plus the `unguided` step-E
derivation it surfaced, `li.3`/`met1.2` pinned per-design, and the
`--verify` double-emit proved byte-identical
(`OURS-DETERMINISTIC 505baea2f937702f`). The r1 design files
(`emit-leg.md`, `gate.md`, `script-fix.md`, `residuals.md`) are the
record of that landed work.

**r2 scope — docs recount only.** The diffreview verified the leg
(golden internally consistent, determinism byte-identical, gates
pass) but found two factual errors in the `docs/results.md`
unrouted-census decomposition that propagate to follow-up tickets:

1. **Stub-carrying ≠ `Unattachable`.** The merged text equated the
   1,820 stub-carrying unrouted nets with the `Unattachable` class.
   The golden's `unrouted_kinds.Unattachable = 1,480` counts
   *pin-failures*; `stub_outside_cover` records are pins that
   *ever* had no legal paint, accumulated across all 8 iterations.
   ≥340 stub-carrying nets therefore ended `NoPath`-only, and the
   exact per-net kind split is not directly derivable from the
   golden — the honest bounds and the named gap are specified in
   `recount.md`.
2. **The met3-bridge class does appear.** The merged text claimed
   all 196 no-stub nets have met1/met2-only guide unions. Recount:
   58 of the 196 carry met3+ boxes in the committed
   `picorv32.guide` (`_03824_` at guide line ~47,764, `mem_ready`,
   `_00013_`, `decoded_imm[27]` among them); 48 of the 196 are
   `met3_bridge` by the `guide_union_connected` classifier. The
   v0.12.4 met3-bridge-class exclusion is **not supported** —
   138 of all 2,016 unrouted nets are bridged.

No design/implementation change: no re-emit, no golden
regeneration, no `.rs`/`.sh` behavioral edits. The remaining work
is a docs correction — recount from committed artifacts via a
committed reproduction script, rewrite the results.md paragraph
honestly, fix the `docs/e6-drt.md` reflow artifact, annotate the
stale r1 `steps.md` checklist item 4, and rerun the affected
gates to confirm green.

## Scope (r2)

1. **Committed recount script** — `tests/designs/recount_unrouted.py`
   (stdlib-only): joins the golden's `ours_router` block to the
   committed guide, reproduces the `guide_union_connected`
   classifier, prints the census table + kind-split bounds.
   E0 rule 1: every number quoted in `docs/results.md` comes from
   the committed script, run as written. Details: `recount.md`.
2. **`docs/results.md` correction** — rewrite the "Unrouted census
   decomposition" paragraph of the 2026-11-25 entry with the honest
   split, mark it as a recount superseding the merged claim
   (failed-record discipline, E0/AGENTS §2), and record the
   recount method. Details: `docs.md`.
3. **`docs/e6-drt.md` reflow fix** — rejoin "the two-name /
   `unclassified` / allowlist" (the §9 "Test gate" paragraph).
   §9's aggregate census numbers are correct and stand.
4. **Stale-checklist note** — annotate `steps.md`'s r1
   diff-audit item 4, which under-enumerated the landed diff
   (`unguided` step-E derivation, gcd golden regen, `drt.rs`
   joins). Details: `steps.md` "Post-landing notes".
5. **Gate rerun** — `cargo test --workspace` + clippy
   `-D warnings` to confirm green; record the verdict line.

## Verified recount values (planning-verified; the script must reproduce)

From `picorv32.drt.ours.reference.json` + `picorv32.guide`:

| quantity | value |
|---|---|
| unrouted nets | 2,016 |
| `unrouted_kinds` (per-pin final findings) | NoPath 2,225 / Unattachable 1,480 |
| `stub_outside_cover` lines (distinct pins) | 3,288 on 1,988 nets |
| stub ∩ unrouted / routed-later | 1,820 / 168 |
| no-stub unrouted | 196 |
| no-stub met3+ / bridged / met2-connected / disjoint | 58 / 48 / 148 / 0 |
| all-unrouted met3+ / bridged / connected / disjoint | 187 / 138 / 1,878 / 0 |
| kind-split bound | ≤1,480 nets Unattachable-carrying; ≥536 all-NoPath incl. all 196 no-stub; ≥340 stub-carriers NoPath-only |

Any divergence between the committed script's output and this
table is a bug to investigate, not a rebaseline.

## Acceptance traceability (r2 delta)

| item | state |
|---|---|
| golden committed + provenance | landed, unchanged |
| gate test live + pinned | landed, unchanged |
| `unrouted_kinds` contract fix | landed, unchanged |
| `docs/results.md` dated entry | **correction**: unrouted census recount + method |
| tests + clippy green | re-run to confirm |

## Files

- `tests/designs/recount_unrouted.py` (new, committed)
- `docs/results.md` (census paragraph rewrite + method record)
- `docs/e6-drt.md` (reflow fix)
- `docs/status.md` (no change — aggregate numbers already correct)
- `kurena/tasks/e6-drt-e2e-picorv32/design/steps.md` (stale-item annotation)
