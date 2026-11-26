# drt-converge-stats — Data model & semantics (r3)

Semantics contract for the landed code (`maze.rs`, HEAD 08141b1)
— keep in step with the code (E0 lesson 8). r3 corrects the
re-queue/monotonicity claim per the review memo; nothing else
changed.

## `IterStat` — `maze.rs:194-212` (unchanged)

```rust
pub struct IterStat {
    /// `bad.len()` at iteration entry; entry 0 is always
    /// `graphs.nets.len()`.
    pub attempted: u32,
    /// Routes committed this iteration — counts attempts, not
    /// distinct nets.
    pub routed: u32,
    /// Deduped provisional `Collision`s this iteration's commits
    /// recorded — the re-queue trigger. 0 on the hard iteration
    /// and under `OccMode::Disabled`.
    pub prov_collisions: u32,
}
```

`RouteDb.iter_stats: Vec<IterStat>` (`maze.rs:227-231`):
`iter_stats[i]` describes iteration `i + 1`; `len() ==
iterations` and ≤ `max_iters` by loop construction. Collection
is additive at `maze.rs:3755, 3763, 3768, 3776, 3809-3813` (push
after the per-net `for` closes, before `if hard { … break; }`),
ctor literal `maze.rs:3899`.

## The re-queue construction — `maze.rs:3819-3828` (corrected)

After each soft iteration:

```
next    = failed[i] ∪ { net_idx(c.a), net_idx(c.b) : c ∈ prov[i] }
bad[i+1]= next   (BTreeSet dedupe, in-order)
```

- `failed[i] ⊆ bad[i]` by construction — only queued nets
  attempt.
- `parties(prov[i]) ⊆ bad[i]` is **false**. A `Collision`'s
  parties resolve via `net_idx` to any guided net. The foreign
  party is whichever net's `Net(id)` marks the route hit in
  `occ`, and marks persist for:
  - *kept* nets — committed in an earlier iteration, never
    re-queued, still occupying `occ`, **not members of
    `bad[i]`**; and
  - nets routed earlier in the same iteration (members of
    `bad[i]`).
  A kept net named as a collision party is ripped up and
  re-queued: `bad[i+1]` gains a member from outside `bad[i]`.

Consequence — the r1/r2 claim is corrected:

```
bad[i+1] = failed[i] ∪ parties(prov[i]) ⊆ bad[i]   — WRONG
bad[i+1] = failed[i] ∪ parties(prov[i])            — the code
attempted[i+1] ≤ attempted[i]                      — EMPIRICAL
```

`attempted` non-increase held on both recorded designs (gcd
`269,94,87,81,81,81,81,81`; picorv32
`6744,2557,2437,2382,2371,2342,2338,2326`). An increase requires
a kept net to be provisionally hit — legal under the mechanism,
unobserved on these inputs. `maze_census` therefore *reports*
the property (`iter_progress_monotonic=`, `output.md`) instead
of asserting it.

## Structural invariants (still asserted in `maze_census`)

- `iter_stats.len() == iterations as usize`
- `iter_stats[0].attempted == graphs.nets.len()` (non-empty case)
- `routed ≤ attempted` per entry — each attempt commits ≤ once
- `last.prov_collisions == 0` — hard mode never populates
  `prov`; early convergence exits only when `prov` and `failed`
  are both empty
- `last.attempted - last.routed == unrouted.len()` — hard-cap
  failures are the unrouted census; early-converge leaves both
  sides 0

The picorv32 `#[ignore]`d census gets the same coverage via
`maze_census`; `test_maze_determinism`'s `RouteDb` equality
covers the field via derived `PartialEq`.

## Edge cases (unchanged)

- **Hard iteration** (`it == cap`): `prov` never populated →
  `prov_collisions = 0`; `routed` still counts hard-mode
  commits; failures become `unrouted`.
- **Early convergence**: last entry is a soft iteration with
  `prov_collisions == 0` and zero failures.
- **`OccMode::Disabled`**: `prov_collisions` 0 on every entry.
- **`n == 0` / `bad` empty at entry**: empty `iter_stats`,
  `iterations == 0`.
- **Re-attempted nets**: `sum(routed) ≥ nets.len()` possible;
  `routed` is a per-attempt commit count, not a union count.

## Files

Implementation touches only:

- `crates/grus-core/tests/drt.rs` (assert removal + informational
  print + comment fix — `steps.md` §1)

Reference-only this round (semantics documented, no edits):

- `crates/grus-core/src/drt/maze.rs` — comments audited: the
  `// bad = unrouted ∪ both parties…` note (:3818) states the
  construction verbatim and never claims `bad[i+1] ⊆ bad[i]`;
  the `IterStat` docs (:194-212) make no monotonicity claim.
- `crates/grus-core/src/drt.rs`
