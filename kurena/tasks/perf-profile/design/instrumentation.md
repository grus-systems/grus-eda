# perf-profile — instrumentation mechanism

Phase timing at the natural boundaries of the heavy commands, off by default,
stderr-only when on. This file is the mechanism; [`phases.md`](phases.md) lists
where the scopes go.

## Shape: env-gated scope guards + thread-local accumulator

New module `crates/grus-core/src/timing.rs` (~70 lines), exported from
`lib.rs`:

```rust
pub fn enabled() -> bool;                        // OnceLock<bool>, reads env once
pub fn phase(name: &'static str) -> Option<PhaseScope>;
pub struct PhaseScope { name: &'static str, start: Instant }   // Drop records elapsed
pub fn take_report() -> Vec<(&'static str, f64, u32)>;         // drains TLS map
pub fn print_report();                                         // stderr, per phase
```

- **Gate**: `GRUS_PHASE_TIMING` — enabled when set and not `0`/`""`. Read once
  through a `OnceLock` inside grus-core, not through `main()`: `run_sta`,
  `estimate_power` and `legalize_placement` keep their signatures (every test
  and bench call site is unchanged), and the gate works identically under
  `cargo test`, benches, and the CLI.
- **Call sites**: `let _p = timing::phase("sta.eval.walk");` Disabled cost is
  one relaxed atomic load per phase boundary — nothing in the per-pin,
  per-arc, or per-iteration hot loops (phases.md only scopes whole passes).
- **Accumulator**: `thread_local!` `Vec<(&'static str, u64 ns, u32 count)>`
  with linear lookup (~40 names — a `HashMap` is overkill). Scope guards never
  cross a `rayon` boundary (a `par_iter` body is never a scope; a scope wraps
  the whole parallel section), so all drops land on the caller thread —
  thread-local is correct and needs no lock. The count field makes repeated
  phases (per-class `eval`, per-round legalization passes) report
  `total + count` instead of overwriting.
- **Emission**: `print_report()` drains the calling thread's map and writes
  one stderr line per phase, in first-touch (program) order:

  ```
  phase sta.load.liberty            43.21 ms   x1
  phase sta.parasitics             149.02 ms   x1
  phase sta.eval.walk              891.44 ms   x1
  ```

  `main.rs` calls it at the end of each heavy command's arm, only when
  `timing::enabled()` — stdout is untouched, so every `elapsed=` line and the
  skip summary stay byte-identical.

## Why this shape (alternatives considered and rejected)

- **Per-scope `eprintln!` on drop** — simplest, but emits in completion order
  (fine for the `sta` pipeline, misleading for `place`'s 4 legalization
  rounds and partitioned eval classes) and cannot answer "how much of the run
  was Pass 3". The accumulator reports totals, which is the profile question.
- **CLI flag (`--timing`)** — would need to reach `run_sta_detail` /
  `estimate_power` / `legalize_placement`, all of which have public
  signatures shared with dozens of test call sites; either signatures change
  or a global is set anyway. Env var gives the same one-switch UX with zero
  plumbing and also gates instrumented benches/tests for free.
- **`tracing` / `tracing-chrome`** — real infrastructure for a two-run task;
  a new dependency for what is ~70 lines. Rejected per the ticket's "small"
  constraint; if phase timing proves permanent-useful, a follow-on ticket can
  swap the backend without touching call sites (the `phase()` name set is the
  API).

## Nested attribution

`sta_pin_slews` runs a full `run_sta_detail` inside `power`, so a `power` run
fills both `sta.*` and `power.*` keys. That is the honest attribution — the
results table reads `power = power.* + sta.*` — and is stated in
`measurement.md` so no one double-counts.

Likewise `run_sta_partitioned` calls `eval_sta` per signature class: the
`sta.eval.*` keys accumulate across classes (count = number of eval runs), and
`sta.partition.*` covers the per-endpoint composition that is not in any
class's eval.

## Overhead bound

Scopes per `sta` run: ~15–25 `Instant::now()` pairs at pass granularity —
sub-microsecond total against a ≥50 ms command. Nothing is allocated or
formatted when disabled. The disabled path is therefore not measurable; the
results entry does not claim a zero cost, it claims "below measurement noise
and never on a hot loop".

## Verification

- **Off-by-default**: unit test in `timing.rs` asserting `take_report()`
  drains and that the accumulator records `(ns, count)` correctly —
  environment-independent (tests the mechanism, not the env var, which
  `cargo test` parallelism makes flaky to mutate).
- **Gate proof**: run `grus-eda sta … 2>phases.txt` with and without
  `GRUS_PHASE_TIMING=1`; without it stderr must be the pre-change byte stream
  (the existing `lef:`/`warning:`/`power:` lines only); with it the `phase `
  lines appear. Record both transcripts' tails in the results entry.
- **Golden identity**: `cargo test --workspace` plus `cargo test --release -p
  grus-core sta` (the OpenSTA golden compare) must pass untouched — the
  instrumentation changes no number. CI runs `cargo test --workspace` and
  clippy `-D warnings` (`workflow.toml`).
- **Intentional-failure check** (`AGENTS.md` §7 rule 7): temporarily make
  `phase()` return a scope unconditionally → an ungated `sta` run writes
  `phase ` lines to stderr (proves the lines are real); restore.

## Files

- `crates/grus-core/src/timing.rs`
- `crates/grus-core/src/lib.rs`
- `crates/grus-eda/src/main.rs`
