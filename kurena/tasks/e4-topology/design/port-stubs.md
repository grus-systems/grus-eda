# Port rung stacks — keeping `*P` pins off dead islands

## Root cause (verified on `resp_val`)

`resp_val`'s DEF PINS record carries `+ LAYER met3`, so
`pin_attach`/`pin_conn_layer` resolves `conn = met3` and the pin lookup
tries `(grid_pt, met4)` then `(grid_pt, met3)`. `build_guides` covers
the port GCell with a `met1` strip only: with `cover_layer = met1`, the
`met3` access falls into `stub_extra` (`access >= cov`) → emitted as an
unpairable zls → `(P,met3)` is a dead self-loop island → the pin
attaches to it → dead `*P` → OpenSTA's lumped fallback (+250.8 ps, the
gcd max).

Reference behavior (`gcd.guide`, measured): the port cell is covered on
`met1`+`met2` strips and stacked `met2`+`met3` boxes — consecutive
same-point zls pair `met2→met3`, so `(P,met3)` chains to the wire. The
port lands on a live node (`resp_val:2`), whose unpaired top stub still
keeps its `n n r` self-loop — a live-node residual, not a dead island.

## Pairing algebra (the constraint everything hangs on)

`updateVias` pairs a zero-length segment (zls) only **upward**: a zls on
`L` becomes an `L→L+1` via when the *next* segment is on `L+1` at the
same point (rule 1), or a zls on `L` after a segment on `L+1` is
*raised* into an `L+1→L` via (rule 2, the end-cell tail). Consequences:

- A contiguous **ascending** zls stack `[S, S+1, …, A]` at one cell
  pairs each rung into the next → vias `S→S+1→…→A`, so nodes
  `(P,S..A)` all join. The bottom rung `S` interns the same node as the
  covering strip's wire endpoint — that is the whole bridge.
- A descending stack fails: `[m3z, m2z, m1z]` yields only `m3→m2` plus a
  `met1` self-loop — `(P,met1)` never reaches `(P,met2)`.
- A zls below the strip pairs into the wire directly (`[m1z, met2-wire]`
  → `m1→m2`), but a zls **at/above** the strip layer cannot pair into it
  (`met2z` needs a `met3` seg next). High accesses are only reachable
  through the self-contained rung stack anchored on `S`.

## Emission rule

Replace `stub_chain`/`stub_extra` for **bound** pin cells (covering
strip on layer `S`) with a single contiguous zls stack, computed per
cell from `accesses` (the pin layers at the cell) and `S`:

- `high = accesses ∩ [S, ∞)`, `low = accesses ∩ (-∞, S)`.
- `high` empty (all instance pins today: `li1` < `met1` ≤ S): stack =
  contiguous `min(low) .. S-1` — identical to today's chain (`{li1}` →
  `[li1]` over met1, `[li1, m1]` over met2; the `met1` rung falls out of
  the contiguous fill).
- `high` non-empty: stack = contiguous
  `min(low ∪ {met1} ∪ {S}) .. max(high)` — every rung from the lowest
  access/fill up to the top access. The `met1` fill keeps today's
  `met1↔met2` bridge at pin cells that are also bends (dropping it
  would re-detach the `met1` side — verified against the pairing
  algebra); `S` must sit inside the span because it is the rung whose
  node the wire shares. `{m3}` over `met1` → `[m1, m2, m3]`; `{m3}`
  over `met2` → `[m1, m2, m3]`; `{m2}` over `met2` → `[m1, m2]`;
  `{li1, m3}` over `met2` → `[li1, m1, m2, m3]`.

Ordering by stub-group position (the existing start/interior/end cases
stay; only the group's contents change):

- **before-wire** (strip start, interior split, unbound-adjacent
  `group_before`): emit the stack **ascending**. For `high` empty the
  top rung `S-1` lands adjacent to the wire and pairs into it (today's
  behavior). For `high` non-empty the bottom rung `S` shares the wire's
  node, each rung pairs into the next, and the top `max(high)` zls keeps
  its self-loop — on a now-live node.
- **end cell** (`group_after`): emit the stack's below-`S` rungs
  **descending** `[S-1 … min]` first — `S-1` raises into the wire
  (today's tail convention; lower rungs keep reference-equal
  self-loops) — then its `S`-and-above rungs **ascending**
  `[S … max]`; the `S` rung shares the wire node and the stack pairs
  itself (`{m3}` over `met2` at an end cell → `[wire, m1z, m2z, m3z]`:
  `m1z` raises `met2→met1`, `m2z→met2→met3` via, `m3z` self-loop on a
  live node). For `high` empty this is exactly today's `[m1z, li1z]`-
  style tail. Never emit the `≥S` rungs descending and never place them
  before the below-`S` rungs at an end cell — both orders were checked
  against the pairing algebra and leave a dead sub-island.
- **unbound** pin cells: unchanged (`[accesses ∪ {met1}]` ascending,
  inherent residual — no strip exists to anchor a rung).

## Coverage notes / residual cases

- Instance pins are untouched (`access = li1` < every `S`) — the rule
  fires only for `pin_io_layer` accesses (`met2`/`met3` on sky130),
  i.e. exactly the `*P` class.
- A port whose attach cell (`pin_attach` `grid_pt` — majority vote over
  conn-layer box middles + single-track snap) differs from its
  `pin_pos_dbu` GCell gets the rung at the **pos cell**; if the lookup
  then misses at `grid_pt` it falls back to the nearest reached node —
  still live. Dead `*P` can only persist when a conn/`conn+1` node at
  `grid_pt` exists *on a dead island* — the census must list any such
  residual per net (expected: cells where the route covers the pos cell
  but not the voted cell, or genuinely uncovered cells — the same class
  the reference's own 7 dead `*P`s carry).
- Unbound port cells stay dead by construction (no strip to anchor to —
  re-routing would be required; out of scope, record per net).
- Box delta: +`max(high) − S` boxes per bound high-access cell (gcd ~tens
  of ports; picorv32 ~hundreds of port cells) — inside the ±15% gate.

## Invariant asserted

After this change + `bend-vias.md`, every `*P` pin on a bound cell
lands on the driver component: dead `*P` census → 0, or the residual
list matched net-for-net against the reference's own dead-`*P` class
(`probe.md` §3).
