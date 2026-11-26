# drt-progress — CLI wiring (`main.rs`)

## Cadence — recorded choice

Print a line when **any** of:

- `done % 500 == 0` — every 500 completed nets,
- `done == queued` — the last net of each iteration (always:
  iteration boundaries stay visible even when the re-queued `bad`
  set is smaller than 500),
- `last_print.elapsed() >= 60 s` — a heartbeat when nets are slow.

500 nets / 60 s, whichever first, plus the iteration-end line. On
picorv32's recorded profile (`iter_progress` ≈ `6744,2557,…,2326`)
that is ~14 lines for iteration 1 — a line every ~1-2 min at the
measured ~1.2 s/net — plus 7 boundary lines across the cap. On gcd
(269 guided nets, sub-second route) it is one boundary line per
iteration — present but not spammy. 500 is also the stride the
`GRUS_DRT_TIMING` net print already uses (`maze.rs:3861`), a
consistent granularity.

Constants next to `DrtArgs` in `main.rs`:

```rust
/// `drt` stderr progress cadence — every N completed nets or T
/// seconds since the last line, whichever fires first; the last
/// net of each iteration always prints.
const DRT_PROGRESS_NETS: u32 = 500;
const DRT_PROGRESS_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(60);
```

## Sink

In `Commands::Drt` (`main.rs:2085-2096`), `route_nets` →
`route_nets_with_progress` with a closure over one `Instant`:

```rust
let mut last = std::time::Instant::now();
let mut sink = move |p: grus_eda_core::drt::RouteProgress| {
    if p.done % DRT_PROGRESS_NETS == 0
        || p.done == p.queued
        || last.elapsed() >= DRT_PROGRESS_INTERVAL
    {
        eprintln!(
            "drt: iter {} nets {}/{} unrouted={} elapsed={}s",
            p.iteration,
            p.done,
            p.queued,
            p.unrouted,
            p.elapsed.as_secs(),
        );
        last = std::time::Instant::now();
    }
};
let route_db = grus_eda_core::drt::maze::route_nets_with_progress(
    &design_db, tech_db, &tracks, &graphs, &vias,
    &grus_eda_core::drt::MazeConfig { occ, max_iters: args.max_iters },
    Some(&mut sink),
);
```

(`elapsed.as_secs()` — integer seconds; a 2 h run reads
`elapsed=7241s`, unambiguous and format-trivial. No `humantime`
dependency for one field.)

Sample (picorv32 iteration 1, shape only — real numbers recorded
in `docs/results.md` from the gcd leg):

```
drt: iter 1 nets 500/6744 unrouted=0 elapsed=605s
drt: iter 1 nets 6744/6744 unrouted=4187 elapsed=8012s
drt: iter 2 nets 500/2557 unrouted=12 elapsed=8621s
```

## stderr-only contract

- The only new output is `eprintln!` inside the closure — a
  `main.rs` locals-only sink. The library gains no `eprintln!`
  beyond the pre-existing `GRUS_DRT_TIMING` gates.
- `generate_drt_ours.sh:67` tees **stdout** into the committed
  `.drt.ours.log`; stderr lands on the terminal only, so
  `ours_router.cli_lines` (`check_drt_ours.sh:929-957`) and every
  `== drt emit:` consumer see byte-identical output.
- Progress lines carry wall time — they must never be asserted
  byte-equal or committed; the `docs/results.md` sample is marked
  observational.
- Not gated behind a flag: stderr writes are the spec'd behavior
  and cost nothing to consumers who ignore the stream.
