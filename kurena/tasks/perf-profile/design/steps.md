# perf-profile — implementation order and gates

Order chosen so every artifact a later step quotes exists before it is
written. Two commits suggested: (1) instrumentation + measure.sh + version
bump, (2) results entry + plan fill + roadmap pointer. One commit is also
acceptable — the ticket is a single logical unit.

## 1. `timing.rs` — mechanism

Per `instrumentation.md`: new `crates/grus-core/src/timing.rs` (`enabled`,
`phase`, `PhaseScope`, `take_report`, `print_report`, TLS accumulator,
`OnceLock` env gate), `lib.rs` export, accumulator unit test.
Gate: `cargo test -p grus-core timing` green; `cargo clippy` clean.

## 2. Scope call sites

Per `phases.md`: `timing::phase` scopes in `main.rs` (sta/power load arms,
congestion, place write), `sta/analysis.rs` (`prepare_sta`, `eval_sta`,
`collect_endpoints`, `run_sta_partitioned`, `run_sta_detail`), `power.rs`
(`estimate_power`), `activity.rs` (optional fine keys only if cheap),
`placement/nesterov.rs`, `placement/legalize.rs`, `congestion/route.rs`.
`main.rs` prints `timing::print_report()` to stderr at each heavy command's
end, gated on `timing::enabled()`.

Rules: `&'static str` names exactly as `phases.md`; no scope inside a
`par_iter` body or per-iteration loop (aggregate keys carry the count);
no `println!` additions — stdout untouched.

Gate: `cargo test --workspace` + `cargo clippy --workspace --all-targets --
-D warnings` green; the gate proof from instrumentation.md §Verification
(ungated stderr byte-compares to pre-change; gated emits `phase ` lines).

## 3. `measure.sh` + runs

Per `measurement.md`: commit `kurena/tasks/perf-profile/measure.sh`, run the
full matrix ≥5 reps on the release build, capture the raw log.
Gate: every table cell filled or its absence named; `/usr/bin/time -l` max
RSS captured.

## 4. `sample` on the top hotspot

Per `measurement.md`: one `sample` trace of whatever the phase table ranks
#1; collapsed stacks kept for the entry.

## 5. `docs/results.md` entry + `docs/roadmap.md` pointer

Per `measurement.md` skeleton: context, wall table, phase breakdown, ranked
hotspots, RSS note, "what was run" (incl. gate-proof transcript tails).
`docs/roadmap.md` gains the short perf pointer naming the candidate ticket
order.

## 6. `plan.md` fill + version

Fill `plan.md`'s share/ceiling table and ticket ranking from the measured
numbers (replace expectations with measurements where they disagree, and say
so). `Cargo.toml` workspace version `0.3.1` → `0.3.2`; `Cargo.lock` follows.

Gate: `cargo test --workspace` + clippy green on the final tree; design
docs committed under `kurena/tasks/perf-profile/design/` reflect what
landed (instrumentation shape recorded per the goal).

## Files

- `crates/grus-core/src/timing.rs`
- `crates/grus-core/src/lib.rs`
- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/power.rs`
- `crates/grus-core/src/activity.rs`
- `crates/grus-core/src/placement/nesterov.rs`
- `crates/grus-core/src/placement/legalize.rs`
- `crates/grus-core/src/congestion/route.rs`
- `crates/grus-eda/src/main.rs`
- `kurena/tasks/perf-profile/measure.sh`
- `kurena/tasks/perf-profile/design/`
- `docs/results.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
