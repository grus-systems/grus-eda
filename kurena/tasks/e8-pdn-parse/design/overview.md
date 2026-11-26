# e8-pdn-parse — Overview

E8 ticket 2 (reference ticket `e8-irdrop-ref` landed with
`tests/designs/generate_irdrop_ref.sh` + committed goldens). Replaces the
flat `DesignDb::special_wire_rects` with a per-net PDN model parsed from
DEF `SPECIALNETS`, resolves via stacks through real via definitions (DEF
`VIAS` + LEF `VIA`), attaches instance power pins, and runs a
`check_power_grid`-shaped connectivity self-check that agrees with the
committed reference on `gcd.placed.def` and `picorv32.placed.def`.

## Measured input facts (this checkout, 2026-11-23)

- `gcd.placed.def` `SPECIALNETS`: two nets, `VGND` (`+ USE GROUND`) and
  `VPWR` (`+ USE POWER`). Per net: conn groups `( PIN <name> ) ( * VNB )
  ( * VGND )` / `( PIN <name> ) ( * VPB ) ( * VPWR )`, then `+ ROUTED`
  plus `NEW` wire statements: 47 two-point wire segments per net
  (15 `FOLLOWPIN` met1 rails, 1 `STRIPE` met5, 1 `STRIPE` met4,
  15 `DRCFILL` met3, 15 `DRCFILL` met2) and 46 one-point via records
  (`NEW <layer> 0 + SHAPE STRIPE ( x y ) <viaName>`): 15× `via2_3`,
  15× `via3_4`, 15× `via4_5`, 1× `via5_6`.
- `picorv32.placed.def` (gitignored, seeded in the worktree): same
  structure — 615 two-point wires and 792 via records per net
  (261× each of `via2_3`/`via3_4`/`via4_5`, 9× `via5_6`).
- **The SPECIALNETS via names are DEF-defined, not LEF-defined.** Both
  DEFs carry `VIAS 4 ;` generated-via records:
  `- via2_3_1600_480_1_5_320_320 + VIARULE M1M2_PR + CUTSIZE 150 150
  + LAYERS met1 via met2 + CUTSPACING 170 170 + ENCLOSURE 85 165 55 85
  + ROWCOL 1 5 ;`. `+ LAYERS <bot> <cut> <top>` gives the layer triple
  directly; `+ ROWCOL r c` gives the parallel cut count (1×5 = 5 for
  `via2_3`; `via5_6` has no `+ ROWCOL` → 1).
- The sky130 techlef (`sky130_fd_sc_hd__nom.tlef`) defines 25 `VIA`
  blocks (currently skipped, `lef: skipped … (VIA x25 …)`), e.g.
  `VIA L1M1_PR DEFAULT` with three `LAYER`/`RECT` groups in order
  **cut, bottom, top** (`LAYER mcon` → `LAYER li1` → `LAYER met1`).
  The cell LEF defines none.
- Top-level `VPWR`/`VGND` pins are `+ SPECIAL` `PINS` records with two
  `+ LAYER` port rects each (met5 + met4), already stored absolute in
  `db.pin_io_rects`; they are not in `db.io_pins` and not in the
  reader's `io_pin_name_to_id` map.
- Standard-cell `VPWR`/`VGND` `MacroPin.rects` are on `met1` (already
  captured); `VNB`/`VPB` ports are on `pwell`/`nwell` — non-routing
  layers, so `MacroPin.rects` is empty for them and they cannot attach
  to metal. This is exactly why each CSV has one row per instance per
  net.
- `gcd.irdrop.reference.json`: `nets.<N>.connectivity_result == "1"`
  ("All shapes on net VPWR are connected.") and
  `nets.<N>.zeroed_probe.csv_rows == 384`; the committed
  `*.embedded.csv` files each carry 384 data rows + header.
  picorv32: `connectivity_result == "1"`, `zeroed_probe.csv_rows` =
  10,205 per net.
- CSV columns: `Instance,Terminal,Layer,X location,Y location,Voltage`.
  X/Y is the instance-terminal position in µm and matches
  `DesignDb::pin_pos_dbu` (`_411_,VPWR,li1,12.6500,51.6800` ↔ the
  truncated `getAvgXY` center). The `Layer` column does **not** equal
  the pin's PORT layer (`dfxtp_2` `VPWR` is `met1`, CSV prints `li1`) —
  recorded, not asserted (`golden.md`).

## Key decisions

- **`PdnNet` stores every special net** (any `+ USE`), not only
  POWER/GROUND — `special_wire_rects` today feeds gcell blockage from
  all special nets, so keeping them all makes the derived accessor
  bit-identical in coverage.
- **`special_wire_rects` field is removed** and replaced by
  `DesignDb::special_wire_rects()`, an iterator over
  `pdn_nets[..].wire_rects`. One source of truth; its only consumer
  (`congestion/gcell.rs`) is repointed. This is a breaking signature
  change → **minor version bump** (0.6.0 → 0.7.0), explicitly allowed
  by the acceptance.
- **Two-level via resolution** at DEF parse time: DEF `VIAS` `+ LAYERS`
  triples first, then `tech_db.via_defs` (LEF `VIA` blocks). Cut count
  = `+ ROWCOL` product for generated vias, LEF cut-layer `RECT` count
  for fixed vias. Name-pattern heuristics are never consulted
  (`lef-via.md`).
- **Conn groups are stored raw** (`PdnConn` enum) on `PdnNet` and
  expanded in `pdn.rs`, not at parse time — no COMPONENTS/PINS vs.
  SPECIALNETS ordering assumption (`model.md`). `VIAS`-before-
  `SPECIALNETS` is the only ordering assumption; a violation errors
  loudly (`def.md`).
- **Connectivity = union-find** over same-layer wire rects (closed
  intersection) + via point-containment links, plus source nodes from
  the top-level PIN's `pin_io_rects`. `unreachable_pins` = attachable
  conn pins that either touch no wire or land on a non-source
  component — the pin-level check the acceptance asks for; the
  shape-level `check_power_grid` verdict is exposed separately as
  `all_shapes_connected` (`model.md`).
- **Spec correction recorded**: `acceptance.md`'s "= 384 rows total
  over both" is wrong — each embedded CSV carries 384 data rows, so
  the per-net assertion is `attached_terminals(net) == data_rows(csv
  (net))` = 384/384, 768 combined (`golden.md`).

## Files

Implementation is expected to touch:

- `crates/grus-core/src/readers/def.rs` (SPECIALNETS per-net parse, `VIAS` section parse)
- `crates/grus-core/src/readers/lef.rs` (`VIA` statement parse, skip-report test updates)
- `crates/grus-core/src/tech_db.rs` (`CutLayer`, `ViaDef`, lookup maps)
- `crates/grus-core/src/design_db.rs` (`PdnNet`/`PdnVia`/`PdnConn`/`PdnUse`, `pdn_nets`, accessor, `orient_rect` helper)
- `crates/grus-core/src/pdn.rs` (new: conn expansion, pin attach, union-find connectivity, census)
- `crates/grus-core/src/congestion/gcell.rs` (consumer repoint to the accessor)
- `crates/grus-core/src/lib.rs` (`pub mod pdn`)
- `Cargo.toml` (workspace version bump 0.6.0 → 0.7.0)
- `docs/results.md` (run log: census, reference agreement, runtime)
- `docs/roadmap.md` (§E8 progress line)
- `docs/e0-readers.md` (SPECIALNETS bullet kept in step with code)
