# mc-xdomain — Implementation steps

Ordered so every step keeps the tree green; the version bump and doc
updates land in the same commit as the functional change (`AGENTS.md`
§5). One ticket ≈ one commit — this ticket is at the upper end of the
size budget; split only if review demands it (the sdc.rs parse piece is
the natural seam).

## Step 1 — `sdc.rs`: inter-clock uncertainty model + parse

- `InterClockUnc` struct + `Sdc::inter_clock_uncertainty` field +
  `Sdc::uncertainty_ns` accessor (`api.md` §1).
- `set_clock_uncertainty` arg walk: consume `-from`/`-to` (resolved via
  `resolve_clock`, `Clocks`/`Str` values), keep `-setup`/`-hold`
  exclusivity, `cannot name clocks with -from/-to` conflict bail,
  `unsupported option` for other dash flags (`api.md` §2).
- Tests: move the `multi_clock_bails` case out, add the positive parse
  table and accessor tests (`tests.md` §3–§4).
- Gate: `cargo test -p grus-core sdc` green; all goldens byte-identical
  (no eval reads yet — `inter_clock_uncertainty` is empty on existing
  SDCs anyway).

## Step 2 — `analysis.rs`: pairing helpers

- `grid_lcm`, `setup_edge_pair`, `hold_edge_pair` (`pairing.md` §2) +
  unit tests (`tests.md` §2). Wire call sites in the same step to avoid
  dead code — helpers land with step 3's consumers or gated `#[allow]`
  for one commit only.

## Step 3 — `analysis.rs`: endpoint evaluation

- `xdomain_defer` replaces `foreign_launch` (`api.md` §3): zero-mask →
  `<undeclared>`, latch+foreign → parked, min-side non-coincident →
  parked, LCM miss → defensive.
- `collect_endpoints`: per-(domain, parity) candidate loops on the max
  side (`setup_edge_pair`) and min side (`hold_edge_pair`) for foreign
  bits; same-domain branch verbatim except `uncertainty_ns(d, cap,_)`
  reads; `Worst.arr` carries the `+m·PL` shift (`api.md` §3).
- Output-port and gating-enable candidate loops (`api.md` §4).
- Partitioned skip pass: `xdomain_skip` → `xdomain_defer` (`api.md` §6).
- Compose pass: `d_min = −nh·Pc` when the candidate's `dom_min` mask has
  a foreign bit (`pairing.md` §6); delay-check rewrite switches to the
  worst-over-bits uncertainty accessor (`api.md` §5).
- Gate: all existing tests green; `t2_int`/`t2_frac`/`t2_unc`/… now
  report the cross endpoints at STRICT (`tests.md` §5 table).

## Step 4 — Fixture: `t2_mdx` + regen

- `t2_mdx.sdc` (`fixture.md` §1), `probe.sh` scenario list, container
  regen, `git diff` audit — only the new scenario's JSON block and new
  `probe_t2_mdx.txt` may differ.
- `tests/designs/README.md` entry update.

## Step 5 — Test updates

- `parse_gated` → `["t2_grp"]`, count 16; update
  `sta_evaluates_two_clock_same_domain` (all 8 endpoints, `cross_domain
  == 0`, spot asserts on `u_fy/D` 1.885446/`u_fx/D` 0.885446) —
  consider renaming to `sta_evaluates_two_clock`.
- `cargo test --workspace` + clippy `-D warnings` green.

## Step 6 — Fail-proof + docs + version

- Fail-proof mutation `tests.md` §6: red on `t2_int`/`t2_frac`, restore,
  record both runs in `docs/results.md`.
- `docs/multi-clock.md`: tick §3 item; rewrite §2 remaining-deferral
  list (latch borrow pairing, non-coincident hold parity, folded-virtual
  tag matching, equal-gap tie-break, non-converging LCM); §1.6 addendum
  for `d_min = −nh·Pc` on cross candidates.
- `docs/results.md`: dated entry — regen command, per-scenario match
  table (now including wns/tns/violations), `cross_domain` counts,
  fail-proof runs, what ran vs. not (cross-domain gating/output paths
  unexercised).
- `docs/roadmap.md`: mc-xdomain done, next `mc-selectors`.
- `docs/path-delays.md` §5: note two-clock delay-check coverage now
  runs at STRICT (formal closure stays with `mc-golden`).
- `Cargo.toml` `0.2.1 → 0.2.2` + `Cargo.lock` in the functional commit.

## Files

- `crates/grus-core/src/sta/sdc.rs`
- `crates/grus-core/src/sta/analysis.rs`
- `tests/designs/sta_clocks/t2_mdx.sdc`
- `tests/designs/sta_clocks/probe.sh`
- `tests/designs/sta_clocks/two_clock.probe.json`
- `tests/designs/sta_clocks/probe_t2_mdx.txt`
- `tests/designs/README.md`
- `docs/multi-clock.md`
- `docs/results.md`
- `docs/roadmap.md`
- `docs/path-delays.md`
- `Cargo.toml`
- `Cargo.lock`
