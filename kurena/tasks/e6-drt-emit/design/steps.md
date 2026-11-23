# e6-drt-emit — Implementation steps

Order matters: the roundtrip test proves the grammar before the
self-DRC gate consumes it; picorv32 evidence runs detached while
docs are drafted. Estimated diff: ~300–400 lines code + tests
(emit.rs ~200, CLI ~60, tests ~150) — larger than the ~50–150
guideline but one coherent unit (the roundtrip can't be split from
the writer it tests).

## 1. `drt::emit` module + `apply_routes`

- New `crates/grus-core/src/drt/emit.rs` per api.md; `pub mod emit`
  in `drt.rs`.
- `apply_routes`: resize `db.net_geoms` to `num_nets`; iterate
  `route_db.nets` (each `NetRoute` carries `nr.net: NetId`) → build
  `NetGeom`; clear `net_geoms` for every `route_db.unrouted` entry's
  `net`. `RouteDb` has no outcome enum — `nets` holds only
  successful routes, `unrouted: Vec<UnroutedNet{net, connected,
  failed}>` the rest.
- No width token anywhere; square-cap expansion; via via
  `pairs[..].default`.

## 2. `write_routed_def`

- Port `write_def`'s `COMPONENTS` splice mechanics to `NETS`
  (scan for `NETS ` … `END NETS`); verbatim passthrough elsewhere.
- Record emission per def-model.md: name via `guide_net_name`,
  conns from `net_pin_ids` (`( comp pin )` / `( PIN name )`),
  `+ ROUTED` first geom stmt, `NEW` rest, `+ USE SIGNAL ;`.
- Centerline recovery with the bail arm for non-square-cap rects.

## 3. Roundtrip test first

`test_emit_roundtrip_gcd` — the grammar proof. Green before moving
on; a red here means the grammar diverged from `parse_net_wire`
(most likely spot: via-record layer token, or a stray width).

## 4. CLI `drt` subcommand

`DrtArgs` + `Commands::Drt` + run arm per api.md; census print
block; `--reference` JSON load for the delta columns;
`--self-drc` runs the gate sequence and prints findings (never
exits nonzero on findings).

## 5. Self-DRC gate + fail-proof tests

`test_emit_selfdrc_gcd`, `test_emit_selfdrc_picorv32` (`#[ignore]`),
`test_emit_fail_proof` (both arms), `test_emit_determinism`,
`test_emit_unrouted_fixture`, `test_emit_via_default_consistency`,
CLI parse test.

If `run_all` fires on our routes: classify per selfdrc.md's table —
real router bug → fix in `drt/maze.rs`/`emit.rs` in place (small,
focused); structural looseness → name rule + count + class in
`results.md`. Never loosen the assert.

## 6. Long legs — detached, before docs

```
cargo test --release -p grus-core --test drt \
    test_emit_selfdrc_picorv32 -- --ignored --nocapture \
    2>&1 | tee /tmp/emit-picorv32.log
```

Captures: per-rule census, any finding dumps, census block,
unrouted names, wall clock. The 65 met3-bridge `NoPath` nets are
expected in `unrouted=` — anything else is a regression in maze
land, named not silenced.

## 7. Docs (docs.md)

`docs/e6-drt.md` emission section, `docs/results.md` dated entry,
`docs/status.md` head line, `docs/roadmap.md` progress bullet.

## 8. Verify

```
cargo test --workspace                                # detached, tee log
cargo clippy --workspace --all-targets -- -D warnings
```

The workspace gate now includes gcd route+emit+DRC legs
(fast) — picorv32 stays `#[ignore]`d, so the gate does not
inherit the 135-min leg. Record real wall clock in results.md.

## 9. Version + commit

`[workspace.package] version` `0.12.4 → 0.12.5` (feature → patch
per 0.x rule), same commit. Conventional Commit, English, agent
trailer:

```
feat: Routed DEF emission and self-DRC gate
```

## Contingencies

- **`run_all` red on gcd**: expected classes listed in selfdrc.md —
  fix-or-name; a spacing red reachable by the Disabled arm but not
  the Enabled arm means the gate is vacuous — investigate the
  fixture, don't accept the pass.
- **Via default `None` at emit** (gap pair reached emit): the
  router skips gap transitions, so a `ViaPlace` on a gap pair means
  graphs/table disagreement — bail loudly, it's a bug not data.
- **Re-parse error on our own text**: grammar drift — compare the
  emitted record against `parse_net_wire` token-by-token; the
  `NEW <lower> ( x y ) name` via form is the most fragile spot.
- **picorv32 leg finds violations**: name every rule + count in
  `results.md` per the acceptance wording — the gate records them
  honestly; "0 or named" both satisfy the acceptance.

## Files

- `crates/grus-core/src/drt/emit.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-eda/src/main.rs`
- `crates/grus-core/tests/drt.rs`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
