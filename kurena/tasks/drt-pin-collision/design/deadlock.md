# Deadlock avoidance — why the fix cannot hang

The pre-change rationale (recorded in `maze.rs` ~L268–274) is real:
rip-up cannot move a pin, so a mark a wire can never satisfy could
churn the queue forever. The design keeps the mark's price soft in
early iterations but gives every exhaustion path a named terminal
state. Enumerate the cycles:

## Cycle 1 — wire re-queue on a pin hit

A committed wire takes a foreign `Pin` hit → directed provisional
`Collision{wire, pin_net}` → `bad[i+1]` contains the wire net, which
is ripped and re-routed with `hist + 1`. Bounded:

- `hist` escalation is monotone per net — the same hit costs
  `OCC_PENALTY + hist·HIST_STEP` more each round, so a priced detour
  eventually wins if one exists.
- The pin's net is **never** re-queued on its own port being hit
  (the directed `prov_pins` set) — its marks are premarks, so
  re-queuing it could not change anything anyway. No bilateral
  ping-pong amplification is added: a wire×pin record contributes
  exactly one re-queued net.
- The loop is capped (`MAX_ITERS = 8`); the hard iteration cannot
  record (hits block), so `it == cap` always terminates `bad`.

## Cycle 2 — only path crosses the pin

A net whose *every* wave path crosses a foreign port band: soft
iterations commit the overlap and re-queue it; the hard iteration
finds no legal edge → `NetOutcome::Failed` → `UnroutedNet` with
per-pin `FailKind::NoPath` and `blockers` attributing `(layer,
3=Pin)` (postmortem arm §4 extended). Named failure, emitted in the
census — the acceptance's "or a named failure" arm.

## Cycle 3 — attach-stub exhaustion

A pin whose projected candidates all imply stubs through foreign
marks: `project_attach` (topo, premark occupancy) and
`try_pin_paint` (extraction, live occupancy) each exhaust to the
next candidate, then `projected = false`/`stub_blocked` →
`Unattachable` + `stub_outside_cover` census entry. Both loops walk
finite candidate lists; the closure `Reject` path drops the element
and re-enters `try_pin_paint` on the next candidate — bounded by
`tried_proj` growth.

## What is *not* introduced

- No new `Occupant` kind → no mark the rip-up journal cannot free.
  `Pin` marks are premarks: never inserted by `commit_route`, never
  in `marks[i]`, never unmarked. Re-queue order and mark lifetimes
  are unchanged.
- `OccMode::Disabled` semantics unchanged: `hit_verdict` returns
  `Some(0)` before the foreign scan, `edge_collisions` and both
  censuses are skipped — the fail-proof knob still compares
  occupancy on/off.
- The `unrouted_blockers` CLI line gains `Pin` attributions for free
  — no format change (kind 3 was already encoded).
