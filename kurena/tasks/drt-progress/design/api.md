# drt-progress — Engine API (`maze.rs`)

## `RouteProgress`

One net-completion snapshot — the router's only live-progress
record. Value type, `Copy`; the sink gets a copy, never a reference
into router state.

```rust
/// One attempted net's completion snapshot — the router's only
/// live-progress channel, emitted once per `bad`-loop attempt to
/// the `route_nets_with_progress` sink. `elapsed` is observational
/// and never feeds a routing decision.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteProgress {
    /// 1-based rip-up iteration (the `IterStat` index).
    pub iteration: u32,
    /// Attempts finished so far this iteration, `1..=queued`, in
    /// `bad` order — routed or failed alike.
    pub done: u32,
    /// Nets queued into this iteration (`IterStat.attempted`).
    pub queued: u32,
    /// Failed attempts so far this iteration — the "unrouted so
    /// far" count. Resets each iteration: re-queued failures count
    /// again in the iteration where they re-fail.
    pub unrouted: u32,
    /// Wall time since `route_nets` entry.
    pub elapsed: std::time::Duration,
}
```

Field semantics vs. the loop's locals (`maze.rs:3819-3872`):

- `iteration` = `it` (already maintained, `maze.rs:3823-3824`).
- `done` = `bi as u32 + 1` (`bi` is the `enumerate` index at
  `maze.rs:3832`).
- `queued` = `attempted` (`maze.rs:3825` — `bad.len()` at entry).
- `unrouted` = `failed.len() as u32` at event time (`failed` is the
  per-iteration `Vec<(usize, UnroutedNet)>` at `maze.rs:3830`).
- `elapsed` = `t0.elapsed()` (`t0` already exists at
  `maze.rs:3783`).

"Unrouted so far" maps to `failed.len()`, not `n − committed`:
during iteration 2+ a large `n − committed` would mostly be
pending retries, not failures — noise for the "is it stuck?"
question. Per-iteration `failed` is the honest failure count and
joins to `IterStat` (`attempted − routed`).

## `route_nets_with_progress`

The current `route_nets` body (`maze.rs:3774-3990`) is renamed
`route_nets_with_progress` with one added parameter:

```rust
pub fn route_nets_with_progress(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    graphs: &RouteGraphs,
    vias: &ViaTable,
    cfg: &MazeConfig,
    progress: Option<&mut dyn FnMut(RouteProgress)>,
) -> RouteDb
```

`route_nets` becomes the thin wrapper — same signature, same docs
moved verbatim plus one line pointing at the sibling:

```rust
pub fn route_nets(
    db: &DesignDb, tech: &TechDb, tracks: &[LayerTracks],
    graphs: &RouteGraphs, vias: &ViaTable, cfg: &MazeConfig,
) -> RouteDb {
    route_nets_with_progress(db, tech, tracks, graphs, vias, cfg, None)
}
```

## Emit site

Inside `for (bi, &i) in bad.iter().enumerate()`, directly after the
`match route_one_net(…)` block and before the `timing` block
(`maze.rs:3859-3871`):

```rust
if let Some(sink) = progress.as_deref_mut() {
    sink(RouteProgress {
        iteration: it,
        done: bi as u32 + 1,
        queued: attempted,
        unrouted: failed.len() as u32,
        elapsed: t0.elapsed(),
    });
}
```

Per-attempt cost under `None`: one `Option` branch. Under a sink:
the branch, a 40-byte value copy, one virtual call, one clock read
— nil against a maze search.

No event fires for iterations that never start (`bad` empty) or
for post-loop phases — the loop is the only emit site. A sink that
wants iteration boundaries sees `done == 1` (first event) and
`done == queued` (last event) and distinguishes them without an
event enum.

## Determinism

- The closure's input is a `Copy` struct; it holds no `&mut` into
  `occ`, `routes`, `marks`, `hist`, `bad`. It cannot perturb the
  route — determinism is a type-level fact, backed by the
  `RouteDb`-equality assert in `tests.md`.
- Event order is `bad` order — the same total order the router
  walks. Events never enter `RouteDb`, `route_digest`, or any
  committed artifact.

## Re-export

`drt.rs:20-23` adds `RouteProgress` to the `pub use maze::{…}`
list so the CLI can name `grus_eda_core::drt::RouteProgress`
alongside `IterStat`.
