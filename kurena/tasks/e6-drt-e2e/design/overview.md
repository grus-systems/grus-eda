# e6-drt-e2e — overview (r4)

E6 stage gate: run the real Magic `drc(full)` / OpenROAD antenna deck
on **our** natively-routed DEF (`grus-eda drt`) inside the pinned
LibreLane container, against the committed `e6-drt-ref` surfaces —
and finish the verification so the gate records an honest verdict.
Depends on `e6-drt-emit`. This revision continues from checkpoint
`b372d5e` (review memo, 2026-11-24): **the self-spacing closure is
implemented; what remains is measurement, residual bookkeeping, and
the workspace gate — the previous driver timed out mid-verification,
not mid-design.**

## What b372d5e already landed

- `crates/grus-core/src/drt/maze.rs` — `close_self_spacing` (Rule-B
  repair pass, `self-spacing.md`): pairwise notch detection over the
  committed piece set (merged wire spans, `pair.default` via plates,
  member pin shapes), `try_fill` with shrink-retry against mask +
  occupancy blockers, `PaintSrc` provenance (`Tree`/`ProjStub(pi)`/
  `Access(pi)`/`PinShape`) so a failing fill rejects the *attach
  element* that painted the notch and the pin re-attaches on its next
  candidate; undroppable-prong notches land in
  `RouteDb.notch_residuals`. Also: `ACCESS_K = 8` and union-cover
  probing (`pin-access.md` rungs A1 + A3-partial).
- `crates/grus-core/src/drc/union.rs` — `rect_minus` +
  `rect_covered` (union-exact cover, the L-shaped-pin class).
- `crates/grus-core/src/drt/emit.rs` — `NetRoute.fills` →
  `NetGeom.wire_rects` → `metN ( x y ) RECT ( 0 0 w h )` records
  (non-square-cap rects emit as `RECT`, parser round-trips them).
- `crates/grus-eda/src/main.rs` — `notch_fills=<n>` on the `guided=`
  census line; `  notch_residual <net> <layer> a=… b=…` per
  unrepaired notch.
- `tests/designs/check_drt_ours.sh` — `notch_fills` parsed (emit log
  without the token → `fail`, version-locked); `notch_residual`
  lines kept in `cli_lines`.
- `tests/designs/gcd/gcd.drt.ours.routed.def` — **regenerated**
  (post-closure geometry, fills included).

## Measured state at b372d5e (emit log, not yet re-measured by Magic)

`guided=269 routed=188 unrouted=81 projected=11 collisions=0
area_patches=1 area_residuals=0 iterations=8 route_ms=14224
hull_masked=227182 notch_fills=296`; `unrouted_kinds NoPath=45
Unattachable=57`; `unrouted_blockers l0Obs=1 l1Obs=4 l1Net=38
l2Net=2`; `stub_outside_cover` = 155 pins; `self_drc
findings_total=2`:

- `met1.2 [70450,55460,70770,55720] x [70820,55520,71300,55660]
  gap=(50,0) bound=140 nets=_007_,_007_` — a same-net
  *separated-islands* pair the closure never saw (`residuals.md` §2:
  the `area_patches=1` post-closure growth is the primary suspect).
- `met1.7 [74360,42400,74652,42600] area=54680 bound=140000
  nets=_055_` — an enclosed hole **the fills created** (`residuals.md`
  §3: the emitted `RECT`s seal the right side of a wire U-shape).
- `  notch_residual _099_ met1 a=…{41240,34170,41380,34510}
  b=…{41470,34380,41790,34640}` — one undroppable-prong notch whose
  gap (90×130) is fully blocked (`residuals.md` §4).
- `_055_`'s DEF also shows **triplicated identical `RECT` fills**
  (`residuals.md` §1 — a fill-dedup bug; harmless paint, wrong count).

Committed `gcd.drt.ours.reference.json` is **stale**: it still reads
the pre-fix 514 (`met1.2=740`, `met2.2=56`) and its
`input_def_sha_prefix` no longer matches the committed DEF — the
memo calls this out; the refresh is this ticket's core step.

Unrouted moved 73 → 81 (`NoPath` 57→45, `Unattachable` 33→57) — the
expected notch-rejection cost (`pin-access.md` interaction section);
the 81 are the ticket's named connectivity residual per the memo
(remaining access rungs are not part of this revision).

## What remains (memo order)

1. **Bounded residual closure** (`residuals.md`) — three small fixes
   whose mechanisms are already evidenced in the committed DEF, plus
   the census/script contract for naming residuals: fill dedup,
   void-closure pass, same-net guard on the min-area patch walk.
   Each surviving finding then maps to a named site + mechanism —
   `notch_residual`/`hole_residual` census lines carry the sites
   Magic is expected to flag.
2. **Re-emit + container re-measure** (`steps.md`): `generate_drt_ours.sh`
   → `check_drt_ours.sh gcd` in the pinned image → commit the
   regenerated DEF + refreshed `gcd.drt.ours.reference.json` iff the
   `lef_expressible` outcome is zero-or-named-residuals. The memo's
   expectation: `met1.2 ~740 → small named residuals`.
3. **Gate tests + re-pins** (`gate.md`): the new e2e test joins the
   ours reference with a residual-pinning mechanism; the two tests
   currently red (`test_emit_roundtrip_gcd` on unrouted=81,
   `test_emit_selfdrc_gcd` on 2 findings) get fixed geometry or
   measured pins — never blanket-tolerated.
4. **Docs + verify** (`docs.md`): `docs/results.md` dated r3/r4 entry
   (trajectory ~3025 → 847 → 514 → final, per-site decomposition,
   unrouted-81 named, **picorv32 leg explicitly deferred** per the
   memo), `docs/e6-drt.md` e2e + self-DRC scope correction,
   `docs/status.md`/`docs/roadmap.md` honest line, version bump,
   `cargo test --workspace` + clippy `-D warnings`.

## Acceptance traceability

| acceptance item | where |
|---|---|
| `<d>.drt.ours.reference.json` generated by committed script in pinned image, committed both designs | `golden.md`, `steps.md` — **picorv32 deferred per r4 memo, noted in `docs/results.md`** |
| Magic `listall why` 0 on every `lef_expressible` rule on ours; `magic_only` decomposed, baseline-matched | `residuals.md` (close the measured sites), `gate.md` (asserts + pinned-residual mechanism) |
| `generate_drt_ours.sh` byte-identical reruns | `gate.md` (committed-DEF byte-equal + `--verify`) |
| wirelength/via/antenna deltas in `docs/results.md`, named outliers | `docs.md`, `golden.md` (`vs_baseline`/`vs_delegated`) |
| `cargo test --workspace`, clippy `-D warnings` | `steps.md` |
| `docs/status.md` honest E6 state (native-DRT subset scope / named residuals) | `docs.md` |

## Non-goals

- No relaxation of the hull mask, `ObsPin`/`Obs` hard blocks,
  `w/2 + s` scans, or `s_l` to clean the gate — repairs add scanned
  paint or drop candidates, never loosen a check.
- No further access-ladder rungs this revision — the 81 unrouted are
  the named residual the memo records (the memo supersedes r3's
  "recover to 0" push; the ladder stays documented in
  `pin-access.md` for a later ticket).
- No new Magic rules; the `lef_expressible` gate is unchanged.
- No claiming `count_flat` as a routing-level signal.
- No route-surgery repair for the `_099_`-class residual — a named,
  evidence-pinned residual is the honest call inside this ticket.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-eda/src/main.rs`
- `crates/grus-core/tests/drt.rs`
- `tests/designs/check_drt_ours.sh`
- `tests/designs/generate_drt_ours.sh`
- `tests/designs/gcd/gcd.drt.ours.routed.def`
- `tests/designs/gcd/gcd.drt.ours.reference.json`
- `tests/designs/picorv32/picorv32.drt.ours.reference.json` (deferred — `docs/results.md` notes it)
- `docs/results.md`, `docs/e6-drt.md`, `docs/status.md`, `docs/roadmap.md`
- `Cargo.toml`
