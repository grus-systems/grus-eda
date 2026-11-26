# e7-drc-enc-area: enclosure/overlap + min-area + hole-area rules

E7 ticket 4, depends on `e7-drc-spacing`. Completes the remaining
`lef_expressible` rule families so all 50 unique rules are implemented.

## Scope

1. **Via enclosure / overlap** — `met1.4`, `met1.5`, `via.4a`/`via.5a`
   (as `Metal1 overlap of Via1 < %d in one direction (via.5a - via.4a)`),
   `met2.4`/`met2.5`, `via2.4a`, `met3.4`, `via3.5 - via3.4`,
   `met4.3 - via3.4`, `met5.3 - via4.4`: metal must enclose the cut by a
   margin, with the "in one direction" variants requiring the margin on
   at least one axis. Via placement comes from the geometry model —
   enclosure is measured on the via's own layer rects plus the owning
   net's same-layer shapes.
2. **Minimum area** — `li.6`, `met1.6`–`met5.4`-family
   (`met1.6`,`met2.6`,`met3.6`,`met4.4a`,`met5.4`): merged polygon area
   per net/island on the layer ≥ LEF `AREA` where the LEF declares one,
   else the recorded Magic constant.
3. **Minimum hole area** — `met1.7`, `met2.7`: holes inside a merged
   metal polygon ≥ 0.14 µm². Polygon-with-holes extraction goes through
   the same union structure spacing uses.

## Non-goals

- CLI (next ticket), antenna checks (`check_antennas` is a different
  surface per the taxonomy).

## Files

- `crates/grus-core/src/drc/enclosure.rs`, `drc/area.rs` (new)
- `crates/grus-core/src/drc/` (polygon union/hole extraction as needed)
- `docs/e7-drc.md` (family table — all 50 unique lef_expressible done)
- `docs/results.md` (dated entry)
- `docs/status.md`, `docs/roadmap.md` (progress line)
- `Cargo.toml`/`Cargo.lock` (version bump per surface)
