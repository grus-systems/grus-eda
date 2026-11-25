# e6-drt-e2e-picorv32 — docs corrections (r2)

Docs recount only. The landed r1 doc work stands except the items
below; every recount number transcribes from the committed
`recount_unrouted.py` output — never hand-edited (E0 rules 1–2).

## 1. `docs/results.md` — rewrite the census paragraph

Inside the existing `### 2026-11-25 — e6-drt-e2e-picorv32` entry,
replace the **Unrouted census decomposition** paragraph. The
correction is marked, not silent — AGENTS §2 keeps failed records
in the log; a one-line header note names what the merged text got
wrong.

Shape of the replacement:

> **Unrouted census decomposition** *(recounted 2026-11-25 — the
> merged version equated the 1,820 stub-carrying nets with the
> `Unattachable` class and claimed all 196 no-stub nets had
> met2-only guide unions; both wrong on the committed artifacts —
> recount method below)*. `unrouted=2,016`; `unrouted_kinds`
> counts per-pin final failures (`NoPath=2,225`,
> `Unattachable=1,480` — a net may carry pins of both kinds).
> `stub_outside_cover` records 3,288 distinct pins on 1,988 nets —
> every pin that *ever* had no legal paint, accumulated across
> all 8 iterations; 1,820 of those nets are unrouted, 168
> re-attached later. Stub-carrying is **not** the final
> `Unattachable` class: at most 1,480 nets carry a final
> `Unattachable` pin (all ⊆ the 1,820), so ≥340 stub-carriers
> ended `NoPath`-only and ≥536 unrouted nets' final failures are
> all `NoPath` — including all 196 no-stub nets. The exact
> per-net kind split is not derivable from the golden (per-pin
> kinds live only in `UnroutedNet.failed`; the `#[ignore]`d
> `test_maze_census_picorv32` `pins={kinds:?}` printout or an
> emit-schema addition would close it — follow-up material).
> Guide-union census (`guide_union_connected` semantics, ≤met2
> vs all layers): of the 196 no-stub nets, 58 carry met3+ boxes —
> **48 `met3_bridge`** (≤met2 union disconnected, met3+ rejoins)
> and 148 `met2_connected`; over all 2,016: **138 `met3_bridge`**,
> 1,878 `met2_connected`, 0 disjoint, 0 guide-absent — the
> v0.12.4 met3-bridge class *does* appear at scale (`_03824_`,
> `mem_ready`, `_00013_`, `decoded_imm[27]` named bridged
> examples); the guide-union class is a topology property, not a
> proven failure mechanism. Aggregate blocker attribution
> `l1Net=1,194` of 1,973 stays dominant — per-pin, not per-class.
> The 116 → 2,016 growth vs the v0.12.6 mini emit remains the
> access-model refusal story (pre-confinement emits painted those
> stubs anyway); 1,039 of the 2,016 unrouted nets are named
> (`clk`, `count_cycle[*]`, `alu_out[*]`).

Append a **Recount method** line: counted by
`tests/designs/recount_unrouted.py picorv32` (committed; gcd leg
`sanity`) from `picorv32.drt.ours.reference.json`
(`ours_router.unrouted`, `cli_lines`, `unrouted_kinds`,
`unrouted_blockers`) and `picorv32.guide` — union-find per net,
`|Δlayer|≤1` + strict 2D overlap, classes per the
`guide_union_connected` classifier.

Numbers not in `recount.md`'s verified table are filled from the
script's actual output at write time; a divergence is a bug.

Everything else in the entry stands: emit/check-leg numbers,
gate table, residual decomposition, `unguided` discovery,
def_metrics, what-ran, determinism decision — all verified by
the diffreview as consistent.

## 2. `docs/e6-drt.md` — reflow fix only

§9 "Test gate" paragraph: the wrap broke "the two-name
unclassified allowlist" into

```
the two-name
unclassified
allowlist,
```

Rejoin so `unclassified` sits inline (the two-name `unclassified`
allowlist). No content change — §9's aggregate census
(`routed=4728 unrouted=2016 unguided=67`, named pin residuals)
is correct and stays. The v0.12.4-era met3-bridge mention in the
maze section is dated historical record, not the v0.12.9 claim —
untouched.

## 3. `docs/status.md` — verified, no change

The 2026-11-25 `v0.12.9` block quotes only aggregate census
numbers (`guided=6744 routed=4728 unrouted=2016`,
`unguided=67`) — none of the wrong claims appear. Explicitly
confirm while editing; no edit expected.

## 4. `kurena/tasks/e6-drt-e2e-picorv32/design/steps.md`

Annotated per `steps.md` "Post-landing notes" — the r1
diff-audit item 4 under-enumerated the landed diff; recorded so
later audits diff against landed state.

## 5. Version

No `Cargo.toml`/`Cargo.lock` change — docs + a measurement
script; no shipped functionality moves (v0.12.9 stands).

## Files

- `docs/results.md`
- `docs/e6-drt.md`
- `docs/status.md` (verify-only)
- `kurena/tasks/e6-drt-e2e-picorv32/design/steps.md`
