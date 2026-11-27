# docs — repository text that moves with the code

AGENTS rule 8 (`docs/` in step with the code) applied to the
parallel-scheme change. All repo text stays English.

## `docs/e6-drt.md`

- **§7 opening** (L447–449): "sequential, deterministic, one
  `NetGraph` at a time in `join.nets` order" → the snapshot-search /
  ordered-commit model: soft iterations run `route_one_net` over the
  iteration-start occupancy in parallel and commit serially in `bad`
  order; the last (hard) iteration stays a serial interleave; output
  is deterministic for any thread count but **not**
  serial-identical — the paragraph must say both halves.
- **§7 "Order and rip-up"** (L643–658): the provisional-collision
  scan's home moves from extraction (`route_one_net` tail) to the
  commit step — the paragraph names that `edge_collisions` runs at
  commit against live `occ`, which in soft iterations includes
  same-iteration earlier commits (stale-collision detection under
  snapshot search); gating (`!hard && Enabled`) unchanged.
- **§7 Occupancy paragraph** (L560–584): note `ScanStats` counts via
  `AtomicU64` (why: `Cell` blocked `Sync`; totals stay exact — the
  query stream is a pure function of the snapshot).
- **§5 graph paragraph** ("Emission order is fixed … byte-identical",
  L259–261): build runs per-net in parallel with a serial
  `join.nets`-order fold — byte-identity holds and is pinned.
- **§8 emission** (L739+): `apply_routes`/`nets_section` assemble
  per-net in parallel, fold serially — the emitted text is
  byte-identical; the section's ordering paragraph gains the
  one-line note (the ordering contract itself does not change).
- New §7 sub-block or appended paragraph: thread control — all drt
  parallel sections run inside a per-call `rayon::ThreadPool`
  reading `RAYON_NUM_THREADS` at call time (why: the global pool
  pins the env var at first use, untestable under `cargo test`).

## `docs/results.md`

One dated entry per `measurement.md` §5: phase-split baseline table,
wall table (baseline / threads=1 / threads=8 medians), digest old →
new with the snapshot-semantics explanation, DEF sha + golden
regeneration record (which scripts ran, verbatim), picorv32 arm,
prove-it-red transcript tails, exact command list (E0 rule 9: say
what was run).

## `docs/status.md`

The "Last updated" line for the version bump, in the occupancy-ds
style: `v0.14.3 drt-rayon` — one sentence naming the scheme, the
recorded gcd `route_ms`/`total` before → after, and that output is
deterministic at any thread count while no longer serial-identical
(digest re-pinned). The E6 row's residual text does not change —
the stage-gate criteria (Magic taxonomy joins, deterministic DEF)
are unchanged, only the runtime column's spirit improves; note the
new `route_ms` inside the E6 row's existing performance mention if
present.

## `AGENTS.md` / `README.md`

No changes — the verification discipline and stage map are
untouched.
