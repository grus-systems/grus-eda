# drt-unrouted-census: surface per-pin failure classification

External review (large-scale run, ~8k unrouted): the operator
cannot tell whether unrouted nets died `Unattachable` or `NoPath`,
so a class of failure (e.g. 42 consecutive unrouted nets sharing a
guide region) cannot be root-caused without a debugger. The
classification already exists — `UnroutedNet.failed` carries
per-pin `FailKind` — it is simply never printed.

## Status at v0.12.9 (post-picorv32 leg) — rescope to the remainder

Item 1 is DONE: `unrouted_kinds NoPath=… Unattachable=…` prints
the pin-count aggregate, and `unrouted_blockers l<L><Kind>=n` adds
a per-layer blocker census (`main.rs` ~2079-2095). The picorv32
leg also lands `stub_outside_cover <net> <cell/pin>` records and
`recount_unrouted.py` consumes them. What remains of this ticket:

- Item 2 (per-pin failure detail: pin name + final `FailKind`
  inside the `  unrouted <name>` block) — still missing; the
  current print is net-name-only.
- Item 3 (`region_escape` threading) — still missing entirely.
- Item 4 (digest unchanged) — verify as written.
Also resolve the semantics question the picorv32 recount surfaced:
`u.failed` accumulates across iterations, so pin counts exceed
final-failure counts — document what the printed census means
(ever-failed vs final) and pick the honest one.

## Scope

1. Extend the `== drt emit:` census with an aggregate unrouted
   breakdown: counts per `FailKind` (pins, not nets — a net may
   mix kinds), plus nets with zero failures recorded if that case
   is reachable.
2. Print per-pin detail for unrouted nets in the existing
   `  unrouted <name>` block: pin name + kind. Net names already
   resolve through `guide_net_name`; resolve pin names the same
   way the access/report code does.
3. Thread the pre-maze `region_escape` records
   (`graphs.rs` — pins outside the guide region on a non-top
   layer) into the same per-pin output where the mapping is
   unambiguous, or print them as a separate census line — do not
   silently drop them into `Unattachable` without a note.
4. Keep the digest contract: `route_db`'s serialized digest already
   covers `FailKind`; census output is a print path, not a digest
   input — verify the digest is unchanged.

## Files

- `crates/grus-eda/src/main.rs` (census printing)
- `crates/grus-core/src/drt/maze.rs` (only if the printed data
  needs plumbing — prefer using existing fields)
- `docs/results.md` dated entry (sample on gcd)

Note: queue after `e6-drt-e2e` lands — it is rewriting `maze.rs`
pin-access paths.
