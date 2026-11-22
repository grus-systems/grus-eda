# e8-pdn-parse — DEF `VIAS` + `SPECIALNETS` parsing

Two sections change in `readers/def.rs`: `VIAS` moves from
`skip_section` to a real (small) record parse, and `add_special_net`
grows into a per-net parse that emits `PdnNet`s.

## `VIAS` records

Shape (all test DEFs: 4 generated records):

```
VIAS 4 ;
    - via2_3_1600_480_1_5_320_320 + VIARULE M1M2_PR + CUTSIZE 150 150
        + LAYERS met1 via met2 + CUTSPACING 170 170
        + ENCLOSURE 85 165 55 85 + ROWCOL 1 5 ;
END VIAS
```

Records go through `for_each_record(&mut lexer, "VIAS", …)` (count
check comes free) and `split_attributes` on `words[1..]`:

| attribute | handling |
|---|---|
| `+ VIARULE <name>` | marks the record generated; the rule name is stored only for diagnostics — `+ LAYERS` is authoritative |
| `+ LAYERS <bot> <cut> <top>` | `bot`/`top` resolve through `tech_db.layer_by_name` (unknown → error); `cut` resolves through `tech_db.cut_layer_by_name` (unknown or routing name → error) |
| `+ ROWCOL <r> <c>` | parallel cut count = `r * c`; absent → 1 |
| `+ CUTSIZE <x> <y>`, `+ CUTSPACING <x> <y>`, `+ ENCLOSURE <a> <b> <c> <d>` | numbers validated, geometry not stored |
| `+ VIA <lefViaName>` | fixed form — resolves through `tech_db.via_defs` at use |
| `+ RECT`, `+ POLYGON`, `+ PATTERN`, `+ MASK`, `+ FILL_SHAPE`, `+ STYLE`, `+ PROPERTY` | named skip list (their args not modelled) |
| anything else | `bail!` — strict reader convention |

Result type held in a parse-local map `FastMap<String, DefVia>` keyed
by the DEF-spelled via name:

```rust
enum DefVia {
    /// `+ VIARULE` + `+ LAYERS` triple, fully resolved.
    Generated { lower: LayerId, upper: LayerId, cut: SymbolId, cut_count: u32 },
    /// `+ VIA <name>` — resolve `tech_db.via_defs` at use.
    Fixed(SymbolId),
    /// No resolvable layer stack (`+ RECT`/`+ POLYGON` inline via).
    /// Tolerated when defined; errors only when SPECIALNETS references it.
    Inline,
}
```

A `+ VIARULE` record missing `+ LAYERS` is an error (the grammar
requires it).

## `SPECIALNETS` records

Shape (gcd; parens already dropped by `read_statement_into`, so the
word stream is `[name, "PIN", pn, "*", m1, "*", m2, "+", "USE", k,
"+", "ROUTED", layer, w, "+", "SHAPE", s, x, y, x, y, "NEW", …]`):

```
- VGND ( PIN VGND ) ( * VNB ) ( * VGND ) + USE GROUND
  + ROUTED met1 480 + SHAPE FOLLOWPIN ( 5520 87040 ) ( 86020 87040 )
  NEW met1 480 + SHAPE FOLLOWPIN ( 5520 81600 ) ( 86020 81600 )
  …
  NEW met4 0 + SHAPE STRIPE ( 25140 30830 ) via5_6_1600_1600_1_1_1600_1600 ;
```

### Conn groups

After `words[0]` (net name, unescaped + interned), conn groups are
two-word pairs up to the first `"+"`/`"ROUTED"`/`"NEW"` word — every
group is exactly `( comp pin )`, and `PIN` in comp position is the
bterm form. Stored verbatim as `PdnConn`:

- `( PIN <name> )` → `PdnConn::TopPin(sym)` — voltage source, resolved
  in `pdn.rs` (see `model.md`).
- `( * <pin> )` → `PdnConn::AllInstances(sym)`.
- `( <inst> <pin> )` → `PdnConn::Instance { inst, pin }`.

A trailing odd word, or any of the group words being `+`, is a
malformed record → `bail!`.

### `+` attributes outside wire statements

- `+ USE <kw>` → `PdnUse`; enum covers the DEF grammar
  (`POWER`/`GROUND`/`SIGNAL`/`CLOCK`/`ANALOG`/`RESET`/`SCAN`/`TIEOFF`/`RING`),
  unknown keyword → `bail!`. Absent → `use_kind: None`.
- Named skip list (args skipped, not modelled):
  `SOURCE`/`PATTERN`/`ESTCAP`/`WEIGHT`/`ORIGINAL`/`FIXEDBUMP`/
  `FREQUENCY`/`VOLTAGE`/`PROPERTY`/`SYNTHESIZED`/`NONDEFAULTRULE`/
  `SHIELD`/`XTALK`. `SUBNET` is **not** skipped — subnets carry their
  own wires and cannot be modelled by this schema → `bail!`.
- Anything else → `bail!`.

### Wire statements

A statement starts on `ROUTED` (after `+`) or bare `NEW`. Body:
`<layerName> <width>` then a point list until the next
`NEW`/`ROUTED`/end of record:

- `+ SHAPE <word>` — skipped (shape keywords are not validated; we do
  not model them).
- `+ MASK` / `+ STYLE` / `+ VOLTAGE` / other `+` attrs inside a wire
  statement — named skip list; unknown → `bail!`.
- Point: `x y` where each coordinate is a number or `*` (repeat of the
  same axis' previous value, same as today). Then:
  - two more coordinates `x2 y2` → **wire segment**; a `RectDbu` is
    pushed to `PdnNet::wire_rects` only when `width > 0` (same rule as
    today: half-width expansion on the shared axis for Manhattan
    segments; `width == 0` two-point segments parse but record
    nothing).
  - a word that is not `+`, `NEW`, `ROUTED`, or a number → **via
    record** at `(x, y)`: resolve the name through the DEF `VIAS` map
    first, then `tech_db.via_defs`; emit a `PdnVia` on the record's
    layer. After a via name the point list ends — the next word must
    be `NEW`/`ROUTED`/`+`/end, else `bail!`.
  - `NEW`/`ROUTED`/`+`/end directly after `x y` → one-point stub is a
    grammar violation here → `bail!` (vias must name their via).

Via-record layer check: the record's `<layerName>` must equal either
`PdnVia::lower` or `PdnVia::upper` — pdngen writes it on the lower
layer, but the grammar pins the point to "the layer the wire is on",
so accepting either endpoint covers writer variation while still
rejecting typos. Mismatch → `bail!`.

### Unknown via names

A SPECIALNETS via name resolving through neither DEF `VIAS` nor LEF
`VIA` → `bail!("DEF line N: SPECIALNETS net X uses via Y, which
neither the VIAS section nor the LEF defines")`. `DefVia::Inline`
references → `bail!("… via Y defines no resolvable layer stack")`.

### Ordering assumption

`VIAS` is parsed when reached into the parse-local map; `SPECIALNETS`
records resolve eagerly. A DEF placing `SPECIALNETS` before `VIAS`
fails on the unknown-name bail — both committed designs emit `VIAS`
first (as does every OpenROAD writer we consume), so this is a named
assumption, not a gap to paper over.

### Duplicate net names

If a `-` record repeats a net name already in `pdn_nets`, records
merge (wires/vias/conns append). A conflicting `+ USE` between the
records → `bail!`.

## What this removes

- `DefToken::Word("VIAS") => skip_section` becomes
  `for_each_record("VIAS")` + `add_def_via`.
- `add_special_net`'s flat number-probe scan is replaced by the
  structured parse above; its `width > 0` rect rule, `*` coordinate
  repeat, and rect construction survive verbatim into `PdnNet`.
- The `width == 0` records stop being silently dropped — they are the
  via-stack records this ticket exists for.

## Files

- `crates/grus-core/src/readers/def.rs`
- `crates/grus-core/src/design_db.rs` (Pdn*/DefVia-adjacent types)
