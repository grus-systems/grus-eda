# e6-drt-e2e-picorv32 — docs updates (r1)

`docs/results.md` is the measurement log; `docs/e6-drt.md` §9 and
`docs/status.md` carry the claim boundary. All numbers transcribe
from the committed golden — never hand-write or round (E0 rules
1–2, 9–10).

## 1. `docs/results.md` — dated entry

Shape mirrors the gcd `2026-11-24` entry. Sections:

- **Header** — `tests/designs/check_drt_ours.sh picorv32` inside
  the pinned image (`image_id sha256:…`), run on
  `picorv32.drt.ours.routed.def` (`input_def_sha_prefix=…`); the
  golden `picorv32.drt.ours.reference.json` commits with the
  entry. Emit-side provenance: `generate_drt_ours.sh picorv32
  [--verify]`, `grus_eda_version`, `route_ms` per arm.
- **Gate table** — `lef_expressible` nonzero rows (ours/placed/
  routed_ref, or "0 on all 50"); `magic_only` rows vs the placed
  baseline (nwell census expected ~8,084 `nwell.4` / 2,756
  `nwell.2a` / 242 `nwell.1` — the routed reference's counts;
  cell-master paint is placement-invariant);
  `unclassified_fired`; `sanity_control` (which rule the probe
  fired, `violated_rules_in_checked_deck`); antenna net/pin
  counts — name ours vs the reference's own violations
  (baseline 19+19, delegated 14+16).
- **Connectivity record** — `routed`/`unrouted` with the named
  list living in the golden (no inline dump); `unrouted_kinds`
  decomposition; `unrouted_blockers`; `stub_outside_cover` census
  (records ≥ `Unattachable`, accumulation semantics named);
  `projected`, `collisions`, `area_patches`/`area_residuals`,
  `notch_fills`/`notch_residuals`/`hole_residuals`,
  `hull_masked`, `iterations`, `route_ms`. Class decomposition
  per `residuals.md` §4 (met3-bridge / met2-connected /
  region_escape / new classes — counted from the census, not
  presumed).
- **New-finding decomposition** — for every `ours != placed`
  finding: site count, the `who*.tcl` probe record (names,
  `drc why`/`select area`/`what`, Magic-units ×5 note), mechanism
  class (X1/X2/X3), and the fix-or-pin verdict with the named
  site(s) the pin covers. Inherited rows state "inherited" with
  counts; `count_flat` is recorded, never claimed as a signal.
- **Comparison table** — vs `variants.delegated` and
  `variants.baseline` (from the golden's `delta` block):
  `total_wirelength_um` + pct, `via_count_total` +
  `vias_per_cut_layer`, `signal_nets_routed`,
  `antenna_violations`. Note ours routes on the *committed guide*
  while the delta's via/wire comparison is against two different
  routers' outputs — informational, same convention as gcd.
- **Determinism record** — `OURS-SHA` / `OURS-DETERMINISTIC` if
  `--verify` ran, or the explicit skip decision + `route_ms`
  projection; the DEF's size + commit decision (<1 MB rule).
- **`unrouted_kinds` fix** — card #44: what changed, the
  fail-arm exercise (`script-fix.md` §2), the gcd inertness
  check.
- **What ran** (E0 rule 9) — exact commands, host, wall times
  (`runs/drt-ours/runtime.json` transcribed), probe names,
  test commands, `cargo test --workspace` + clippy verdicts.

## 2. `docs/e6-drt.md` §9 — claim boundary update

Replace the deferred paragraph:

> **picorv32** — leg deferred (r4/r5 memo): no golden commits;
> `test_e2e_ours_reference_picorv32` is `#[ignore]`d with the
> same joins ready.

with the measured outcome paragraph — golden committed, what
`lef_expressible`/`magic_only`/`unclassified`/antenna read, the
unrouted census summary, pins if any (named), and the determinism
choice (which arms exist and which ran). Update the "Test gate"
paragraph: `test_e2e_ours_reference_picorv32` is live; the
determinism leg's actual shape (byte-compare test iff the DEF
committed, else `--verify` + `test_maze_determinism_picorv32`).

## 3. `docs/status.md` — sync

The "Last updated" block names "picorv32 leg deferred — no
golden, the `#[ignore]`d gate test carries the joins". Update to
the new state (golden committed, leg measured, pins named if
any). E0 rule 8: keep `docs/` in step with the code — this is
the same doc-sync obligation the parent ticket followed for its
deferral wording.

`docs/roadmap.md` — optional one-line append to the E6 progress
trail naming the leg's landing; keep it terse like the existing
2026-11-24 bullet.

## 4. Version

`Cargo.toml` `[workspace.package] version` `0.12.8 → 0.12.9`
(patch on 0.x — test un-ignore + script contract fix + golden
land; no breaking change). Same commit as the functional change,
`Cargo.lock` regenerated. Bump **before** the check leg runs —
the golden's `ours_router.grus_eda_version` records the version
it was generated under.

## Files

- `docs/results.md`
- `docs/e6-drt.md`
- `docs/status.md`
- `docs/roadmap.md` (optional one-liner)
- `Cargo.toml`, `Cargo.lock`
