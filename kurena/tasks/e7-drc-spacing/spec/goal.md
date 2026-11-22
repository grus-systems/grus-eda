# e7-drc-spacing: same-layer spacing rules on the DRC geometry model

E7 ticket 3, depends on `e7-drc-geom` (layer shapes + rule framework).
Implement the `lef_expressible` **spacing** family: minimum same-layer
spacing and the wide-metal spacing rules, joined 1:1 to
`magic_per_rule` names.

## Scope

1. **Min spacing, same layer, different shapes** — `li.3`, `li.c2`,
   `mcon.2`, `via.2 - 2 * via.4a`, `via2.2 - 2 * via2.4`,
   `via3.2 - 2 * via3.4`, `via4.2 - 2 * via4.4`,
   `met1.2`–`met5.2`. Shapes on the same net merge before spacing is
   evaluated (touching/overlapping primitives union like Magic's
   connectivity; different nets — or same-net-separated — check
   edge-to-edge distance). Record the chosen same-net merge semantics in
   `docs/e7-drc.md` and verify 0 violations on both routed DEFs.
2. **Wide-metal spacing** — `met1.3a`/`met1.3b`, `met2.3a`/`met2.3b`,
   `met3.3c`/`met3.3d`, `met4.5a`/`met4.5b` (">3um" spacing to unrelated
   metal + attached-within-window variants). Thresholds again from LEF
   (`SPACING`/`SPACINGTABLE` if present, else the Magic-deck constant
   recorded as an explicit named constant with provenance).
3. **Rule-name parity** with `magic_rules_checked.classification` as
   established in e7-drc-geom.

## Non-goals

- Enclosure/overlap, min-area, min-hole-area families — next ticket.
- No CLI (lands at `e7-drc-e2e`).

## Files

- `crates/grus-core/src/drc/spacing.rs` (new)
- `crates/grus-core/src/drc.rs`/`drc/geom.rs` (net-merge + index hooks)
- `docs/e7-drc.md` (family status table + merge semantics)
- `docs/results.md` (dated entry)
- `docs/status.md`, `docs/roadmap.md` (progress line)
- `Cargo.toml`/`Cargo.lock` (version bump per surface)

## Evidence hooks already committed

- `generate_drc_ref.sh` sanity control appended a `met2` wire at a
  0.03 µm gap and Magic flagged `Metal2 spacing < 0.14um (met2.2)` —
  reproduce that mutation in our harness and assert our checker flags
  `met2.2` too (the fixture can be constructed programmatically in the
  test rather than re-running the container script).
