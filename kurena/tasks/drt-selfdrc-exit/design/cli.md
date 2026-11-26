# drt-selfdrc-exit — CLI surface

All changes are in the `Commands::Drt` arm and `DrtArgs` of
`crates/grus-eda/src/main.rs`.

## Flags

```rust
#[arg(
    long,
    overrides_with = "no_self_drc",
    help = "(default) Re-parse the emitted DEF, assemble, and run the \
            50-rule lef_expressible subset checker"
)]
self_drc: bool,

#[arg(
    long,
    overrides_with = "self_drc",
    help = "Skip the emitted-DEF subset check — the census prints \
            `self_drc_verdict SKIPPED` and the exit code stays 0"
)]
no_self_drc: bool,

#[arg(
    long,
    default_value_t = 0,
    conflicts_with = "no_self_drc",
    help = "Findings budget for the named residual class — exit stays \
            0 while findings_total <= N"
)]
self_drc_tolerance: usize,
```

Semantics: `let run_self_drc = !args.no_self_drc;` — both flags
default false, so the bare invocation checks; `overrides_with` makes
the last of `--self-drc`/`--no-self-drc` win, which keeps
`generate_drt_ours.sh`'s existing `--self-drc` spelling valid.
`self_drc` field's only remaining use is the pair's override wiring
(keep it; removing it breaks `--self-drc` callers for no gain).

`--self-drc-tolerance` conflicts with `--no-self-drc` (a budget on a
skipped check is meaningless — fail at parse, not silently).

## Order of operations inside the `Drt` arm

Current layout (`main.rs` ~L2123-2390):

1. `apply_routes` → `report`
2. `preemit` subset (opt-in, on committed `net_geoms`)
3. `write_routed_def` → `args.out_def`
4. print census (`guided=…`, `iter_progress`, `iter_prov`,
   `pin_collision`, `unrouted_*`, `fail`, `*_residual` lines)
5. `--reference` wirelength/via delta lines
6. `preemit_check` block, `self_drc` block (re-parse + `run_all`)
7. `wrote routed DEF to …` → `Ok(())`

New layout:

1. `apply_routes` → `report` (unchanged)
2. `preemit` subset (unchanged)
3. `write_routed_def` (unchanged)
4. **self-DRC compute moves here** — re-parse `args.out_def`,
   `assemble`, `run_all`; keep the `?` propagation on parse errors.
   Hold the `SubsetReport` (or `None` when skipped) for printing.
5. print `== drt emit: <name> ==`, `guided=…` line **byte-identical**
   (check_drt_ours.sh's `guided=(\d+) …` regex stays valid)
6. print `self_drc_verdict …` line (see `output.md`)
7. when run: `print_subset_findings("self_drc", &subset)` + the
   existing `self_drc scope:` line; when `--preemit-check`: the
   `preemit_check` block right after (group both DRC blocks)
8. `iter_progress`/`iter_prov`, `pin_collision`, `unrouted_*`,
   `fail`, `stub_outside_cover`, `*_residual` lines — unchanged order
9. `--reference` wirelength/via delta lines — unchanged
10. `wrote routed DEF to {:?}` — unchanged
11. **gate**: `if findings_total > args.self_drc_tolerance` →
    `anyhow::bail!(…)` after step 10.

## Error text

```
self_drc findings_total={n} exceeds --self-drc-tolerance {t} —
emitted DEF is not DRC-clean (routed= counts connectivity only;
see docs/e6-drt.md §8)
```

Lands on stderr as `Error: …` through `main`'s `Result` — exit 1.
No dedicated exit code: callers check `!= 0`, consistent with every
other loud-failure path in the binary.
