# drt-preemit-spacing — overview

## Problem (external review S1)

`emit.rs` writes routed DEF with no geometric check at the emit
boundary. `--self-drc` exists but is opt-in and *post*-emit: it
re-parses the file `write_routed_def` already wrote, so a bad emit
reaches disk before anyone looks at it. The same subset check run on
the in-memory emit shape set — between `apply_routes` and
`write_routed_def` — detects the violation before the file exists.

`drt-pin-collision` has landed (v0.12.13): foreign io-port `Pin` bands
are priced, recorded, and hard-blocked on the last iteration, with
survivors named in `RouteDb.pin_collisions`. This ticket is the
**verification net** for that prevention — an emitted-geometry check
that proves no foreign-pin-adjacency violation survives to the DEF.

## Mechanism

One new function in `emit.rs`, reused verbatim from the self-DRC
toolchain — no third geometry engine:

```
pub fn preemit_check(db: &DesignDb, tech: &TechDb) -> Result<SubsetReport>
{
    let design = crate::drc::geom::assemble(db, tech)?;
    Ok(crate::drc::report::run_all(&design, tech, &db.strings))
}
```

Called on `design_db` between `apply_routes` (which fills
`db.net_geoms`) and `write_routed_def` in the `drt` CLI arm. `assemble`
is the exact input self-DRC builds from the re-parse — same five shape
sources in the same order: signal wires (`net_geoms`), special wires,
via instances, placed-macro pin/OBS rects, io-port rects.

## Decisions (recorded per spec)

| choice | decision | why |
|---|---|---|
| check breadth | **full `run_all`** — all 50 `lef_expressible` rules, not `Family::Spacing` only | `run_all` *is* the self-DRC checker; a spacing filter adds code to remove coverage. Every family in the emitted set can reach the file (the picorv32 `*.6` min-area findings did). The ticket name says "spacing" because that is the motivating class (foreign-pin adjacency → `*.2`/`*.*` spacing rules); the check is the same subset, and `preemit_report == self_drc_report` becomes a testable invariant only under identical breadth. |
| flag shape | **`--preemit-check`**, a separate opt-in flag | Folding it into `--self-drc` would either double the check per `--self-drc` run or silently drop the re-parse (which is deliberate — it proves the emitted *text* describes the checked geometry). The two flags verify different things: `--preemit-check` detects earlier on the same shape set; `--self-drc` validates the serialization path. |
| exit status | **report-only** — findings print, the DEF still writes, exit 0 | `drt-selfdrc-exit` has no `design/` dir — not landed. Per spec, the gap is flagged: the `preemit_check scope:` line and `docs/results.md` both state that findings do not gate the write or the exit code until that ticket lands. |

## Why the in-memory set is *the* emitted set

`test_emit_roundtrip_gcd` already proves `re_parse(write(db)).net_geoms
== db.net_geoms` byte-exact; `seg_rect`/`centerline` are documented
inverses and `RECT` records round-trip verbatim. SPECIALNETS, PINS and
COMPONENTS are template-passed — unchanged by emit. So
`assemble(&db)` ≡ `assemble(&reparse(emit(db)))` as geometry (the
`net` `SymbolId`s may differ across interners — irrelevant: `run_all`
resolves names to `String`s). The clean-case test asserts
`SubsetReport` equality across the two paths — the "same shape set"
claim is tested, not assumed (AGENTS §7.2).

## Claim boundary (corrects the spec's loose wording)

`checkable` (`spacing.rs`) admits `SignalWire | SpecialWire | Via |
IoPort` on routing layers and `Via` on cuts. `MacroPin`/`MacroObs`
join connectivity islands but **never generate findings** — the
subset's documented under-report vs Magic's GDS-plane checks
(`docs/e7-drc.md` §12). Consequences:

- The S1 "li1 pin-adjacency" class was largely wire-vs-`MacroObs`/`MacroPin`
  adjacency inside instance hulls — pairs our subset *cannot* flag.
  The honest claim is narrower: this check catches what `--self-drc`
  catches, earlier — including **io-port** adjacency, the class the
  occupancy model soft-priced until `drt-pin-collision`.
- Instance pin shapes are still *in* the assembled set (island
  membership — "obstacles" in the spec's sense), but extending
  `checkable` to them is a checker-semantics change with golden
  re-validation cost — out of scope; named here and in `results.md`.
- Positive-gap pairs only: a distance-0 overlap is `touching_ok`-exempt,
  same as everywhere in the subset. Committed wire×pin *overlaps*
  remain `RouteDb.pin_collisions` territory — complementary classes.

## Files

- `crates/grus-core/src/drt/emit.rs`
- `crates/grus-eda/src/main.rs`
- `crates/grus-core/tests/drt.rs`
- `docs/results.md`
- `Cargo.toml` (workspace `patch` bump per AGENTS §5)
