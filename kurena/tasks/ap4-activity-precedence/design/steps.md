# steps — implementation order and gates

One commit (~150–250 lines incl. test + docs). No container session
needed: every golden number is already committed; the gcd
re-comparison is a CLI run against `docs/results.md` numbers, and
`gcd.vcd` is already in the worktree (seeded).

## 1. `activity.rs` — `User` origin + `pins` arm

Per api.md §1–§3:

- `ActivityOrigin::User` + `label() == "user"`; `Annotated` doc
  narrows to dump entries.
- `ActivityOverrides::pins: Vec<(PinId, ActivitySetting)>`; drop
  `Copy` from the derive.
- `pin_overrides: HashMap<PinId, ActivitySetting>` built once in
  `resolve_activity` (`.collect()` — last dup wins).
- Seed chain: insert the `pin_overrides` arm and retarget the
  `input_ports` arm to `User` between `global` and dump annotation.
- `dead_user_entries: usize` on `ActivityResolution`: one-time count
  of `pins` entries where `!pin_is_driver[p] || pin_nets[p].is_none()`.
- Doc comments: header fill order, `input_ports` arm comment, `-pins`
  doc per api.md §2.
- Gate: `cargo test -p grus-core` — expect only the mba test red
  (`s_input_ports` `a`/`b` source `annotated` vs golden `user`) — that
  red *is* the pre-fix proof; fix in step 2 before rerunning.

## 2. `power.rs` — test rework

Per golden.md:

- `"user"` leaves the `vcd|saif` collapse; new arm asserts
  `ActivityOrigin::User` on driver pins (residual otherwise —
  none today).
- `s_input_ports` `origin_counts` pin `{clock 1, constant 4, input 1,
  propagated 6, unknown 4, user 2}`.
- `dead_user_entries` folded into the diagnostics assert (0
  everywhere).
- Post-loop `pins`≡`input_ports` equivalence on `s_input_ports`
  (`pin_obj["a"]`/`pin_obj["b"]` → `pins`, `input_ports` cleared;
  verbatim per-net compare).
- Test doc comment: `user` no longer collapses.
- Gate: `cargo test -p grus-core power` green.

## 3. `main.rs` — `ORIGIN_ORDER` gains `"user"`

After `"global"` per api.md §5. No flags.

## 4. Fail-proof (record all three in the entry)

- `User` arm relabeled `Annotated` → `s_input_ports a: source` red;
  restore.
- `pin_overrides` lookup removed → `pins` equivalence assert red;
  restore.
- Golden `s_p40.a` density `2.5e6 → 3e6` → red at the density bound;
  `git checkout --` restore, green.

## 5. gcd re-comparison

Per gcd.md: verify `gcd.vcd` present; run the committed-shape command
(`--vcd`, zero-wire parasitics) plus the no-annotation variant; record
command, origin split, diagnostics, three-column table, moved/residual
analysis in the results entry.

## 6. Docs + version

- `docs/results.md` AP4 entry per docs.md §1 (sections 1–7).
- `docs/roadmap.md` N7 rewrite per docs.md §2.
- `Cargo.toml` `0.2.17 → 0.3.0` (minor — `Copy` dropped, api.md §6);
  `cargo check` refreshes `Cargo.lock`.
- `mb.power.json` regen via the recorded CLI command; `git diff`
  empty.

## 7. Final verify (all recorded in the entry)

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --release -p grus-core sta`

## Acceptance cross-check

| acceptance item | where |
|---|---|
| `-input_ports`/`-pins` resolvable via API, origin `user`; `-activity` vs `-density` at call site, `s_p40` pinned; precedence user < global/clock/constant, > input | api.md §1–§4; steps 1–2 |
| `s_input_ports` + `s_p40` per-pin `{d,P,source}` verbatim; 12 sessions match; fail-proof both new arms | golden.md; steps 2, 4 |
| gcd `report_power` re-comparison in `results.md` — moved/diverged named | gcd.md; step 5 |
| N7/roadmap updated; label + `unknown` residuals recorded | docs.md; step 6 |
| `cargo test --workspace` + clippy + `sta` release; version bump same commit | steps 6–7 |

## Files

- `crates/grus-core/src/activity.rs` — `User`, `pins`,
  `dead_user_entries`, doc updates
- `crates/grus-core/src/power.rs` — header comment, test rework
- `crates/grus-eda/src/main.rs` — `ORIGIN_ORDER` `"user"`
- `docs/results.md`, `docs/roadmap.md`
- `tests/designs/microbench/mb.power.json` — regenerated, diff empty
- `Cargo.toml` (0.3.0), `Cargo.lock`
- `kurena/tasks/ap4-activity-precedence/design/` — this document set
