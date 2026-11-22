# mc-selectors — Revision steps

Ordered so every step keeps the tree green. The version bump
(`0.2.4 → 0.2.5`, patch — correctness fixes) and doc updates land in
the functional commit (AGENTS.md §5). Two commits is the natural split:
fixture+probe first, code+tests+docs second.

## Step 0 — Probe + golden re-fold

- Add `t2_selport.sdc` / `t2_selpto.sdc` (`fixture.md`); extend
  `probe.sh` `SCENARIOS` (+10 names), shrink `PROBE_ONLY` to the four
  bail-pinned forms, and add the `run_sta` empty-table `null` guard
  (`fixture.md`, needed by `t2_selac`/`t2_selunion`).
- Run `probe.sh` whole in `ghcr.io/librelane/librelane:3.0.16`; audit
  `git diff` — additive only (29 existing blocks + every
  `probe_t2_*.txt` byte-identical).
- Read `probe_t2_selport.txt`/`probe_t2_selpto.txt` and the new ref
  blocks; record the measured port-name rule in `docs/results.md` and
  `docs/multi-clock.md` §1.7. If the answer is not the expected
  port-object dead rule (`probe.md` §1 table), stop and come back to
  the card.
- Commit fixture + artifacts on their own.

## Step 1 — `analysis.rs`: `clock_row` + `unresolved` + `dead_ends`

- `clock_row` → `c.name` only (`api.md` §1); fix its comment and the
  `key_dom`/`ep_cap` wording.
- `ObjSel::Names` port-namespace arm: bare io-pin names resolve
  (dead-rule objects) in `resolve_from`/`resolve_delay_from`/
  `resolve_to` per the measured rule.
- `resolved_any` counting: `(coverage, resolved_any)` from every
  `resolve_*`; `resolve_sides` and the delay path bump `unresolved` on
  zero resolved objects, not empty coverage (`api.md` §1). Verify
  `test_sta_exception_diagnostics` still sees `== 4`.
- `dead_ends`: extract `dead_end_leaves`, gate per class on
  `ResolvedRule::Delay` in `sigma` (`api.md` §2).
- Stale-comment sweep (`api.md` §3): the four `coincident` hold-pair
  comments → min-gap/`edge_offset` wording.
- `Cargo.toml` `0.2.4 → 0.2.5` + `Cargo.lock` in this commit.
- Gate: `cargo test --workspace` green; all goldens byte-identical.

## Step 2 — Tests + fail-proof + docs

- `test_sta_clocks_match_opensta`: `evaluated` assert `30 → 40`
  (39 `two_clock` scenarios + `t2_latch`).
- `test_edge_pairing_unit_cases`: add the `edge_offset` table and the
  non-uniform `setup_edge_pair`/`hold_edge_pair` rows (`tests.md` §2).
- Fail-proof (AGENTS.md §7 rule 7): restore `c.port` matching in
  `clock_row` → `t2_selport` must go red (under the port-object
  reading, ours would drop the cka launch checks OpenSTA keeps);
  restore and confirm green. Record both runs.
- `docs/multi-clock.md`: §1.7 addendum — bare port names resolve to
  port objects (dead rule), clock names match `-name` only; update the
  §3 item-4 note and any "30 scenarios" counts.
- `docs/timing-exceptions.md`: the clock-selector bullet — bare names
  resolve clock `-name`s only; port names resolve as port objects.
- `docs/results.md`: dated revision entry — the port-name measurement,
  the re-fold list, `dead_ends` gating note, fail-proof runs, what ran
  vs not.
- `docs/roadmap.md` + `tests/designs/README.md`: scenario counts and
  the `PROBE_ONLY` description.
- Gate: `cargo test --workspace` + `cargo clippy --workspace
  --all-targets -- -D warnings` green.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `tests/designs/sta_clocks/`
- `tests/designs/README.md`
- `docs/multi-clock.md`
- `docs/timing-exceptions.md`
- `docs/results.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
