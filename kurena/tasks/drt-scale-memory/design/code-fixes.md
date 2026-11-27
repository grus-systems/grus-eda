# code-fixes — the three instrumentation repairs (revision 2)

Review items 6–8. All small; the only behavioural change is gated
behind `GRUS_DRT_MEM`. One `fix:` commit (see `steps.md`).

## §1 `mem.rs` `BTREE_NODE_EST` docstring (item 6)

`crates/grus-eda-core/src/drt/mem.rs` L11–15 still says
"`members`/`cover` rows report `len × BTREE_NODE_EST`" — `members`
was flattened into `NetAdj`'s CSR by the round-1 change and no longer
reports a BTreeMap row. Only `NetTopo.cover` still uses the estimate
(`maze.rs` `TopoMem::cover_entries`). Rewrite the doc comment to name
`cover` alone:

```rust
/// `BTreeMap` exposes no capacity API, so `NetTopo.cover`'s per-entry
/// node overhead reports as `entries × BTREE_NODE_EST` beside the
/// exact inner-vec bytes — labeled `~` in the emitted line and never
/// folded into the exact totals.
```

Sweep for other stale `members`-as-BTreeMap doc mentions in `drt/`
(`maze.rs` L2195 already names `cover` correctly).

## §2 Serial-arm `attempt_hi` (item 7 — the "measure it" option)

`route_prepared`'s hard arm (`maze.rs` ~L4481) rebuilds
`(NetGraph, NetAdj)` per net but never censuses the pair, so the
`iter` line prints `attempt_hi=0` for the final iteration — an
unmeasured row, not a real zero. Both size fns are already in scope
(`net_graph_bytes` imported at L17, `net_adj_bytes` at L2176).

Add inside the `for (bi, &i)` loop, after `NetAdj::new`:

```rust
if mem_on {
    attempt_hi = attempt_hi.max(net_graph_bytes(&g) + net_adj_bytes(&adj));
}
```

And extend the `outcomes` comment at ~L4464 so the emitted line reads
truthfully:

```rust
// `drt: mem` "outcomes" row — the transient all-nets buffer
// exists only in the snapshot arm; the hard arm commits inline
// (outcomes stays 0). `attempt_hi` is measured in both arms.
```

`outcomes_b` stays 0 on the hard arm by construction — correct, and
now stated. No test asserts the stderr format; the change is a
`mem_on`-gated size read and cannot touch routing — the fresh gcd
census run confirms the DEF sha and the new `iter=8` figure in one
pass (`steps.md`).

## §3 `libc` → workspace dependency (item 8)

- `Cargo.toml` `[workspace.dependencies]`: add `libc = "0.2"` in the
  existing alphabetical position (between `fast-float2` and
  `memmap2`).
- `crates/grus-eda-core/Cargo.toml`: `libc = "0.2"` →
  `libc = { workspace = true }`.
- `Cargo.lock` unchanged (same resolved version — verify with
  `cargo metadata` or a lock diff of no-op).

## Version

The §2 change alters emitted diagnostics — a functional change per
AGENTS §5 → `[workspace.package]` 0.15.0 → **0.15.1** in the same
commit.
