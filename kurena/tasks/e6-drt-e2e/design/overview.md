# e6-drt-e2e — overview (r5)

E6 stage gate: run the real Magic `drc(full)` / OpenROAD antenna deck
on **our** natively-routed DEF (`grus-eda drt`) inside the pinned
LibreLane container, against the committed `e6-drt-ref` surfaces.
Depends on `e6-drt-emit`. This revision continues from checkpoint
`fba5f85` (review memo, 2026-11-24): **the code work is done and the
post-fix measurement exists — what remains is attribution of the 32
`met1.2` fragments, the workspace gate, and the record. The previous
driver timed out post-measurement, pre-commit; the numbers are
generated, this revision finishes them.**

## What fba5f85 already landed (code complete)

- `crates/grus-core/src/drt/maze.rs` (+692) — the full residual
  closure: `close_self_spacing` fill-dedup (`residuals.md` §1),
  `patch_net_min_area` same-net guard (§2), `close_voids` enclosed-void
  pass + `hole_residuals` (§3), `PaintSrc`/`ClosureResult::{Done,
  Reject}` mechanics.
- Unit coverage in `maze.rs::tests` — `test_closure_fill_dedup`,
  `test_min_area_patch_same_net_guard`,
  `test_min_area_patch_same_net_residual`,
  `test_close_voids_fill_and_residual` (fill + residual arms).
- `crates/grus-eda/src/main.rs` — `guided=` gains
  `notch_fills/notch_residuals/hole_residuals`; `  notch_residual` /
  `  hole_residual` evidence lines.
- `tests/designs/check_drt_ours.sh` — version-locked census regex +
  residual keep-list (r4 contract, landed).
- `tests/designs/gcd/gcd.drt.ours.routed.def` — regenerated
  post-closure, committed; sha256 prefix `1534c323855a6029`.

## Measured state (logs generated; JSON never persisted)

`check_drt_ours.sh gcd` ran in the pinned image on the committed DEF
at 20:07 on 2026-11-24 — all four transcripts + `mutation.json` are
in the gitignored `tests/designs/gcd/runs/drt-ours/` workdir — **but
step E's JSON output was never persisted**: the driver timed out
between measurement and assembly, and the committed
`gcd.drt.ours.reference.json` still holds the *pre-closure* golden
(`count_total=514`, `met1.2=740`, `met2.2=56`,
`input_def_sha_prefix=6671d184`, `unrouted=73` — byte-identical to
HEAD, measuring a DEF that no longer exists). One fresh full
`check_drt_ours.sh gcd` run on the committed DEF is owed to emit the
commit-candidate JSON (`steps.md` §B); a rerun that diffs from the
numbers below on the same DEF + image is itself a finding to record.

The 20:07 transcripts measure:

| surface | value |
|---|---|
| `lef_expressible` ours | **`met1.2 = 32` fragments only; 49/50 rules at 0** (`ours==placed` elsewhere) |
| `magic_only` | nwell.1=6, nwell.2a=120, nwell.4=300 — all `ours == placed == routed_ref` (inherited) |
| `unclassified_fired` | **empty** — the two first-contact names are gone |
| `self_drc` | `findings_total=0` (was met1.2 `_007_` + met1.7 `_055_` — both closed) |
| `ours_router` | `routed=187 unrouted=82 notch_fills=221` (dedup'd from 296) `notch_residuals=1` (`_099_`) `hole_residuals=0` `area_patches=1 area_residuals=0` |
| `sanity_control` | probe fired `met2.2` (new vs clean) — control live |
| `delta` | wirelength −48.5% / −49.4% vs delegated/baseline (restated "on the 187 routed nets"), via/antenna deltas recorded |
| `drc count total` | 317 (was ~3025 → 847 → 514 across the fix trajectory) |

The 32 `met1.2` fragments cluster into **7 sites**: 4 fragments on the
already-named `_099_` `notch_residual`, **28 fragments on 6 new
sites, each adjacent to an emitted met1 `RECT` fill on a different
net** — `met12-attribution.md` has the per-site table and the
fix-vs-pin decision rule.

## What remains (memo order)

1. **Attribute + name the 32** (`met12-attribution.md`): site table
   measured; in-container `drc why` on ≥2 sites names the partner
   class (X1 invisible subcell / X2 LEF-vs-mag pin / X3 masked
   within-island / X4 euclidean pair). X3/X4 → bounded closure fix +
   re-measure; X1/X2 → `PINNED_LEF_RESIDUALS` with every site named.
   Then **one fresh `check_drt_ours.sh gcd` run** emits the golden on
   the final committed DEF → commit `gcd.drt.ours.reference.json`
   (+ regenerated DEF if a fix lands) — never rebaseline.
2. **maze.rs closure unit coverage** — dedup/guard/void arms exist;
   the `Reject` arm (droppable prong) and the `notch_residuals`
   production arm (undroppable notch → named residual) have no direct
   fixture. Two small fixtures close the gap (`gate.md` §4).
3. **Gate tests** (`gate.md`): `test_e2e_ours_reference_gcd` (new —
   taxonomy join + pinned-residual mechanism + `unrouted` golden
   join), `test_e2e_ours_def_deterministic_gcd` (byte-equal vs
   committed DEF), `test_emit_roundtrip_gcd` re-pin (unrouted →
   golden `ours_router.unrouted` join), `#[ignore]`d picorv32 leg.
4. **Docs + verify** (`docs.md`): `docs/results.md` dated r3 entry
   (trajectory ~3025 → 847 → 514 → 317, 7-site table, 82 unrouted
   named, **picorv32 leg explicitly deferred**), `docs/e6-drt.md` e2e
   + self-DRC scope, `docs/status.md`/`docs/roadmap.md` honest E6
   line, `Cargo.toml` 0.12.6 → patch bump,
   `cargo test --workspace` + clippy `-D warnings` — **the workspace
   is red at `fba5f85`**: `test_emit_roundtrip_gcd` still asserts
   `unrouted.is_empty()` against the measured 82; the §2 re-pins are
   what turn it green, so a red baseline before the test work is the
   expected state, not a surprise.

## Acceptance traceability

| acceptance item | where |
|---|---|
| `<d>.drt.ours.reference.json` generated by committed script in pinned image, committed both designs | `golden.md`, `steps.md` — **picorv32 deferred per r4/r5 memo, noted in `docs/results.md` and on the card** |
| Magic `listall why` 0 on every `lef_expressible` rule on ours; `magic_only` decomposed, baseline-matched | `met12-attribution.md` (close or name the 7 sites), `gate.md` (strict + pinned asserts) |
| `generate_drt_ours.sh` byte-identical reruns | `gate.md` (committed-DEF byte-equal test + `--verify` arm) |
| wirelength/via/antenna deltas in `docs/results.md`, named outliers | `docs.md`, `golden.md` (`vs_baseline`/`vs_delegated`) |
| `cargo test --workspace`, clippy `-D warnings` | `steps.md` |
| `docs/status.md` honest E6 state (native-DRT subset scope / named residuals) | `docs.md` |

## Non-goals

- No relaxation of the hull mask, `ObsPin`/`Obs` hard blocks,
  `w/2 + s` scans, or `s_l` to clean the gate.
- No further access-ladder rungs — the 82 unrouted are the named
  connectivity residual (ladder documented in `pin-access.md`).
- No new Magic rules; the `lef_expressible` gate is unchanged.
- No claiming `count_flat` as a routing-level signal.
- No route-surgery repair for confirmed X1/X2 sites — named,
  evidence-pinned residuals are the honest call inside this ticket.

## Files

- `crates/grus-core/src/drt/maze.rs` (attribution-driven fix only + two fixtures)
- `crates/grus-core/tests/drt.rs`
- `tests/designs/gcd/gcd.drt.ours.reference.json`
- `tests/designs/gcd/gcd.drt.ours.routed.def` (only if a fix re-emits)
- `tests/designs/picorv32/picorv32.drt.ours.reference.json` (deferred — `docs/results.md` notes it)
- `docs/results.md`, `docs/e6-drt.md`, `docs/status.md`, `docs/roadmap.md`
- `Cargo.toml`
