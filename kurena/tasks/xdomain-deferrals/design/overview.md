# xdomain-deferrals — Overview

## Goal

Resolve the four `CrossDomain` deferrals parked in `docs/multi-clock.md` §2.
Each is a defensive named skip: for every corner, measure what OpenSTA
3.0.16 does (one container probe per item on `tests/designs/sta_clocks/`),
then either implement the measured behaviour and golden-cover it at STRICT,
or keep the deferral — pinned by the committed probe artifact and a test.

## The four items and where they live

All four funnel through `xdomain_defer` (`crates/grus-core/src/sta/analysis.rs`
~5436), which is called per endpoint before evaluation:

- checks (`collect_endpoints` ~5671, partitioned skip pass ~4904),
- gating enables (~5955, skip pass ~4951; `hold_next = true`),
- output ports (per `-clock` anchor ~6150/4995).

| # | item | trigger in `xdomain_defer` | a priori belief |
|---|------|---------------------------|-----------------|
| 1 | **Undeclared-launch zero mask** | a reached extreme carries `dom_* == 0` → `launch <undeclared>` | believed unreachable: every finite-arrival seed contributes a nonzero `dom` bit (port seeds get `dom_bit(launch_domain(..))` with row < 8 via the >8-clocks bail; `LaunchSeed`s with `dom == u8::MAX` have all-`UNREACHED` pairs — analysis.rs:2810-2857, 2940-2960, 2542-2552). Defensive like `grid_lcm`. |
| 2 | **Latch-D foreign-launch borrow pairing** | `latch && foreign != 0` → unconditional defer | genuinely unknown. The same-domain borrow model is measured (`sta_edge/latch_borrow`); the cross-domain pair/borrow interplay is not — `two_clock_latch.def` has every endpoint same-domain by construction. |
| 3 | **Unpairable hold tying parity** | min-side `(d, g)` where `hold_edge_pair(...)` returns `None` — every launch edge `tl < lcm` lacks a capture edge at-or-before it | **already divergent on a committed golden**: `t2_waverot` `u_fy/D` defers today (`launch ckb -> capture cka`) while OpenSTA times it (`min_arr 2.287085`, `min_req 1.862415`; `probe_t2_waverot.txt` shows launch `2.00 clock ckb` — the LCM-boundary edge — vs capture `1.90 clock cka`). Verified 2026-11-21: it is the *only* deferred endpoint across all committed scenarios. |
| 4 | **`grid_lcm` non-convergence** | `grid_lcm` returns `None` inside `setup_edge_pair`/`hold_edge_pair` | defensive bound (f64 Euclid, 32 iterations, tol `1e-6·min`); docs call it unreachable for f32 periods. OpenSTA's horizon on near-incommensurate periods is unmeasured. |

## What the probe must answer per item

| item | OpenSTA corner | outcome branches |
|---|---|---|
| 1 | A checked endpoint whose only launch has no declared clock — unclocked register launch (`t2_u1`), and the same endpoint covered by `set_max_delay`/`set_min_delay -to` (`t2_u2`) | report / drop / error. If it drops (expected), the deferral stays — defensive and unreachable, evidence committed. If a `-to` delay check reports a number, implement that shape. |
| 2 | `u_ln/D` (`dlxtn` on ckb) with a cka-anchored launch (`t2_xlat`, `t2_xlat2`) | OpenSTA borrows → implement the measured edge rule; OpenSTA checks without borrow (hard-edge) → implement pairing only; OpenSTA drops/errors → keep deferral, pinned. |
| 3 | Already measured: `probe_t2_waverot.txt` | implement boundary-inclusive hold pairing; `u_fy/D` leaves the deferral list, `t2_waverot` aggregates compare again. No new SDC needed (acceptance's artifact is already committed); a second discriminator scenario only if implement raises a form question. |
| 4 | Near-incommensurate periods (`t2_lcm` 2.0/1.7320508, `t2_lcm2` 2.0/1.999999) | OpenSTA evaluates within a bounded horizon → bound `launch_window`/`grid_lcm` to match + golden; OpenSTA warns/errors/hangs → keep deferral, pinned, and still bound the window loop (a converged tiny gcd currently yields an LCM of ~1e7 ns → ~1e7 loop iterations per candidate — a hang hazard worth capping regardless). |

## Decision rule (from the spec)

- OpenSTA produces a value → implement it, add the scenario to `SCENARIOS`
  in `probe.sh` so `ref_*.json` folds into the golden, STRICT-compare in
  `test_sta_clocks_match_opensta`.
- OpenSTA also skips/errors → keep the named deferral; the scenario stays
  out of `SCENARIOS` (a deferring scenario cannot join the golden: the
  test flags `deferred but OpenSTA never timed it`). Pin it with the
  committed `probe_*.txt` + a unit/golden-side test asserting the deferral
  or the equivalent skip outcome.

## Note on "goldens byte-identical"

The acceptance's byte-identical clause means the AGENTS.md §7.1 discipline:
rerunning committed `probe.sh` must reproduce every committed artifact
byte-for-byte. A scenario that moves into `SCENARIOS` because its item was
implemented adds a new key to `two_clock.reference.json` /
`two_clock_latch.reference.json` — that diff is expected; every *existing*
scenario blob must stay byte-stable on regen.

## Files

- `tests/designs/sta_clocks/` — new `t2_*` SDCs, `probe.sh` additions,
  committed `probe_t2_*.txt` artifacts; `two_clock*.reference.json` only
  where an item is implemented
- `crates/grus-core/src/sta/analysis.rs` — `hold_edge_pair` for item 3
  (always); per-item branches for items 1/2/4 only if measured evaluated;
  tests either way
- `docs/multi-clock.md` — §1 measured-rule additions + §2 deferral list
  resolution
- `docs/results.md` — dated probe + comparison entry
- `tests/designs/README.md` — `sta_clocks` section update
- `docs/roadmap.md` — the §-level deferral pointer lines (~339-341)
- `Cargo.toml` — `0.2.11` → `0.2.12`; `Cargo.lock` via `cargo check`
