# e6-drt-e2e-picorv32 — overview (r1)

The deferred picorv32 leg of `e6-drt-e2e`. The gcd gate is green and
committed (`gcd.drt.ours.reference.json`, `input_def_sha_prefix`
`635f19a9aae86a44`, `lef_expressible` ours = 0 on all 50 rules); the
picorv32 leg was deferred by memo and ships `#[ignore]`d with the
full `ours_reference_gate` joins already written. This ticket runs
the leg and lands whatever it measures.

picorv32 is ~7× gcd (6,811 signal nets / 25,037 member pins vs
gcd's 269 / 931) and a different cell mix — it is the only
scale-class measurement the gate has. The last picorv32 emit on
record (v0.12.6 mini run, 5,514.82 s wall) measured
`routed=6628 unrouted=116`, `area_patches=39 area_residuals=3`,
self-DRC `findings_total=3` (all `li.6`, named islands `_02450_` /
`_02905_` / `_04490_`) — but that predates every e2e mechanism
commit (`d8d0c6c` hull mask, `4c5e2cb` confinement, `b372d5e`
Rule A/B, `fba5f85` residual census, `0d113ba` X3 fill fix,
`5d1502b` refactor). Nothing about the fresh leg is presumed:
unrouted count, `lef_expressible` findings, and the new residual
classes are all measured, not extrapolated.

## Scope

1. **Emit leg** — `tests/designs/generate_drt_ours.sh picorv32
   --verify` (release binary, detached; the `--verify` arm is the
   script-side determinism measurement). Produces the gitignored
   `picorv32.drt.ours.routed.def` + `runs/drt-ours/picorv32.drt.ours.log`.
   Details: `emit-leg.md`.
2. **Check leg** — `tests/designs/check_drt_ours.sh picorv32`
   inside the pinned image; commits
   `picorv32.drt.ours.reference.json` (step-E output;
   `image_id`/`generated_by`/`input_def_sha_prefix` recorded).
   Details: `emit-leg.md`.
3. **Gate leg** — un-ignore `test_e2e_ours_reference_picorv32`;
   make `PINNED_LEF_RESIDUALS` per-design (the shared const cannot
   express a picorv32-only pin without breaking gcd). Determinism
   leg per `gate.md`'s decision tree, choice recorded.
   Details: `gate.md`.
4. **Script fix (card #44)** — `check_drt_ours.sh` `unrouted_kinds`
   null-vs-fail fallback: a present `guided=` census line with a
   missing `unrouted_kinds` line is `fail`, not `null` — the
   emit/script version-lock contract in `e6-drt-e2e`'s
   scripts.md/golden.md. Details: `script-fix.md`.
5. **Measurement record** — `docs/results.md` dated entry
   (comparison table, unrouted census, new-finding decomposition
   per `residuals.md`), `docs/e6-drt.md` §9 outcome paragraph +
   claim boundary, `docs/status.md` deferral clause. Details:
   `docs.md`.

## Measured context (what the leg starts from)

| surface | gcd (committed golden) | picorv32 (last emit on record, v0.12.6, pre-e2e commits) |
|---|---|---|
| guided / routed / unrouted | 269 / 188 / 81 | 6,744 / 6,628 / 116 |
| `lef_expressible` ours | 0 on all 50 | unmeasured under the deck — self-DRC showed `li.6`×3 named residuals |
| antenna | 0 / 0 | unmeasured (baseline 19+19, delegated 14+16 — the reference's own violations) |
| route wall | `route_ms=11,526` | 5,514.82 s clean M4 / 30,658 s loaded M1-Pro |
| delegated / baseline wirelength | 6,046.305 / 6,154.505 µm | 313,416.22 / 320,162.155 µm |
| delegated / baseline via total | 1,807 / 1,797 | 61,307 / 60,028 |

Known scale-sensitive suspects the leg's census feeds (the
follow-up tickets `drt-guide-escape` / `drt-unrouted-census` /
`drt-rayon` consume this data): the 592 `region_escape` pins
(355 inst + 237 io), the 43-net met3-bridge `NoPath` class, the
`transit_extend` hull-grid scan cost, the 3 `li.6` `area_residuals`.

## Decision points (no pre-committed answer)

- **Green vs red leg**: both are valid measurements. Green → commit,
  un-ignore, done. Red with named-mechanism residuals → decompose
  per `residuals.md`, pin per-design with site anatomy, record.
  Red without a named mechanism at pin granularity → the
  measurement is the ticket's outcome; escalate on the card rather
  than mass-pin or rebaseline (`residuals.md` §5).
- **DEF commit**: `<1 MB` → gitignore exception + commit (gcd
  convention); `≥1 MB` (expected ~4–6 MB — delegated is 7.15 MB)
  → stays gitignored, reproduction script is the committed path.
- **Determinism leg**: `--verify` default; a byte-compare test
  exists only if the DEF commits, and stays `#[ignore]`d either
  way (dev-profile picorv32 route is hours — the existing
  `test_maze_determinism_picorv32` is already the ignored
  test-side arm). `gate.md` §3.

## Non-goals

- No router/emit changes as the ticket's plan — the leg is a
  measurement. A red finding is decomposed and pinned, or fixed in
  a follow-up; a small in-ticket fix lands only when the mechanism
  is named and the fix is the same order of work as pinning it
  (and it re-emits + re-checks, never edits the golden).
- No mask/spacing/hard-block relaxation, ever — same non-goals as
  `e6-drt-e2e`.
- No `workflow.toml` edits anywhere — the driver reads the main
  checkout's copy (`timeout_sec = 7200` on dev, `63735c5`); the
  worktree copy is never consulted and touching it collides at
  merge (`e6-drt-e2e/design/verify-timing.md`). The suite's time
  grows by seconds only — the un-ignored test is a JSON join.
- No hand-built artifacts — the golden is one fresh step-E output
  on a `generate_drt_ours.sh`-produced DEF; `input_def_sha_prefix`
  must match the measured file.
- `picorv32.placed.def` / `picorv32.routed.def` stay gitignored
  (regenerate via `tests/designs/picorv32/generate_ref.sh`).

## Acceptance traceability

| acceptance item | where designed |
|---|---|
| `picorv32.drt.ours.reference.json` committed, generated by the committed script (SHA + `input_def_sha_prefix` recorded) | `emit-leg.md` §3 |
| `test_e2e_ours_reference_picorv32` un-ignored, pinned to the golden; red is a valid measurement | `gate.md`, `residuals.md` |
| `check_drt_ours.sh` `unrouted_kinds` contract fixed (fail vs null) | `script-fix.md` |
| `docs/results.md` dated entry: table, census, decomposition | `docs.md` §1 |
| `cargo test --workspace`, clippy `-D warnings` | `steps.md` verify |

## Files

- `tests/designs/generate_drt_ours.sh` (invoked, unchanged)
- `tests/designs/check_drt_ours.sh` (unrouted_kinds fix)
- `tests/designs/picorv32/picorv32.drt.ours.reference.json` (new, committed)
- `tests/designs/picorv32/picorv32.drt.ours.routed.def` (generated; committed only if <1 MB + gitignore exception)
- `crates/grus-core/tests/drt.rs` (un-ignore + per-design pins + optional determinism leg)
- `.gitignore` (exception line only if the DEF commits)
- `docs/results.md`, `docs/e6-drt.md`, `docs/status.md`
- `Cargo.toml`, `Cargo.lock` (0.12.8 → 0.12.9)
