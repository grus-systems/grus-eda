# e5-insert-api — Overview

E5 ticket 2: the `DesignDb` netlist-mutation API — the only new
infrastructure the scoping inventory named for E5
(`kurena/tasks/next-stage-scoping/design/inventory.md` §E5: "DesignDb
cell/net insertion — nothing mutates a netlist today"). This is the
primitive layer the CTS ticket (`e5-cts-tree`) builds on: insert buffer
cells, split clock nets into driver/fanout segments, re-emit a legal
DEF. No tree algorithm here.

## What exists

`DesignDb` (`crates/grus-core/src/design_db.rs`, ~250 lines) is
append-once: the three readers (DEF, Verilog, Yosys JSON) each build the
cell/pin/net SoA plus CSR (`cell_pin_offsets`/`cell_pin_ids`,
`net_pin_offsets`/`net_pin_ids`) in one shot. Two invariants they all
maintain:

- **driver-first**: within a net's `net_pin_ids` segment, if any member
  has `pin_is_driver`, segment index 0 is a driver (`net_driver` reads
  `seg[0]`).
- **`pin_nets` back-pointer**: `pin_nets[p] == Some(n)` iff `p` is in
  net `n`'s segment.

`write_def_full` already serializes every cell in `cell_names` and every
net in `net_names` generically, including `+ UNPLACED` components — it
needs **no hook** for inserted cells (spec's conditional; verified by
reading it). `readers/def.rs` is untouched by this ticket.

## Key decisions

- **Same file, no split.** `design_db.rs` is a flat 250-line module;
  the repo splits directories only for multi-file subsystems
  (`placement/`, `congestion/`, `sta/`). The mutators go in the
  existing `impl DesignDb` block (or a second `impl` in the same file);
  tests go in a `#[cfg(test)] mod tests` at the bottom, matching
  `readers/def.rs` and `interner.rs` conventions.
- **`net_pin_ids` stays authoritative; segments edited in place.** A
  full CSR rebuild from `pin_nets` per call was considered and rejected
  at equal complexity: a rebuild must invent intra-net order, and
  ascending-`PinId` order would permute every untouched net's emitted
  connection list, breaking the byte-identity acceptance item. In-place
  `Vec::remove`/`insert` preserves segment order exactly; offsets are
  rebuilt from segment counts after each call. Cost is `O(#net_pins +
  #nets)` per `connect_pin`/`disconnect_pin` — fine at E5 scale (a
  picorv32-size CTS is ~10³ calls × ~10⁵ entries); a batch API can come
  when a profile asks for it. This is the "CSR rebuild policy" the spec
  asks to record; it goes in the methods' rustdoc and `docs/results.md`.
- **`insert_cell` mirrors `add_component`.** Pin dirs/offsets/
  `is_driver`/`macro_pins` come from `tech_db.macro_pins(mac)`;
  `cell_lib_ids` and `pin_lib_pins` are resolved by name against
  `lib_db` exactly as the DEF parser does
  (`crates/grus-core/src/readers/def.rs:429-462`). `db.strings` is a
  clone of `tech_db.strings` in all three readers, so `mpin.name`
  `SymbolId`s are directly valid in `db.strings` — the mutator pushes
  them as-is and documents the assumption.
- **Power pins of inserted cells stay unconnected** (`pin_nets=None`),
  like every existing cell's `VPWR`/`VGND`/`VNB`/`VPB` pins —
  SPECIALNETS are not modeled as nets.
- **No writer/parser change**; `parse_def`'s strictness is itself the
  "DEF is valid" check in the round-trip test.
- **Version**: minor bump `0.3.13 → 0.4.0` per the acceptance checklist
  ("new API"). Note: `AGENTS.md` §5 would call a non-breaking feature a
  patch bump in `0.x` — the acceptance text wins; the discrepancy is
  named in the commit body and `docs/results.md` so the human gate can
  veto.
- **`docs/` surface for the next ticket** is the methods' rustdoc plus
  the `results.md`/`status.md` entries — no new `docs/e5-*.md` file
  (the goal's `## Files` does not name one).

## Files

Implementation is expected to touch:

- `crates/grus-core/src/design_db.rs` (API + `#[cfg(test)] mod tests`)
- `Cargo.toml` (`[workspace.package] version` 0.3.13 → 0.4.0)
- `docs/results.md`
- `docs/status.md` (new E5 row)
