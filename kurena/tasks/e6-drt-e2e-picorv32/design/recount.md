# e6-drt-e2e-picorv32 — unrouted census recount (r2)

The diffreview memo: recount from **committed artifacts** (no
re-emit), describe stub records as what they are, do not equate
them with per-net final kinds, redo the met3+ census honestly,
state whether the v0.12.4 met3-bridge-class exclusion is
supported, and record the method in `docs/results.md`.

## 1. Inputs (committed artifacts only)

- `tests/designs/picorv32/picorv32.drt.ours.reference.json` —
  `ours_router.unrouted` (2,016 names), `ours_router.cli_lines`
  (`unrouted <net>`, `stub_outside_cover <net> <inst>/<pin>`),
  `ours_router.unrouted_kinds`, `ours_router.unrouted_blockers`.
- `tests/designs/picorv32/picorv32.guide` — per-net box lists with
  layer names (`li1`, `met1`–`met5`).

Nothing else is an input: `picorv32.drt.ours.routed.def`,
`runs/drt-ours/*`, and `picorv32.placed.def` are gitignored and
must not be quoted as the recount's source.

## 2. Semantics (what each field actually is)

- `unrouted_kinds` counts **per-pin final failures**
  (`UnroutedNet.failed`, `crates/grus-eda/src/main.rs` ~2074 —
  `nopath`/`unatt` are incremented per failed pin). A net may hold
  pins of both kinds; the emit prints `unrouted <net>` (net name
  only) plus the aggregate line — **no per-pin or per-net kind
  is recorded in the golden**.
- `stub_outside_cover <net> <pin>` is emitted for every pin that
  **ever** had no legal paint — strict + projection attach both
  empty — collected across **all** rip-up iterations and dedup'd
  (`RouteDb::stub_outside_cover`, `maze.rs` ~214–220, dedup
  ~3846). Every final `Unattachable` pin has a record (the gate
  asserts `stub_lines ≥ Unattachable`); the converse does not
  hold — a recorded pin may have re-attached on a later iteration
  (the gcd `_165_` precedent) or ended `NoPath` for another
  reason.
- `unrouted_blockers` is an aggregate per-pin-finding blocker
  attribution (1,973 total across 7 kinds, `l1Net=1,194`
  dominant). It does not split by net or by kind — cite it as an
  aggregate signal only.

## 3. Derivable counts and the kind-split bound

Set definitions (all from the golden):

- `U` = `ours_router.unrouted` = 2,016 nets.
- `S` = distinct nets named by ≥1 `stub_outside_cover` line =
  1,988 nets (3,288 distinct pin records).
- `S∩U` = 1,820 — unrouted nets carrying a stub record.
- `S∖U` = 168 — stub-carriers that routed on a later iteration.
- `U∖S` = 196 — unrouted nets with no stub record.

Kind-split bounds (arithmetic on the golden — no new data):

- Every final-`Unattachable` pin produces a stub record, so nets
  carrying ≥1 final `Unattachable` pin ⊆ `S∩U`, and there are at
  most `Unattachable = 1,480` of them.
- Therefore ≥ `2,016 − 1,480 = 536` unrouted nets' final failures
  are all `NoPath` — this bound includes all 196 `U∖S` nets (a
  final-`Unattachable` pin would have produced a record), and
  ≥ `1,820 − 1,480 = 340` stub-carrying nets ended `NoPath`-only.

**Named gap** (the memo's "say so and name the gap"): the exact
per-net kind split is *not* derivable from committed artifacts —
per-pin kinds exist only in `UnroutedNet.failed` in-process.
Closing it is either (a) the `#[ignore]`d
`test_maze_census_picorv32` leg, which prints
`unrouted <net> pins={kinds:?}` per net (~135 min release, a
re-route, not this ticket), or (b) an emit-schema addition that
prints per-pin kinds — a follow-up ticket's call. The results.md
text states the bounds above and names this gap explicitly; it
must not present 1,480 as a net count.

## 4. Guide-union census (the met3+ recount)

Replicate `guide_union_connected` (`crates/grus-core/tests/drt.rs`
~1552) exactly, over `picorv32.guide` boxes:

- Layer index order `li1 < met1 < met2 < met3 < met4 < met5`
  (`LayerId(2) = met2` in the test).
- Two boxes union when `|Δlayer_index| ≤ 1` **and** their rects
  strictly 2D-overlap (`xl_a < xh_b && xh_a > xl_b && yl_a < yh_b
  && yh_a > yl_b`). Single component under union-find =
  connected.
- Classes per the maze-census classifier:
  - `met2_connected` — ≤met2 union already connected.
  - `met3_bridge` — ≤met2 union disconnected, full union
    connected (met3+ boxes rejoin it; the recorded met2–met3
    edge-domain gap signature).
  - `disjoint` — full union disconnected. Expected 0; the
    classifier asserts it can never appear on real GRT output —
    report the count, do not suppress it.
  - `guide_absent` — net missing from the guide. Expected 0 for
    unrouted nets (unguided nets are the separate `unguided`
    class); report, do not suppress.

Census sets: `U∖S` (the memo's target), `S∩U`, and all of `U`
(the paragraph's class claims span the whole census).

**Expected output** (verified during planning — reproduction,
not a target): `U∖S` → met3+ 58, `met3_bridge` 48,
`met2_connected` 148, disjoint 0, guide_absent 0; `S∩U` → met3+
129, bridged 90, connected 1,730; `U` → met3+ 187, bridged 138,
connected 1,878, disjoint 0. Named bridged examples:
`_03824_` (guide line 47759 block), `mem_ready`, `_00013_`,
`decoded_imm[27]`.

**Verdict on the exclusion**: with 138 bridged unrouted nets (48
of them no-stub), the v0.12.4-era "met3-bridge class does not
appear" claim is **not supported** — the class is present at
scale. Caveat the rewrite must keep: the guide-union class is a
*guide-topology* property, not proof of failure mechanism — a
met2-connected net still fails `NoPath` inside its region. The
dominant class remains `met2_connected` pockets (1,878/2,016).

## 5. The vehicle: `tests/designs/recount_unrouted.py`

Stdlib-only Python 3 (the repo's existing `python3` convention,
same as `check_drt_ours.sh`'s embedded heredocs), generic over
design:

```
python3 tests/designs/recount_unrouted.py picorv32
```

reads `tests/designs/<d>/<d>.drt.ours.reference.json` and
`tests/designs/<d>/<d>.guide` and prints:

- §3 counts and bounds (unrouted, stub lines, distinct stub nets,
  the three set sizes, the Unattachable-carrying upper bound and
  both NoPath-only lower bounds).
- §4 census per set (`U∖S`, `S∩U`, `U`): met3+, met3_bridge,
  met2_connected, disjoint, guide_absent; plus the bridged-net
  name list for citation.
- Exit nonzero (fail loudly) on: missing inputs, `unrouted` set
  vs `unrouted_count` mismatch, `cli_lines` `unrouted` set vs the
  `unrouted` field mismatch, `stub_lines < Unattachable`, or
  census arithmetic not summing (`bridged + connected + disjoint
  + guide_absent ≠ set size`).

Committing the script is required, not optional — E0 rule 1: the
numbers results.md quotes must come from a committed script run
as written. A `gcd` run is a cheap sanity leg (`python3 … gcd`;
its census is small — report it, don't gate on particular
values).

## Files

- `tests/designs/recount_unrouted.py` (new)
- `tests/designs/picorv32/picorv32.drt.ours.reference.json` (input)
- `tests/designs/picorv32/picorv32.guide` (input)
- `tests/designs/gcd/gcd.drt.ours.reference.json`, `tests/designs/gcd/gcd.guide` (sanity-leg inputs)
- `docs/results.md`
