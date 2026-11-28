# drt-unrouted-analysis — Doc deltas (r3: measured, amendment pending)

The dated `results.md` entry, the `e6-drt.md` "Root-cause probe"
subsection, and the `status.md` line all landed and the verify
lanes are now confirmed green on HEAD (r3 planning ran both:
verify 8 cli + 559 lib + 18 drc + 42 drt [4 ignored] + 2 irdrop;
clippy clean — the committed "Checks run" block's counts match).
The picorv32 leg ran once to completion and **measured** its
table (`steps.md` §0) — it failed only the two stale-pin
asserts. What remains is the **picorv32 amendment with the two
named deltas**:

1. **`docs/results.md`** — land the pending picorv32 amendment:
   the measured 13-row table (1,731 nets / 3,083 pins; per-class
   pins + distinct-nets), the anchor block **naming both
   deltas** — unrouted 1,731 vs the spec's pinned 2,016 (stale
   v0.12.9 pin across `drt-guide-escape` + `drt-rayon`; 970
   rescued / 685 newly unrouted vs the committed golden) and
   `geom_met3_bridge` 32 nets vs the spec's 43 (union-census
   lineage 43 → 138 → 142, class count 32 after the
   probe-rescue split — 546 rescued pins) — the
   `region_escape ∩ u.failed` per-class split (73 total),
   `unguided` 67 name-equal to the golden (reproduced),
   `probe_ms=224517` vs the leg's ~53-min route, and the
   picorv32 prioritized target list. Then correct "Checks run"
   to what `steps.md` §4 ran (the re-run of the leg is its own
   record — r1 FAILED on stale pins, r3 outcome).
2. **`docs/status.md`** — the committed line says "picorv32 leg
   in flight — results.md names it": replace with the measured
   headline (1,731 nets / 3,083 pins; 2,544 negotiation vs 539
   geometric; met3-bridge union/class split named).
3. **`docs/e6-drt.md`** — the subsection's tail gains the
   picorv32 headline; check §533's "met3-bridge residual class
   (43 picorv32 nets)" still reads as the historical v0.12.4
   count — fix in step if it presents 43 as current (E0 rule 8).

## 1. `docs/results.md` — one dated entry (landed; amendment pending)

`### 2026-11-27 — drt-unrouted-analysis: root-cause
decomposition of the unrouted population` (date = the day the
run completes; bump `[workspace.package] version` in the same
commit, patch: 0.16.0 → 0.16.1).

Required content, in order:

1. **Method** — one paragraph: standalone post-pass
   `probe_unrouted`; probe occupancy = `premark` minus io-port
   `Pin` marks (`Obs`/`ObsPin`/PDN retained); per-net graph +
   `NetTopo`/`NetAdj` rebuild + `route_one_net`; `OccMode::
   Disabled` rejected because the paint-side checks
   (`fill_blocked`, `try_pin_paint`) are mode-blind — the
   mechanism demands physically-absent marks.
2. **Decomposition table per design** — every class (zero
   rows kept), pins + distinct-nets columns; the geometric vs
   negotiation split named.
3. **Anchor block** — met3_bridge: bridged-union nets split
   into `geom_met3_bridge` vs probe-rescued (negotiation),
   vs the pinned census numbers (43 maze-census / 138+48
   emit-leg recount — the current-leg numbers are what the
   run measures; any delta is named). `region_escape ∩
   u.failed` per-class split. `unguided` count vs golden
   (gcd 0, picorv32 67).
4. **Probe cost** — `probe_ms` vs `route_ms` per design, the
   exact invocation line (the tests in `tests.md` §1-2 —
   opt-in post-pass, not a flag), and the one-line cost model
   (one premark + one attempt per unrouted net).
5. **Prioritized target list** — classes ordered by pin
   count, each with the fix direction from `taxonomy.md` —
   e.g. `neg_wave` → rip-up/ordering/negotiation work;
   `geom_met3_bridge` → met2–met3 transition domain;
   `geom_obs_sealed` → access-geometry/attach-ladder;
   `geom_no_in_region`/`geom_off_track` → guide coverage /
   access arcs. Numbers only from the measured run — the
   list is the ticket's deliverable, not a guess.
6. **Prove-it-red record** — the fixture inject/revert arm
   (`tests.md` §4 arm C) and the classifier-break arm (§5):
   what was injected, what class was expected and observed,
   the failure text of the red run, the revert.
7. **What ran** (E0 lesson 9) — the exact commands: the two
   verify commands, `test_unrouted_probe_gcd` (+ which
   profile), the `#[ignore]`d picorv32 command if run, the
   red-arm reproduction. Name anything not run (e.g. "the
   picorv32 probe leg is `#[ignore]`d; if it did not run this
   session the entry says so and the picorv32 table is
   marked pending" — an entry with a missing leg names the
   missing leg, never extrapolates).
8. **Routed-output-unchanged evidence** — `GCD_ROUTE_DIGEST`
   value before/after (same run vs detached HEAD is not
   required — the pin + the byte-compare e2e test cover it;
   state which ran).

## 2. `docs/e6-drt.md`

§7 "Unrouted taxonomy" paragraph (~737-742) gains a following
subsection, **"Root-cause probe"**: probe occupancy
construction (`premark` minus `Pin`, why `Disabled` is
insufficient — the `fill_blocked`/`try_pin_paint`
mode-blindness), the per-net re-attempt shape, the
geometric/negotiation split with the named subclasses (one
line each, pointing at `taxonomy`-level semantics not the
full tree — the census doc stays a map, the probe module
holds the mechanics), the residual-class rule, and the
`unguided` row semantics. Also note the invocation: opt-in
post-pass called by `tests/drt.rs`, not a `grus-eda drt`
flag (no CLI change this ticket).

If §7's `route_nets` description drifts while implementing
(e.g. `premark_obs` renamed internally), fix the text in the
same change (E0 lesson 8).

## 3. `docs/status.md`

`Last updated:` line — `v0.16.1` `drt-unrouted-analysis`,
one sentence: unrouted population decomposed into
geometric/negotiation root-cause classes by a post-pass
probe; counts pinned on gcd (+ picorv32 if its leg ran);
routed output unchanged. The E6 row's "Known gaps" cell may
gain one clause pointing at the decomposition table location
if the results justify it — keep it to one clause.
