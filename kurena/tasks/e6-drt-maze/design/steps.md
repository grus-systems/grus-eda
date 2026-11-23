# e6-drt-maze — Implementation steps (resume from `a52d771`)

The checkpoint is committed; the tree is clean and does not compile.
Remaining work is one logical unit: finish the `relax` refactor,
run the gates, write the docs → one commit (plus the version bump,
same commit per repo rules).

## 1. Finish the `RelaxedVia` refactor (`drt/maze.rs`)

`cargo check -p grus-core` today reports 9 errors. Fixes:

1. **`relaxed_vias` `b`-endpoint bug** (lines ~810, ~832): the code
   passes `jx`/`jy` — `usize` indices into the `ixs`/`iys` line
   lists — where `node_id(l, ix: u32, iy: u32)` expects the track
   *index values*. Both a type error and a wrong-node bug:
   - Horizontal arm: `b: node_id((k + 1) as u8, ixs[jx], jy)`
   - Vertical arm:   `b: node_id((k + 1) as u8, jx, iys[jy])`
2. **`edge_collisions`** (def ~line 639): add
   `relax: &[RelaxedVia]` after `occ` — its via branch already
   dereferences `relax` at ~line 693. Update the call in
   `route_one_net`'s extraction loop (~line 1277) to pass `&relax`.
3. **`NetRun::new`**: add `relax: &[RelaxedVia]`; after the strict
   `via_edges` loop, push arcs `(rv.a, rv.b, v_plane + ri)` and
   `(rv.b, rv.a, v_plane + ri)` for each entry (pre-gated —
   `relaxed_vias` already skips `default.is_none()` pairs, so add
   unconditionally). *The compiler cannot catch this one:* without
   arcs, relaxed edges exist in the index space but are unreachable.
4. **`extend_once`**: add `relax: &[RelaxedVia]`; pass to
   `edge_cost` (~line 1019) and `other_end` (~line 1050).
5. **`route_one_net`**: `let relax = relaxed_vias(g, tech, tracks,
   vias);` before `NetRun::new` (computed once per call — the
   per-iteration recompute; hoisting is the recorded optimization
   lever, `occupancy.md` §5). Pass `&relax` to `NetRun::new` and
   `extend_once`. Fix the extraction via branch (~lines 1217-1222):
   split `eidx < v_plane` (strict, existing) from `else` (relaxed —
   currently `g.via_edges[eidx - e_plane]` panics out-of-bounds).
   For a relaxed edge emit **both** the `ViaPlace` at
   `(rv.lower, rv.x, rv.y)` and the implied stub wires:
   `a.xy → (rv.x, rv.y)` on the lower layer (one coord shared —
   exactly one of the two endpoints may already coincide) and
   `(rv.x, rv.y) → b.xy` on the upper — pushed into `hlines`/
   `vlines` so the existing merge produces canonical `WireSeg`s.
6. **Delete the diagnosis scaffolding**: `debug_edge_stats`,
   `debug_pin_approach`, `debug_net_wave` (each doc comment says
   "Removed before the ticket lands") and `test_maze_debug_gcd` in
   `tests/drt.rs`. This clears the remaining stale call sites at
   ~1350/~1393/~1488/~1509; if a fresh investigation needs them they
   are recoverable from `a52d771`.
7. Verify: `cargo check -p grus-core` clean;
   `cargo test -p grus-core --lib maze` — in-module unit tests
   (band math, perpendicular crossing, projection fallback, PQ tie
   order, square-graph path).

## 2. Synthetic gate

`cargo test -p grus-core --test drt test_maze_cases` — the four
acceptance cases. Watch case 2 (`ViaPlace { lower:0, x:4000,
y:3000 }`): strict and relaxed arcs tie on `VIA_COST`; the CSR must
enumerate strict before relaxed (step 1.3 appends relaxed last) so
the strict position wins ties — check ordering before touching the
assert. Case 4 relies on relaxed edges *not* bridging the fixture's
split region (no met2 box ⇒ no upper nodes ⇒ no relaxed edge).

## 3. Real-design census

`cargo test -p grus-core --test drt test_maze_census -- --nocapture`
(gcd, then picorv32 — `picorv32.placed.def` is present in the
worktree). Collect the printed line (guided / routed /
pins_connected / projected / unrouted / collisions / iterations /
ms / digest) for `results.md`. Relaxed met2–met3 transitions may
route the met3-bridge class outright — if `unrouted` comes back
nonzero, every `NoPath` must classify (existing met3-bridge probe or
a new named cause pinned in the test comment; do not weaken the
`unclassified.is_empty()` assert silently — name the class).

## 4. Fail-proof + determinism

`test_maze_occupancy` (gcd Enabled vs Disabled — printed digests +
census counts for `results.md`) and `test_maze_determinism`
(double-run `RouteDb` equality + nonzero digest, both designs).

## 5. Workspace gates

`cargo test --workspace` and
`cargo clippy --workspace --all-targets -- -D warnings`. picorv32
runtime is measured by the census test — record the number; if hot,
apply the `occupancy.md` §5 lever and record it.

## 6. Docs + version (same commit)

- `docs/e6-drt.md` §7 per `docs.md` — must record the relaxed-via
  model change (preferred-track crossing, implied stubs, single-
  nearest rule, guide-box looseness) and the pin-carve exemption.
- `docs/results.md` dated entry with the **measured** census
  numbers — never estimates (AGENTS §2).
- `docs/status.md` head paragraph; `docs/roadmap.md` E6 line.
- `Cargo.toml` `[workspace.package] version 0.12.3 → 0.12.4`.

## Commit

One commit: `feat: Sequential maze router over the track graph`
(Conventional Commits, English, agent trailer). The checkpoint diff
plus this remainder is one unit of work.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/tests/drt.rs`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
