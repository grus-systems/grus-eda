# mc-cleanups — Implementation steps (revision 2)

The A-items landed in `e6bd1a7`; this sequence covers the review-memo
follow-up (R1–R5). Order matters at one seam: the probe run precedes
the measured-rule implementation (R1). Keep the diff in one commit; if
the R1 data-model change grows past ~300 lines, split at the
probe/code seam.

## Step 1 — Probe harness + scenario files

- `probe.sh`: harden `collect_endpoints_json` per probes.md P0 (union
  `pins` from both passes, `null` for unset corner fields). Update
  `SCENARIOS` (+`t2_eclk2 t2_clkcor t2_clkcor2 t2_waverot t2_wavez`)
  and shrink `MEASURE_ONLY` to `t2_eclk t2_nsfrom t2_nsto t2_toinv
  t2_noclk`; fix the comment block.
- `t2_eclk.sdc`: append the lines per probes.md P2.
- New `t2_eclk2.sdc`, `t2_clkcor.sdc`, `t2_clkcor2.sdc` per probes.md —
  copy `t2_int.sdc` verbatim then edit; verify names against
  `two_clock.def`.

## Step 2 — Container run

`bash tests/designs/sta_clocks/probe.sh` (Colima/docker per
`AGENTS.md` §2). Then `git status` / `git diff tests/designs/`:

- Pre-existing `probe_t2_*.txt` and the existing golden entries:
  byte-identical. If anything drifts, stop and diagnose.
- `two_clock.reference.json` gains five blocks (`t2_eclk2`,
  `t2_clkcor`, `t2_clkcor2`, `t2_waverot`, `t2_wavez`) — `t2_clkcor*`
  may carry `null` corner fields.
- `probe_t2_eclk.txt` regenerates from the extended file.
- If a `t2_eclk` line aborts `read_sdc` mid-file, split tail forms
  into `t2_eclk3.sdc` and re-run.
- Read the discriminators (probes.md P1–P3 tables) and record the
  measured rules.

Gate: artifacts committed; `cargo test --workspace` is expected **red**
at this point — `test_sta_clocks_match_opensta` compares the new
blocks against the not-yet-fixed implementation. If it is green with
the old code, the probes disagree with the assumed semantics —
re-examine before touching code.

## Step 3 — Measured-rule implementation (R1)

Per `api.md` R1, driven by the measured outcome:

- `delay_at` per-corner fold + accessor changes (`sdc.rs`).
- `PortSeed` per-corner `dom`/`per_pol`, `eval_sta` per-corner masks,
  `key_dom`/`ep_cap` union matching (`analysis.rs`).
- `collect_endpoints` per-corner output-port handling.
- `EndpointReport` → `Option<f32>` corner fields; `record_endpoint`
  emit-on-either-corner; `merge_corner_reports`, `parasitics.rs`
  asserts, `test_sta_clocks_match_opensta` `null` handling,
  `evaluated == 45`.
- `sdc.rs` unit tests for the fold (both orderings, `-add_delay`
  winner case).

Gate: `cargo test --workspace`, clippy. Fail-proof: revert the fold to
port-wide → `t2_clkcor` red on the corner-null cells; revert
`record_endpoint` to `(Some, Some)` → same cells red (endpoint drops
instead of emitting partially). Restore each.

## Step 4 — Wording + records (R4, R5, docs)

- `sdc.rs`: `t2_eclk`/`t2_eclk family` citations now map to real lines
  (audit each); `(measured)` on the no-arg `get_clocks` cites
  `t2_eclk2`; `DelayRule::clock`/`delay_at` docs restated.
- `docs/results.md`: RD1 rewording + RD2 revision entry.
- `docs/multi-clock.md`: RD3 sync (counts, §1.7 bullets, limitation).
- `docs/timing-exceptions.md`: RD4 `unreached_endpoints` restatement.
- `tests/designs/README.md`: RD5 count + coverage note.

## Step 5 — Version bump + final gate

- `[workspace.package] version` `0.2.7 → 0.2.8` in `Cargo.toml`;
  `cargo check` refreshes `Cargo.lock`.

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --stat tests/designs/   # golden: append-only blocks + regenerated t2_eclk probe
```

Optional heavy: `cargo test --release -p grus-core sta`.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/sdc.rs`
- `crates/grus-core/src/sta/parasitics.rs`
- `tests/designs/sta_clocks/probe.sh`
- `tests/designs/sta_clocks/t2_*.sdc`
- `tests/designs/sta_clocks/probe_t2_*.txt`
- `tests/designs/sta_clocks/two_clock.reference.json`
- `docs/results.md`
- `docs/multi-clock.md`
- `docs/timing-exceptions.md`
- `tests/designs/README.md`
- `Cargo.toml`
- `Cargo.lock`
