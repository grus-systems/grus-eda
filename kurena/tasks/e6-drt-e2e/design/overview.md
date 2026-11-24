# e6-drt-e2e — overview (r9, verify-evidence; implementation head `5d1502b`)

E6 stage gate: run the real Magic `drc(full)` / OpenROAD antenna deck
on **our** natively-routed DEF (`grus-eda drt`) inside the pinned
LibreLane container, against the committed `e6-drt-ref` surfaces.
Depends on `e6-drt-emit`.

**r9 — verify-evidence, still no code delta.** The second verify
timeout is ruled **structural, not a code issue**: dev-profile
`cargo test --workspace` needs ~2h+ on this machine (`drt` tests
dominate) vs the driver's 1800 s window. The memo's directive:
**do not re-run it inside the verify step.** Evidence already
exists — results.md what-ran (clean at `0d113ba`, confirmed
post-`5d1502b`), the completed r8 detached dev-profile log
(601 passed / 6 ignored, all `ok`), and the memo's designated
artifact: the detached `cargo test --workspace --release` run at
`/tmp/e2e-ws-release-test.log`, **already complete and green**
(601/0/6; ~121 s of test time). Verify collects it; if the driver
still demands the literal dev-profile command, `workflow.toml` is
flagged for a **human decision** — no fix-loop, no edit.
`verify-timing.md` (r9) carries the procedure; every other design
file stands unchanged.

**r8 record (superseded procedure, valid outcome).** r8's directive
was a T0 detached dev-profile suite — that run *did* complete
green (`tests/designs/gcd/runs/cargo-test-workspace.log`) and now
stands as on-record evidence; the r9 memo replaces the
launch-and-collect procedure with collect-only and adds the
release-profile artifact. Implementation at `5d1502b` confirmed
complete — **nothing to implement**.

**r7 record — as-built at `5d1502b`.** The human memo on the r6
design confirms that checkpoint is complete and correct — doc-sync
plus the `LineNodes`/`PaintLines` type-alias refactor in `maze.rs`
(`#[allow(clippy::too_many_arguments)]` on `access_vias`), a pure
rename with **no functional delta vs the green `0d113ba`**: the
committed DEF (`635f19a9aae86a44`) and golden are unchanged, and the
determinism byte-join re-proves it in verify. The gate stays green
(`lef_expressible` ours all-0, `unclassified` 0, antenna 0), the
comparison table, determinism and what-ran records are in
`docs/results.md`, tests + docs + the version bump are done.
The r5 "what remains" items all landed, including the central one
(met1.2 attribution → X3 fix → `PINNED_LEF_RESIDUALS` empty, not
pinned). Every other design file stands unchanged from r7.

## What landed (checkpoint trajectory)

- `d8d0c6c` — the hull mask (`hull-model.md`): `lef_expressible`
  ~3025 → 847.
- `4c5e2cb` — confinement tightening (union-exact cover, raw pin
  rects): 847 → 514.
- `b372d5e` — Rule A confinement + Rule B `close_self_spacing`
  (`self-spacing.md`), `RECT` fill emission, `notch_fills` census.
- `fba5f85` — residual decomposition (`residuals.md` §1–§4): fill
  dedup, `patch_net_min_area` same-net guard, `close_voids` +
  `hole_residuals`; DEF regenerated (`1534c323855a6029`), measured
  317 / `met1.2=32` at 20:07.
- `0d113ba` — **the final hop**: met1.2 attribution probes
  (`who6.tcl`/`who7.tcl`, sites D/E) classified all 7 surviving sites
  as **X3** — `try_fill` ran the li1-strict uncovered-cover carve on
  `hull_host_open` met1, truncating every member-hull fill 20–30 DBU
  short of the facing band. Fix: skip the carve on
  `mask.host_open(l)` (`maze.rs`). `notch_fills` 221 → 343, the
  `_099_` `notch_residual` closed by the same arm, `routed` 187 →
  188 / `unrouted` 82 → 81. Fresh full `check_drt_ours.sh gcd` run
  emitted the committed golden on the regenerated DEF
  (`input_def_sha_prefix=635f19a9aae86a44`). Gate tests + fixtures +
  docs + `0.12.6 → 0.12.7` in the same commit.
- `5d1502b` — **refactor + doc-sync only** (the memo's description):
  `LineNodes` (`layer → line → sorted in-region node indices`, the
  `layer_line_maps` pair) and `PaintLines` (the extraction ledger
  `(layer, line idx) → sorted (lo, hi, PaintSrc)` intervals — every
  committed wire/stub/fill/via plate carrying its `PaintSrc` for the
  self-spacing closure) type aliases in `maze.rs`, plus
  `#[allow(clippy::too_many_arguments)]` on `access_vias`. No
  functional delta — same emit bytes, same golden, same gate.

## Final measured state (committed golden `gcd.drt.ours.reference.json`)

| surface | value |
|---|---|
| `lef_expressible` ours | **0 on all 50 rules** (`ours == placed == routed_ref` throughout; `PINNED_LEF_RESIDUALS` = `[]`) |
| `magic_only` | `nwell.1` 6/6/6, `nwell.2a` 120/120/120, `nwell.4` 300/300/300 — all `inherited` (cell-master paint, identical on every input) |
| `unclassified_fired` | empty — both first-contact hierarchy-boundary names closed |
| `sanity_control` | probe mutation fires `met2.2` `new_rules_vs_clean`, `violated_rules_in_checked_deck=true` — control live |
| antenna (`check_antennas`) | 0 net / 0 pin violations |
| `ours_router` | `guided=269 routed=188 unrouted=81 projected=9 collisions=0 area_patches=1 area_residuals=0 iterations=8 hull_masked=227182 notch_fills=343 notch_residuals=0 hole_residuals=0`; `unrouted_kinds NoPath=46 Unattachable=52`; `unrouted_blockers l0ObsPin=1 l1Obs=4 l1Net=39`; `stub_outside_cover` 142 records |
| `self_drc` | `findings_total=0` |
| `delta` | wirelength −32.49% vs delegated / −33.68% vs baseline (on the 188 routed nets), via census +117/+10 on via2/via3 vs delegated (named in `docs/results.md`), `signal_nets_routed` −81 both |
| `drc count total` | 305 = the placed baseline's own 305 (the trajectory: ~3025 → 847 → 514 → 317 → 305, i.e. routing adds zero) |

Determinism: `generate_drt_ours.sh gcd --verify` → two byte-identical
emits (`OURS-DETERMINISTIC 635f19a9…`); `test_e2e_ours_def_deterministic_gcd`
byte-compares a fresh emit against the committed DEF test-side.

## What remains

1. **Verify/diffreview only** — `steps.md` carries the checklist;
   `verify-timing.md` (r9) carries *where* the
   `cargo test --workspace` evidence comes from (the completed
   `--release` log + the completed r8 dev-profile log — collected,
   never re-launched). clippy `-D warnings`, the golden-vs-DEF sha
   join, and the diff audit are unchanged. The literal in-window
   `test-workspace` command is a `workflow.toml` question flagged
   for the human.
2. **picorv32 leg — honestly open.** Deferred per the r4/r5 memo and
   flagged on the card: `picorv32.drt.ours.reference.json` is not
   committed; `test_e2e_ours_reference_picorv32` ships `#[ignore]`d
   with the full join set so a `--ignored` run fails loudly on the
   missing file. The acceptance item "committed for both designs"
   stays open by name — it is the one item verify cannot check green.

## Acceptance traceability

| acceptance item | state |
|---|---|
| `<d>.drt.ours.reference.json` generated by committed script in pinned image (SHA recorded), committed both designs | **gcd done** (`image_id sha256:02d50f…` recorded); picorv32 deferred — open item above |
| Magic `listall why` 0 on every `lef_expressible` on ours; `magic_only` decomposed to baseline class | green — all 50 at 0; 3 nwell rows all `inherited`; `unclassified_fired` empty |
| `generate_drt_ours.sh` byte-identical reruns | green — `--verify` arm + committed-DEF byte-equal test |
| wirelength/via/antenna deltas in `docs/results.md`, named outliers | landed — dated 2026-11-24 entry, via2/via3 +wirelength-rise named |
| `cargo test --workspace`, clippy `-D warnings` | green on record — dev-profile suite completed detached (601/6-ignored) + results.md what-ran at `0d113ba`/post-`5d1502b`; r9 evidence = the completed `--release` log collected at verify (`verify-timing.md`); literal in-window command flagged for human `workflow.toml` decision; clippy re-runs in-window |
| `docs/status.md` honest E6 state | landed — carries unrouted=81 + picorv32 deferral |

## Non-goals (held — nothing was relaxed)

- No relaxation of the hull mask, `ObsPin`/`Obs` hard blocks,
  `w/2 + s` scans, or `s_l` — the gate went green by *adding* legal
  paint (fills spanning the full facing band), never by loosening a
  check.
- No further access-ladder rungs — the 81 unrouted are the named
  connectivity residual (ladder documented in `pin-access.md`).
- No new Magic rules; the `lef_expressible` gate is unchanged.
- No claiming `count_flat` as a routing-level signal.
- Never rebaseline: the golden is one fresh script run's step-E
  output on the committed DEF; `input_def_sha_prefix` matches the
  committed file.

## Files (as actually touched on the branch, head `5d1502b`)

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/tests/drt.rs`
- `tests/designs/gcd/gcd.drt.ours.reference.json`
- `tests/designs/gcd/gcd.drt.ours.routed.def`
- `docs/results.md`
- `docs/e6-drt.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
- `tests/designs/picorv32/picorv32.drt.ours.reference.json` (deferred — intentionally absent)
