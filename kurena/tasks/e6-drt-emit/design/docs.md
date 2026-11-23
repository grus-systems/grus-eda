# e6-drt-emit — Documentation updates

All English, existing style. Every number in `results.md` comes from
the step-1/2/3 captures verbatim (detached picorv32 log, in-suite
gcd log, CLI run), with the test profile stated. Numbers measured
under the *new* occupancy marking may drift from the pinned
`e6-drt-maze` entry — record fresh values and name the cause
(PDN metal + io-port premark, projected stubs); do not back-edit
the maze entry.

## `docs/e6-drt.md`

New section after §7 "Maze search" — **§8 Routed DEF emission** —
covering, in the doc's claim-boundary voice:

- RouteDb → `net_geoms` mapping: square-cap expansion `seg ± w/2`
  (identical to `parse_net_wire`'s 2-point expansion and to the
  occupancy rasterization — the emitted DEF describes exactly the
  shapes the router priced).
- The emitted record grammar: `+ ROUTED <layer> ( p )( p )` /
  `NEW …` / `NEW <lower> ( x y ) <via>` / `+ USE SIGNAL` — and the
  **no-width deviation**: regular-net wires carry no width token
  because our own `NETS` parser consumes a bare integer as a
  coordinate; width is a `SPECIALNETS`-only construct. (Cite the
  spec's example grammar as the source of the deviation.)
- Via records name the `ViaTable` default — emitter never reselects.
- Template splice: everything outside `NETS` passes through
  verbatim (why `write_def_full` was rejected: it drops
  `VIAS`/`SPECIALNETS`/etc.).
- Unrouted nets: conns-only records, named in `EmitReport`.
- Byte-stability: the fixed-point argument (order + square-cap
  recovery), one paragraph.
- The self-DRC gate: `run_all` on the re-parsed emit, 50-rule
  subset — with the standing claim-boundary line: this is our
  subset on our DEF; the full Magic deck gate on our DEF is
  `e6-drt-e2e`.
- The emit-driven maze amendments land in §7 too (same doc, same
  change): PDN `SPECIALNETS` metal + via stacks as immovable
  `Obs(u32::MAX)`, io-port rects marked `Pin(own_net)` (priced
  band, not hard block — the `met3.2` `req_msg`/`resp_msg`
  finding class), and the projected-pin access stub `route_one_net`
  paints.
- CLI: the `drt` command surface + flags.

Also update the file's claim-boundary paragraph (§4) — it still
says routed DEF output is future work.

## `docs/results.md`

`### <date> — e6-drt-emit: routed DEF emission + self-DRC (v0.12.5)`

- What landed (dense paragraph): emit model, CLI, gate, census.
- **Self-DRC census**: per design, all 50 rules' candidates and
  findings (the zero table prints compactly — one line per family
  if all zero, or the full per-rule table if anything fires).
  Any firing rule: named with count + root-cause class.
- **Roundtrip evidence**: `out1 == out2` byte check, `net_geoms`
  re-parse equality, non-vacuity numbers (rects/vias totals).
- **Structural census**: the per-design table from census.md —
  wirelength + per-cut vias vs delegated + baseline, signed deltas,
  with the "informational, topology differs" line.
- **Unrouted**: picorv32's fresh residual count + named classes,
  plus an explicit line naming any delta vs the maze entry's pinned
  108 (43 met3-bridge + 65 met2-connected) and its cause — the
  WIP's occupancy-premark widening; cite the maze entry rather than
  re-listing names.
- **Fail-proof record**: both arms — injected-rect finding
  (rule + count), `occ: Disabled` finding (rule + count) — proving
  the gate goes red.
- **Determinism**: two-pass byte equality + identical SubsetReport.
- **Runtime**: emit is trivial; the picorv32 leg's measured wall
  clock verbatim (release profile, `#[ignore]` convention stated).
- **Checks run**: exact commands incl. `--release`/`--ignored`
  where used, suite wall clock, version bump.

## `docs/status.md`

New `Last updated:` head paragraph, demoting `e6-drt-maze` to the
next `Previous:` in the chain. Content: `e6-drt-emit` (`v0.12.5`) —
RouteDb → routed DEF through a NETS-section template splice
(no-width grammar, ViaTable-default via names, square-cap model);
`grus-eda drt` CLI; self-DRC gate state — honest wording:
"clean on our 50-rule subset on OUR routed DEF" or named residuals;
roundtrip byte-stability; census deltas recorded as informational;
remaining = `e6-drt-e2e` (full Magic deck on our DEF in the
container). Do **not** write anything implying Magic ran on our
output.

## `docs/roadmap.md`

One `- Progress <date> (`v0.12.5`, ticket `e6-drt-emit`):` bullet
after the `e6-drt-maze` line in the E6 block — emission model +
CLI + gate verdict + census headline + remaining (e2e), same style
as the prior bullets.

## Files

- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
