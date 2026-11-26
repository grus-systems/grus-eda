# drt-progress — Steps

One commit, ~120 lines of source + docs. No routing-behavior
change, no stdout change, no golden regeneration.

## 1. `maze.rs` — `RouteProgress` + `route_nets_with_progress`

a. Add `RouteProgress` (struct per `api.md`) near `IterStat`
   (~`maze.rs:216`), before `RouteDb`.
b. Rename `route_nets` (`maze.rs:3774`) →
   `route_nets_with_progress`, add the
   `progress: Option<&mut dyn FnMut(RouteProgress)>` parameter;
   keep the doc comment, extend it with one line naming the sink.
c. Add the emit hunk inside the `bad` loop after the
   `route_one_net` match, before the `timing` block
   (`api.md` §Emit site).
d. Re-add `route_nets` as the three-line delegating wrapper
   directly above `route_nets_with_progress`, carrying the
   original doc comment plus "…; use `route_nets_with_progress`
   to observe per-net progress."

## 2. `drt.rs` — re-export

Add `RouteProgress` to the `pub use maze::{…}` list (:20-23),
alphabetically after `OccMode`.

## 3. `main.rs` — wire the sink

a. `DRT_PROGRESS_NETS` / `DRT_PROGRESS_INTERVAL` consts next to
   `DrtArgs` (`cli.md`).
b. `Commands::Drt`: swap `route_nets` →
   `route_nets_with_progress` (:2086-2096), passing the closure
   per `cli.md`. Nothing else in the arm changes — every
   `println!` is untouched.

## 4. `tests/drt.rs` — `test_route_nets_progress_hook`

New test next to `test_maze_cases` per `tests.md`: hooked run on
the `maze_db` 3-net fixture, event-stream asserts, and the
`RouteDb` equality assert against the unhooked `route_nets` run.
Import `route_nets_with_progress` and `RouteProgress` in the
existing `use` (:1379).

## 5. Docs + version

a. `Cargo.toml` `[workspace.package] version`:
   `0.13.0 → 0.13.1` — additive feature, patch on 0.x.
b. `docs/e6-drt.md` §7 Instrumentation paragraph (:712-714):
   one clause — `route_nets_with_progress` emits a `RouteProgress`
   per attempted net to an opt-in sink; `grus-eda drt` prints
   `drt: iter … nets … unrouted … elapsed …` to stderr every 500
   nets / 60 s / iteration end.
c. `docs/results.md` — dated entry `2026-11-26 — drt-progress`:
   command run (the `generate_drt_ours.sh gcd` invocation with
   stderr captured), stderr line count, one verbatim sample line
   marked observational, `cli_lines`-unchanged outcome, checks
   run.

## 6. Verify

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p grus-eda
tests/designs/generate_drt_ours.sh gcd   # stderr → progress.log
tests/designs/check_drt_ours.sh gcd      # cli_lines golden holds
```

Fail-proof (E0 lesson 7): before committing, break the emit hunk
(`unrouted: 999`) and confirm `test_route_nets_progress_hook`
goes red; restore.

## 7. Commit

```
feat: Add route_nets progress hook and drt stderr reporting

- maze.rs: RouteProgress snapshot per attempted net; new
  route_nets_with_progress entry point, route_nets delegates
  with no sink — library callers get no output
- main.rs: drt prints "drt: iter N nets k/q unrouted=u elapsed=Ts"
  to stderr every 500 nets or 60 s, plus each iteration's last
  net; stdout census block unchanged (generate_drt_ours.sh tees
  stdout only)
- tests/drt.rs: test_route_nets_progress_hook — event coverage,
  ordering, unrouted semantics, RouteDb equality hooked vs not
- docs/e6-drt.md §7: name the hook alongside GRUS_DRT_TIMING;
  docs/results.md dated entry with a sample line
- version 0.13.0 -> 0.13.1 (additive API)

<agent trailer>
```

## Contingencies

- **Closure borrow conflicts at the call site** — the sink
  captures only `last: Instant`; if `graphs`/`design_db` borrows
  collide, bind the closure before `t_route` and scope it
  explicitly. Trivial fix, not a design change.
- **stderr interleaving on `--self-drc` runs** — self-drc prints
  on stdout after routing; streams never mix into each other's
  files. No ordering guarantee needed.
- **`route_nets` becomes dead pub API in-tree** — it's library
  surface for external callers and tests; keep, do not mark
  deprecated.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-core/tests/drt.rs`
- `crates/grus-eda/src/main.rs`
- `docs/results.md`
- `docs/e6-drt.md`
- `Cargo.toml`
