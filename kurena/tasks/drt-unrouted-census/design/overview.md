# drt-unrouted-census — Overview

E6 print-path ticket. The unrouted census already knows *why* each
pin failed — `UnroutedNet.failed` carries `(PinId, FailKind)` and
`RouteGraphs.region_escape` names the pre-maze escapes — but the
`== drt emit:` block prints only net names. At picorv32 scale
(2,016 unrouted — the v0.12.9 leg) an
operator cannot tell `Unattachable` (no legal paint / no attach
candidate) from `NoPath` (attachable, unreachable) without a
debugger. This ticket surfaces the per-pin classification and the
`region_escape` provenance in the existing emit output.

## State at v0.12.9 (what is already landed)

- `unrouted_kinds NoPath=… Unattachable=…` — the per-pin *final*
  `FailKind` aggregate (`main.rs` ~2070-2079). Spec item 1's count
  half is done; the "zero-failure nets" arm is unreachable
  (`semantics.md` §3).
- `unrouted_blockers l<L><Kind>=n` — per-layer wave-death postmortem
  (`main.rs` ~2080-2097). Aggregate only; `blockers` carries no pin
  id, so per-pin blocker attribution is out of scope.
- `stub_outside_cover <net> <cell/pin>` — the ever-failed paint
  census (`main.rs` ~2101-2116), consumed by
  `tests/designs/recount_unrouted.py`.

## What remains (this ticket)

1. **Per-pin failure detail** — inside each `  unrouted <net>`
   block, one nested line per `u.failed` entry:
   `    fail <label> <Kind> [region_escape]` (`format.md`).
2. **`region_escape` threading** — `graphs.region_escape` is a
   `(pin, net)` record; the join is unambiguous, so it is a suffix
   token on the `fail` line, not a separate census (`semantics.md`
   §2 for the taxonomy it creates).
3. **Digest contract** — `route_digest` serializes
   `u.failed` (pin + kind) and `stub_outside_cover` already; the
   change touches only `println!` paths, so the digest is unchanged
   by construction. Verified, not assumed (`steps.md` step 4).
4. **Semantics resolution** — the goal asks "ever-failed vs final";
   `u.failed` is already the final hard-iteration classification
   (`semantics.md` §1). The accumulating field is
   `stub_outside_cover`. Print the final kind — it is the honest
   one: it matches the emitted DEF's connectivity truth.

## Decisions

1. **`  unrouted <net>` stays byte-identical; detail nests below
   it.** `check_drt_ours.sh` step-E anchors `^\s+unrouted (\S+)$`
   and `recount_unrouted.py` requires `len(parts) == 2` — kinds on
   the net line would break both consumers. Nested `    fail …`
   lines carry the detail (`format.md`).
2. **Iterate `route_db.unrouted`, not `report.unrouted`.** Emit's
   report pushes `guide_net_name(u.net)` in the same order, so the
   printed names are identical; iterating `route_db` directly keeps
   the `UnroutedNet` (kinds, pins) in hand instead of relying on a
   parallel-array invariant.
3. **`region_escape` is a provenance token, not a third `FailKind`.**
   An escape can end `Unattachable` (never had any candidate) or
   `NoPath` (recovered via projection/access arcs, still
   unreachable) — both get the flag (`semantics.md` §2).
4. **`fail` lines join the `cli_lines` keep-list** in
   `check_drt_ours.sh` so the *next* golden regeneration carries
   the per-pin evidence. Committed goldens are untouched — they are
   snapshots; nothing asserts the new lines (`consumers.md`).
5. **No `maze.rs` plumbing** — every datum the print needs is
   already on `RouteDb`/`RouteGraphs` and in scope in the `Drt`
   arm. The only core-side change is a doc comment on
   `UnroutedNet.failed` naming the final-iteration semantics.
6. **No aggregate `region_escape` count line** — `grep -c
   ' region_escape'` on the log answers it; keep the census
   surface minimal.

## Output shape (target)

```
== drt emit: gcd ==
guided=269 routed=188 unrouted=81 …            (unchanged)
unrouted_kinds NoPath=46 Unattachable=52       (unchanged)
unrouted_blockers l0ObsPin=1 l1Obs=4 l1Net=39  (unchanged)
  unrouted <net>
    fail _<inst>_/<pin> NoPath
    fail _<inst>_/<pin> Unattachable
    fail <port> Unattachable region_escape
  unrouted <net2>
    fail _<inst>_/<pin> NoPath
  stub_outside_cover <net> <label>             (unchanged)
  …residuals…                                  (unchanged)
```

## Risks

- **Consumer drift** — any token after `<net>` on the `unrouted`
  line breaks step-E and the recount; the nested-lines decision is
  load-bearing (`format.md` §Constraints).
- **Vacuous `region_escape` on gcd** — gcd's 17 escapes mostly
  re-attach via projection; whether any of them land in
  `u.failed` is measured, not assumed. If zero carry the flag on
  gcd, the results entry says so (the picorv32 leg — 592 escapes —
  is the real coverage; deferred like every picorv32 emit leg).
- **`fail` vs `findings` prefix collisions** — checked: no emit
  line starts with `fail ` today; the keep-list addition is a
  single arm (`consumers.md`).

## Files

- `crates/grus-eda/src/main.rs`
- `crates/grus-core/src/drt/maze.rs`
- `tests/designs/check_drt_ours.sh`
- `tests/designs/recount_unrouted.py`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `Cargo.toml`
