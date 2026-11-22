# e7-drc-geom — `NETS` routed geometry parse

How the `NETS` section is refactored to retain routed geometry, and the
exact statement shapes the committed DEFs exercise.

## Grammar facts from the committed inputs

`tests/designs/gcd/gcd.routed.def`: 269 `NETS` records, all routed.
`tests/designs/picorv32/picorv32.routed.def`: ~6.8k records. Observed
forms:

```
- _12345_ ( u3 A ) ( u7 Y ) + USE SIGNAL
  + ROUTED met2 ( 10350 48450 ) ( * 53210 )
  NEW met1 ( 10305 53210 ) ( 10350 * )
  NEW li1 ( 10350 48450 ) L1M1_PR_MR
  NEW met1 ( 10350 48450 ) M1M2_PR
  ;
- _99_ ( u1 A ) + USE SIGNAL
  NEW met3 ( 94530 245140 ) RECT ( -620 -150 0 150 )
  ;
```

- Conn groups `( comp pin )` / `( PIN name )` precede all attributes.
- `+ ROUTED layer pt+` / `NEW layer pt+` open a wire statement on
  `layer`; **`NEW` is bare, not `+ NEW`** (mirroring SPECIALNETS).
- Paths chain points: every `( x y )` after the first is a segment
  endpoint; `*` repeats the same axis' previous coordinate. Observed
  scope is **statement-local** (`( x1 y1 ) ( x2 * )` — no committed
  file starts a `NEW` statement with a `*` point); `WireSink` keeps
  record-global `last_x/last_y` anyway so behavior is identical to
  special wires if a first-point `*` ever appears.
- A via name after a point terminates the path (`pt <viaName>`).
- `pt RECT ( dx1 dy1 dx2 dy2 )` emits an offset rect relative to the
  preceding point (negative deltas prove it is relative, not absolute).
- Via names referenced: `L1M1_PR_MR`, `M1M2_PR`, `M2M3_PR`, `M3M4_PR`,
  `M4M5_PR` — all are **LEF `VIA` names** (no `VIAS`-section record
  defines them); the 4 `VIAS` records are `+ VIARULE` generated vias
  used by SPECIALNETS-style tools and resolved through `def_vias` first.
- `+ FIXED`/`+ COVER` appear on some nets (picorv32 uses `FIXED`);
  `TAPER`/`EXT`/`VIRTUAL`/`SHIELD`/`SUBNET` do not occur in either file
  but `SHIELD`/`VIRTUAL` are grammar-legal and cheap to support.

## Refactor: section loop

Current code (`def.rs` ~1265–1380) streams tokens per record: the name
and `net_def_spellings` into `net_names_vec`/`net_spellings_vec`, conn
groups via `read_conn_group` on the live lexer into `net_pins_list`,
then `in_attributes` skips the rest verbatim. **After** the section the
code builds the net SoA: `net_names`, `net_def_spellings`, and CSR
`net_pin_offsets`/`net_pin_ids` with the driver pin swapped first.

Keep the deferred build verbatim; only the per-record body changes:

```rust
"NETS" => {
    let declared = section_count(&mut lexer, "NETS")?;
    for_each_record(&mut lexer, "NETS", declared, &mut words, |words, line| {
        // returns (name, spelling, pins, NetGeom) pushed onto the
        // parallel lists net_names_vec / net_spellings_vec /
        // net_pins_list / net_geoms_list
        add_net(db, tech_db, &def_vias, words, line, ...)
    })?;
    // existing SoA build unchanged + `db.net_geoms = net_geoms_list`
}
```

`read_statement_into` drops parens, so conn groups arrive as 2-word
pairs — identical to `add_special_net`'s conn scan. Consequences:

- Conn-pair scan stops at `+`, `NEW`, or `ROUTED` (same boundary set as
  `add_special_net`; component names colliding with keywords are already
  excluded by DEF's reserved-word rule).
- Each conn pair resolves exactly like today: `PIN name` →
  `io_pin_name_to_id` + `pin_nets[pid]`; `comp pin` →
  `cell_name_to_id` + macro-pin lookup by name — the lookup bodies move
  into `add_net` unchanged (they are inline in the current loop, not a
  helper). Driver-first ordering is **not** done at parse — it stays in
  the section-end CSR build where `pin_is_driver` is already set.
- Malformed conns keep the pinned error text —
  `test_connection_group_with_extra_word_is_an_error` and
  `..._one_word_...` assert ``must be `( component pin )` ``; emit the
  identical message from the pair-scan path. The unterminated-group case
  (`( u1 ;`) dies inside `read_statement_into` with its own message —
  adjust the test expectation only if the existing text changes.
- `net_geoms_list` is pushed once per record (empty `NetGeom` for
  unrouted nets) so it stays parallel with `net_names_vec`; the
  section-end block assigns it to `db.net_geoms` alongside the CSR
  build, satisfying I7 for DEF inputs.

## `add_net` record walk

```
add_net(words, line):
  [name, rest @ ..] → net_name = unescape(name);
      push name → net_names_vec, raw spelling → net_spellings_vec
  conn phase: for pairs while rest[i] ∉ {"+", "NEW", "ROUTED"}:
      (comp pin) → resolve via io_pin_name_to_id / cell_name_to_id +
                   macro-pin name match (current loop's bodies, verbatim);
                   append PinId to current_pins → net_pins_list;
                   db.pin_nets[pid] = net_id
      odd/leftover word → bail "malformed connection ... must be `( component pin )`"
  then dispatch loop (mirrors add_special_net):
    "+"  → keyword dispatch:
        "USE"      → validate PdnUse set (SIGNAL/CLOCK/etc.; unknown → bail)
        "ROUTED"|"FIXED"|"COVER"|"PLACED" → parse_net_wire(rest[i+2..])
        "SHIELD"   → consume the shield-net name only (a net attribute
                     per DEF grammar; any shielding wires arrive as
                     their own NEW/+ ROUTED statements — the special-net
                     wire-body variant does not occur in NETS)
        "SUBNET"   → bail (geometry-aliased shape, never emitted by DRT)
        "EXT"|"DESIGNRULEWIDTH"|"NONDEFAULTRULE" → bail (width-altering)
        "TAPER"|"TAPERWIDTH"|"STYLE"|"MASK"|"SYNTHESIZED"|"PATTERN"|
        "SOURCE"|"ESTCAP"|"WEIGHT"|"ORIGINAL"|"PROPERTY"|"XTALK"|
        "VOLTAGE"|"FREQUENCY"|"FIXEDBUMP" → consume args, no geometry effect
        other      → bail "unexpected"
    "NEW"    → parse_net_wire(rest[i+1..])
    "ROUTED" bare → parse_net_wire(rest[i+1..])  (defensive; not emitted)
    else     → bail
  push NetGeom { wire_rects, vias } into net_geoms_list
```

`+ USE` reuses `parse_pdn_use`. `SIGNAL`/`CLOCK`/`ANALOG`/`RESET`/
`SCAN`/`TIEOFF` are the grammar-legal NETS uses; `POWER`/`GROUND` in
NETS are grammar-illegal but `parse_pdn_use` accepts them — keep the
shared parser (leniency already established for SPECIALNETS).

## `parse_net_wire` — the wire body

Regular-net paths carry **no width token** — width is the LEF layer
`WIDTH`. Signature mirrors `parse_special_wire` but takes the statement
slice and a `&mut WireSink` (shared `last_x/last_y` for `*`):

```
layer_name → lid via tech_db.layer_by_name (unknown → bail)
modifiers (loop, before first point):
    "TAPER"            → consume, no effect (full-width taper is default)
    "TAPERWIDTH"/"STYLE"/"MASK" → consume 1 arg, validate number/name
    "EXT"/"DESIGNRULEWIDTH"/"NONDEFAULTRULE" → bail (change shape/width)
element loop until "+", "NEW", "ROUTED", or end:
    "RECT"    → read 4 numbers (dx1 dy1 dx2 dy2) relative to prev point;
                requires a preceding point in THIS statement → bail else;
                rect = point + normalized delta → push (lid, rect)
    "VIRTUAL" → consume point, update last_x/last_y, emits nothing
    coord pair (is_coord) → resolve `*` against sink.last_*, update sink;
                if this statement already has a prev point → emit segment
                (prev → cur) half-expanded by routing_layers[lid].width_dbu;
                else it is the path start
    else      → word is a via name → resolve_def_via → push PdnVia
                (lid ∈ {lower, upper} check same as special wires);
                next word must be "+"/"NEW"/"ROUTED"/end → bail else
statement end with no second point and no via/RECT → bail "one-point
wire stub" (same rule as special nets)
```

Half-width expansion **copies `parse_special_wire` verbatim**, with
`width = routing_layers[lid].width_dbu` in place of the statement's
width field: horizontal `y1==y2` → `(min_x, y-half, max_x, y+half)`;
vertical `x1==x2` → `(x-half, min_y, x+half, max_y)` — **flush ends,
perpendicular expansion only** (DEF paths are FLUSH-capped; the wire
does not extend past its endpoints). Diagonal segments get the same
full-bbox expansion special wires already use (`min-half .. max+half`
on both axes) — not a bail; mirror, don't "fix". `RECT` stubs emit
their offset rect un-expanded (the delta already encodes the shape).

Determinism: rects append in statement order inside the record; nets
append in `NETS` order → `net_geoms` order is fully input-determined.

## `VIAS` record param retention (same file, related change)

`add_def_via` already validates `CUTSIZE`/`CUTSPACING`/`ENCLOSURE`/
`ROWCOL` numbers and drops them. Now retain into `DefVia::Generated`'s
new `geom: GeneratedViaGeom` (see `model.md`):

- `CUTSIZE w h` — required for a `+ VIARULE` via → bail when absent.
  This is an explicit **unsupported-input error**, not a lookup: the
  geometry model cannot derive cut size from `+ VIARULE` (we do not
  model via-rule widths), and every committed DEF supplies it. A
  negative unit test asserts the bail.
- `ROWCOL r c` — default `1 1`; `cut_count = r*c` (existing behavior).
- `CUTSPACING x y` — default `0 0` (edge-to-edge).
- `ENCLOSURE a b c d` — **OpenDB semantics: `(botX, botY, topX, topY)`
  symmetric overhangs** — verified against
  `via2_3_1600_480_1_5_320_320` (CUTSIZE 150, ROWCOL 1 5, CUTSPACING
  170 → cut array 1430×150; `ENCLOSURE 85 165 55 85` → lower metal
  1430+2·85=1600 × 150+2·165=480 = the `1600_480` in the via's name;
  upper = 1540×320). Default `0 0 0 0`. Repeated attr → last wins.

## Error-message rules

Every bail carries `line` + net name (`resolve_def_via` already takes
`net_name` for exactly this). New call sites for `resolve_def_via` from
`NETS` reuse it verbatim — its error text mentions "SPECIALNETS net";
generalize the message to "net" in the same commit (SPECIALNETS tests
pin only the unknown-name substring — verify before editing).

## Files

- `crates/grus-core/src/readers/def.rs`
- `crates/grus-core/src/design_db.rs`
