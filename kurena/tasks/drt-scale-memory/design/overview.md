# drt-scale-memory — overview (revision 2)

## Status

Round 1 landed (`13bcf52 kurena: implement drt-scale-memory`) and the
implementation is **approved-in-principle**: the measured dominant
reduction (a generalization of `options.md` O2 — every occupancy-free
per-net structure is rebuilt per attempt) cut peak RSS 153.1 → 82.6 MB
on gcd and 2.37 GB → 958 MB on picorv32, routed DEF byte-identical on
both designs. The human rejection is **documentation-conformance
only**: the `docs/results.md` entry does not meet this design's own
record spec, and three small instrumentation repairs are needed.

This revision covers only the remaining work. `measurement.md` and
`options.md` stay unchanged — they are the round-1 record the results
entry cites.

## Rejected-feedback map

| # | review item | fix | spec |
|---|---|---|---|
| 1 | machine-context block + rep policy missing from the entry | add both, verbatim format from the `drt-rayon` entry | `docs.md` §1 |
| 2 | split table missing designed rows (marks, routes+residuals, outcomes hi-water, rest, separate access/cover) | complete the table from the retained run logs — no re-run needed | `docs.md` §2 |
| 3 | gcd baseline inconsistency: census table 152.8 MB vs before/after table 153.1 MB | reconcile by build label: 153,092,096 B is the HEAD baseline median of 3 reps; 152,829,952 B is the commit-1 instrumented binary's peak | `docs.md` §3 |
| 4 | gcd `route_ms` delta not recorded | record baseline median 5705 ms vs the after median (new reps) | `docs.md` §4 |
| 5 | version typo "0.14.4 → 0.15.0" — base was 0.14.3 | fix in place | `docs.md` §5 |
| 6 | `mem.rs` docstring cites `members`/`cover` for `BTREE_NODE_EST`; `members` was flattened into `NetAdj`'s CSR | doc now names `cover` only | `code-fixes.md` §1 |
| 7 | `drt: mem iter` reports `attempt_hi=0` on the hard (serial) iteration — the serial arm is never measured | measure it (small gated addition) + record the one-pair bound argument for the already-run picorv32 leg | `code-fixes.md` §2, `docs.md` §6 |
| 8 | `libc` not workspace-managed like the other deps | move to `[workspace.dependencies]` | `code-fixes.md` §3 |

## Remaining work

1. `fix:` census conformance repairs — serial-arm `attempt_hi`,
   `mem.rs` docstring, `libc` workspace-managed; workspace bump
   0.15.0 → 0.15.1 (`code-fixes.md`, `steps.md`).
2. Cheap gcd re-runs only: two more ungated after-reps (rep policy
   needs ≥3; `/tmp/gcd.mem.new.log` holds one) plus one
   `GRUS_DRT_MEM=1` run to record the serial `attempt_hi`. **No
   picorv32 re-run** — all three legs' stderr logs are retained
   (`/tmp/picorv32.mem.{base,inst,new}.log`), the code change is a
   gated read-only size walk that cannot affect routing, and the
   serial arm's transient is bounded by the recorded `attempt_hi`
   (`docs.md` §6 states the argument).
3. `docs:` rewrite the 2026-11-27 `results.md` entry per `docs.md`;
   touch `docs/e6-drt.md` (serial-arm census clause) and
   `docs/status.md` (version token).

If any retained log has been rotated away, re-run that leg instead of
synthesizing numbers (E0 rules 1–2) — gcd legs are seconds; picorv32
legs are detached ~1–2 h runs and their absence must be named.

## Files

- `crates/grus-eda-core/src/drt/maze.rs` — serial-arm `attempt_hi`
  census + iter-line comment
- `crates/grus-eda-core/src/drt/mem.rs` — `BTREE_NODE_EST` docstring
- `Cargo.toml` — `[workspace.dependencies] libc`, version bump
- `crates/grus-eda-core/Cargo.toml` — `libc = { workspace = true }`
- `docs/results.md` — corrected dated entry
- `docs/e6-drt.md` — serial-arm census clause in the Resident
  footprint block
- `docs/status.md` — version token only (figures unchanged)
