# e6-drt-emit — Emitted DEF model

**Landed at WIP `dc8cdb4`** — the grammar below is what
`write_routed_def`/`nets_section` ship. The exact text it produces,
why it re-parses through `parse_net_wire`, and why a second write
is byte-identical.

## NETS splice

`write_def` already demonstrates the splice pattern for
`COMPONENTS`: locate the section header line and its `END`, copy
everything else verbatim. Same mechanism here:

- Find the line matching `NETS <n> ;` … the matching `END NETS`.
- Emit `NETS <db.num_nets()> ;`, one canonical record per net in
  `db` order, then `END NETS`.
- `n` is unchanged — every guided net is emitted (unrouted nets get
  a conns-only record), and parse adds nets in record order so
  template NETS order == `db` net order.

Everything outside the section — header, `UNITS DISTANCE MICRONS`,
`DIEAREA`, `ROW`, `TRACKS`, `GCELLGRID`, `COMPONENTS`, `PINS`,
`VIAS`, `NONDEFAULTRULES`, `BLOCKAGES`, `SPECIALNETS` (the PDN
stripes), `FILLS` — passes through byte-for-byte. That is what makes
`resolve_def_via` keep working for DEF-generated vias and keeps the
full PDN in the self-DRC picture.

## Record grammar

One net record, TritonRoute's statement order:

```
 - <def_spelling> ( <comp> <pin> ) …
   + ROUTED <layer> ( x1 y1 ) ( x2 y2 )
   NEW <layer> ( x1 y1 ) ( x2 y2 )
   …
   NEW <lower_layer> ( x y ) <via_name>
   …
   + USE SIGNAL ;
```

- **Name**: `guide_net_name(db, net)` — the raw DEF spelling when
  `net_def_spellings` is populated, else the escaped `net_names`
  entry.
- **Conns**: every `net_pin_ids` member — `( comp pin )` for cell
  pins, `( PIN <name> )` for top-level ports — in stored order
  (driver first). Matches what the placed template already lists.
- **Geometry statements**: exactly one `+ ROUTED` — the first
  emitted statement, whatever its kind — then `NEW` for each
  remaining wire and each via. A wire emits its two endpoints; a via
  emits `NEW <lower_layer_name> ( x y ) <via_name>` (the layer in a
  standalone via record is the lower metal — confirmed by
  TritonRoute's `NEW met2 0 ( 53210 53210 ) via2_3` where the via
  stacks met2→met3).
- **`+ USE SIGNAL ;`** terminates every record. All `NETS` nets are
  signal nets; `PdnUse::Signal` is what the template carries.
- **No `NEW <layer> <width>` form, no `+ ROUTED <layer> <width>`** —
  regular-net wires are zero-width (default-routing-width implied);
  see overview decision 2. No bends: each wire record is 2 points.
  No `EXT`/`DESIGNRULEWIDTH`/`VIAINPIN`/`RECT` modifiers — our
  parser rejects or ignores them anyway.

Layer names are `tech.routing_layers[l].name` resolved via
`db.strings` — plain tokens (`li1`, `met1`…), no escaping needed.

## Ordering (the determinism rules)

- Nets: `db` order (== template NETS order on DEF input).
- Wires within a net: `nr.wires` order — already sorted by
  `WireSeg`'s derived `Ord` (`layer, x1, y1, x2, y2`) inside
  `commit_route`.
- Vias within a net: `nr.vias` order — sorted by `ViaPlace`'s
  `Ord` (`lower, x, y`).
- Segments are emitted as stored — `WireSeg` canonical order keeps
  `(x1,y1) ≤ (x2,y2)` lexicographically, so horizontal wires run
  left→right and vertical wires bottom→top.

`apply_routes` pushes `wire_rects`/`vias` into `NetGeom` in exactly
this order, so the emitted statement sequence equals the `net_geoms`
vector sequence.

## Byte-stability argument

`write(parse(write(db))) == write(db)` byte-exact:

1. Non-NETS bytes pass through verbatim both times — identical by
   construction.
2. NETS is regenerated from `net_geoms` identically both times:
   - net order: `db` order is preserved by `parse_def` (nets append
     in record order);
   - conn order: `net_pin_ids` is preserved (parse appends members
     in record order, driver-first move is idempotent);
   - wire order: emit writes `wire_rects` in vector order; parse
     appends rects in statement order → same sequence;
   - wire coordinates: emit writes `(x1 y1)(x2 y2)`; parse expands
     ±w/2 square-cap; re-emit derives the centerline `(xl+w2, yl+w2)
     →(xh−w2, yh−w2)` — recovers `(x1,y1),(x2,y2)` exactly (below);
   - via order/coords: same argument — emit writes `vias` in order,
     `( x y ) name` round-trips through `resolve_def_via` back to
     the identical `PdnVia` fields (name sym may differ numerically
     across parses but `resolve`d text is identical — `PdnVia` has
     no `PartialEq` concern for the *text* comparison; for the
     `net_geoms` equality assert see tests.md).
3. Formatting is a pure function of `db`/`tech` — identical
   whitespace both passes.

### Centerline recovery

`emit` derives a segment from each `wire_rect`:

```
h-wire: yl + w2 == yh − w2  →  (xl + w2, yl + w2) – (xh − w2, yh − w2)
v-wire: xl + w2 == xh − w2  →  (xl + w2, yl + w2) – (xh − w2, yh − w2)
point : both equal          →  (x, y) – (x, y)
else    → bail: "net <n>: wire_rect <rect> not expressible as a
        zero-width 2-point record"
```

The `else` arm fires only on foreign geometry (a RECT-stub or
non-square-cap rect in a re-routed template) — never in the
apply_routes→write path, where every rect is `seg ± w2` by
construction.

### Flush-cap stub note

TritonRoute emits `NEW met2 0 ( x y )` point stubs (flush caps at
width 0). We emit none — our grammar uses only 2-point records —
but the parser accepts them if they ever appear in a template,
mapping a zero-width point to the same `seg ± w2` rect. The point
case in the table above covers the symmetric situation.

## Example

RouteDb for `_059_`: wires `met1 (24140,30210)-(24140,30730)`,
`met2 (24140,30730)-(25960,30730)`; via `via` (met1→met2 cut) at
(24140,30730) `lower=met1`. Emitted:

```
 - _059_ ( _358_ A ) ( _360_ X )
   + ROUTED met1 ( 24140 30210 ) ( 24140 30730 )
   NEW met2 ( 24140 30730 ) ( 25960 30730 )
   NEW met1 ( 24140 30730 ) M1M2_PR
   + USE SIGNAL ;
```

`M1M2_PR` is the `ViaTable` default name for the met1→met2 pair;
the record's layer token is the via's **lower** metal.

## Files

- `crates/grus-core/src/drt/emit.rs`
- `crates/grus-core/src/readers/def.rs` (reference only — splice
  mirrors `write_def`; no changes expected)
