# Implementation steps

One logical unit per commit; Conventional Commits + agent trailer
(`AGENTS.md` §5). Permission-free work first (`AGENTS.md` — ordering):
parser/duration changes need no container; the fixture golden does.

## Step 1 — activity state durations + `when` grammar (no container)

Independent of the probe verdict: whatever rule wins, the marginal
probability data and the expression parser are needed.

- `readers/vcd.rs`: `SignalActivity` + `t0_s`/`t1_s`; per-id lazy
  duration accumulation; emit entries for all declared signals
  (`api.md`). Unit tests for durations + update the existing
  constructors for the new fields.
- `readers/saif.rs`: `annotate()` fills `t0_s`/`t1_s`; module doc update.
- `lib_db.rs`: `WhenLit` + `parse_when_expr` + unit tests.
- `cargo test --workspace` + clippy green. The only behavior delta is
  constant nets gaining `signals` entries (`driven_nets_without_activity`
  semantic) — record in `results.md`.
- Commit: `feat: carry per-net state durations and a when-expression
  grammar for power annotation`.

## Step 2 — fixture + probes (container; verdict + golden)

- Author `mbw.def`/`mbw.sdc`/`mbw.vcd` + the probe variants
  (`fixture.md`), `generate_when_ref.sh`.
- Run `bash tests/designs/microbench/generate_when_ref.sh` inside
  LibreLane 3.0.16 (Colima). Emits `mbw.openroad.txt`,
  `mbw.saif`, `mbw.reference.json`, and every probe table.
- If `mbw.sdc` needs a clock after all, apply the documented fallback
  and record which landed.
- `docs/results.md`: dated entry with the P0–P4 numbers and the three
  verdicts (count-net, marginal-vs-joint, leakage model) — commit the
  fixture + golden + this record together (AGENTS §7 rules 1, 9: say
  which parts of the script ran).
- Commit: `test: add mbw when-power fixture, OpenSTA golden, and
  measured when semantics`.

## Step 3 — `when` evaluation (verdict-dependent shape)

Expected (H1 marginal): the `api.md` shape — `net_p1`, `eval_when`,
`weight` multiply, diagnostics doc update, module doc update. If P1
returns related-pin counting, flip the `count_net` line and name it in
the commit. If the verdict is H2 or the leakage probe returns
state-weighted, apply the documented contingency (liberty.rs enters the
change; rescope is stated in the commit + `results.md` before landing).

- `power.rs` `test_when_power_mbw_matches_opensta` green at the
  measured tightest bounds; `conditional_power_blocks == 0` on the
  fixture.
- Fail-proof: `weight = 1.0` → red → restore; corrupted golden → red →
  restore. Record in `results.md`.
- Bump `[workspace.package] version` 0.1.96 → 0.1.97 in this commit
  (functional change, patch — `AGENTS.md` §5).
- Commit: `feat: evaluate when-qualified internal_power blocks at their
  measured marginal probability`.

## Step 4 — docs close-out

- `docs/results.md`: finish the entry (golden deltas vs bounds, the
  fail-proof record, full-suite checks).
- `docs/roadmap.md` N7: `when`-conditional blocks move from "still open"
  to done, one line naming the measured rule; keep the
  activity-propagation gap open.
- Commit: `docs: record when-power rule and close N7 conditional
  blocks`.

## Verification gates

- `cargo test --workspace`, `cargo clippy --workspace --all-targets --
  -D warnings` after every step.
- `bash tests/designs/microbench/generate_when_ref.sh` idempotent —
  rerun → clean `git status` on `mbw.*`.
- Existing `mb`/`mb.saif` tests stay green (no `when` cells; golden
  untouched — `git diff` on `mb.*` must be empty).
- `grus-eda power --vcd tests/designs/microbench/mbw.vcd` end-to-end
  clean; diagnostics show `conditional_power_blocks 0`.

## Out of scope (stays open)

- OpenSTA's statistical activity propagation (the documented gcd-class
  divergence) — `when` pins on unannotated nets are counted, not
  propagated.
- `when` on `timing` arcs and `leakage_power` `when` unless P4 returns
  state-weighted (then only `leakage_power` gains the contingency
  implementation).
- `|`, `^`, parenthesized `when` grammar (never instantiated for
  `internal_power` in sky130_fd_sc_hd — counted).
