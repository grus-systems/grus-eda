# mc-cleanups — Implementation steps

Order matters only at two seams: the probe run must precede the
measured-behaviour fixes (A2, A5, A7, A8, A9), and the `docs/results.md`
entry is written last. Everything else is independent. Keep the diff in
one commit (the ticket is cleanup-scale); if the diff grows past ~150
lines of non-doc change, split along the probe/no-probe seam.

## Step 1 — Mechanical code nits (no probe dependency)

`analysis.rs`:

- A1 `grid_lcm` guard restatement (~L5083–5101).
- A3 `delay_max_pins`/`delay_min_pins` last-wins comment (~L4019) and
  `ResolvedPathDelay::from_pins` doc (~L3562).
- A6.2 hold-pairing comment fix (~L4597–4601) + `coincident` grep audit.
- A6.3 `SkippedEndpoint` doc update (~L353).
- A6.1 `same_domain`/`sta_evaluates_two_clock_same_domain` audit —
  record result.
- A4 PROBE_ONLY per-name expected-substring asserts (~L9399).

`saif.rs`:

- A10.1 DURATION/TIMESCALE integer-required error wording.
- A10.2 negative-vs-fractional message split (~L229).
- A10.3 `Vec<u8>` atom accumulation + `String::from_utf8`.
- Unit tests: DURATION `1990.0`/`abc`/`-5`, negative `T0`, UTF-8 name.

Gate: `cargo test -p grus-core`, `cargo clippy --workspace --all-targets
-- -D warnings`. Fail-proof: point a PROBE_ONLY assert at the wrong
substring and confirm red; revert.

## Step 2 — Probe scenarios + container run

- Write the seven `t2_*.sdc` scenario files per `probes.md` (copy
  `t2_int.sdc` verbatim, then edit the listed lines; verify names like
  `u_fa0/Q` against `two_clock.def`).
- Add `MEASURE_ONLY` + its loop to `probe.sh`.
- Run `bash tests/designs/sta_clocks/probe.sh` (Colima/docker per
  `AGENTS.md` §2). This regenerates all `probe_*.txt` — confirm with
  `git status`/`git diff` that pre-existing files are byte-identical
  and only the new `t2_*` artifacts appear. If a regenerated
  `probe_*.txt` drifts, stop and diagnose before proceeding (the
  scenario set did not change; drift means environment or a real
  change).

Gate: `two_clock.reference.json`, `two_clock_latch.reference.json`,
all pre-existing `probe_*.txt` byte-identical; `probe_t2_{eclk,nsfrom,
nsto,toinv,noclk,waverot,wavez}.txt` committed.

## Step 3 — Measured-behaviour fixes

Implement per `api.md` decision trees, using the recorded probe
evidence:

- A2 empty clock list — `plain_names`/`get_clocks` empty-collection
  handling + `set_clock_*` empty-selection rule.
- A5 `-waveform` post-wrap validation (or documented keep).
- A7 `-to` resolved-but-invalid counting in `resolve_to` +
  `unresolved_exception_selectors` field doc.
- A8 port-vs-clock priority in `resolve_from`/`resolve_to`/
  `resolve_delay_from` `ObjSel::Names` arms.
- A9 `-clock`-less anchor under `-waveform` (possibly a no-change
  record).

Each fix that changes a counted diagnostic updates the corresponding
field doc and `docs/timing-exceptions.md` in the same edit. Any fix
visible to an endpoint table gets a unit test pinning it (e.g. a new
`t2_*`-style `load_sdc`+`run_sta` assert or a parser test — match the
existing test shapes in `analysis.rs`/`sdc.rs`).

Gate: `cargo test --workspace`, clippy. Fail-proof each behaviour
change: break the new code path (e.g. revert the post-wrap check or the
priority order), confirm the new/changed test goes red, restore.

## Step 4 — Documentation pass

Per `docs.md`:

- D1 `timing-exceptions.md` counters.
- D2 `results.md`: "15→14+`t2_grp`" fix, literal `awk` command (run it),
  saif-power entry audit.
- D3 `tests/designs/README.md` fixture entries (`microbench`,
  `sta_exceptions`, `repro_extreme_ceff` — verify filenames first).
- D4 `driven_nets_without_activity` doc.
- D5 `t2_mdx` description audit (likely no-op).
- D6 `multi-clock.md` sync: scenario count, §1.2 hold note pointer,
  §1.7 new measured rules.

## Step 5 — Results entry + version bump

- D7 `docs/results.md` dated entry: probe lines verbatim (with STA
  codes), implemented rules, no-op items, verification record.
- Bump `[workspace.package] version` `0.2.6 → 0.2.7` in `Cargo.toml`;
  `cargo check` (or any cargo command) refreshes `Cargo.lock`.

## Final gate

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --stat tests/designs/    # only new probe artifacts differ
```

Optional heavy: `cargo test --release -p grus-core sta`.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/sdc.rs`
- `crates/grus-core/src/readers/saif.rs`
- `crates/grus-core/src/power.rs`
- `tests/designs/sta_clocks/probe.sh`
- `tests/designs/sta_clocks/t2_*.sdc` (new)
- `tests/designs/sta_clocks/probe_t2_*.txt` (new outputs)
- `docs/timing-exceptions.md`
- `docs/results.md`
- `docs/multi-clock.md`
- `tests/designs/README.md`
- `Cargo.toml`
- `Cargo.lock`
