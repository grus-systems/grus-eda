# drt-progress — Overview

E6 observability ticket. `route_nets` (`maze.rs:3774`) emits nothing
until it returns a `RouteDb` — measured ~135 min of silence on
picorv32 (`tests/drt.rs:2675`). The spec asks for a periodic stderr
progress line during `grus-eda drt` routing: nets done/total,
unrouted so far, elapsed wall time.

## Mechanism decision — opt-in callback, not an engine `eprintln`

`route_nets` gains a sibling entry point:

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

`route_nets` keeps its signature and delegates with `None`; the
~20 existing call sites (`tests/drt.rs`, `maze.rs` test module,
`main.rs:2086` switched to the `_with_progress` form) are untouched.
Rejected alternatives:

- **`MazeConfig` flag/field** — `MazeConfig` is `Copy + Debug`
  (`maze.rs:43`); a `dyn FnMut` cannot live in it. A bool flag would
  hard-code an `eprintln!` inside the engine, violating "no stderr
  writes from library callers".
- **Signature change on `route_nets`** — churns every test call
  site for a CLI-only concern; the spec asks for a minimal surface
  change.
- **Throttle inside the engine** — cadence is a presentation
  decision; the engine emits one event per attempted net and the
  sink decides when to print.

`FnMut` (not `Fn`) because the CLI sink carries last-print state.
`Option<&mut dyn …>` (not a generic `impl FnMut`) so `route_nets`
delegates with `None` without monomorphizing a second body — the
hot loop's only cost is one `Option` check per net.

## Event

One `RouteProgress` snapshot per attempted net, fired inside the
`for (bi, &i) in bad.iter().enumerate()` loop (`maze.rs:3832`)
after the `route_one_net` match — see `api.md`. Fields cover
exactly the spec'd line: iteration, done/queued, unrouted so far,
elapsed. The closure receives a value copy and no router-state
references — it cannot perturb routing, so determinism is
structural, not just tested.

## Boundary vs `GRUS_DRT_TIMING`

`GRUS_DRT_TIMING` (`maze.rs:3782`) is a developer knob for
*diagnostic* timings (premark/topo/per-slow-net), env-gated, not a
contract. The progress hook is the supported, always-on-at-CLI
channel. They coexist; neither replaces the other. `docs/e6-drt.md`
§7's Instrumentation paragraph gains one clause naming the hook.

## stdout contract — byte-identical

`generate_drt_ours.sh:67` tees **stdout only** into
`{design}.drt.ours.log`; `check_drt_ours.sh:929-957` filters that
log into `ours_router.cli_lines` — the committed golden. Progress
goes to stderr (`eprintln!` in `main.rs` only), so the log, the
golden, and every `== drt emit:` consumer see identical bytes.
Verified on a real gcd run, recorded in `docs/results.md`
(`tests.md`).

## Non-goals

- No routing-behavior change: `bad` computation, rip-up, history,
  hard mode, occupancy — all untouched.
- No progress events for non-loop phases (premark, topo build,
  min-area patch, censuses). Spec scope is "during routing";
  `GRUS_DRT_TIMING` already covers phase timings.
- No CLI flag (`--quiet`, `--progress-interval`) — stderr is free
  to write unconditionally; nobody's pipeline parses it.
- No carriage-return/in-place redraw line — plain appended lines
  survive `tee`/log capture.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-core/tests/drt.rs`
- `crates/grus-eda/src/main.rs`
- `docs/results.md`
- `docs/e6-drt.md`
- `Cargo.toml`
