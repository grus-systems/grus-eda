# Goal

E5 ticket 2: the `DesignDb` netlist-mutation API — the
only new infrastructure the scoping inventory named
(`next-stage-scoping/design/inventory.md` §E5: "DesignDb
cell/net insertion — nothing mutates a netlist today —
the same mutation API any future buffering/repair step
needs").

Today `DesignDb` is append-once at read time: cell/pin/
net SoA with CSR offsets (`cell_pin_offsets`,
`net_pin_offsets` with driver-first ordering). CTS needs
to insert buffer cells, split clock nets into
driver/fanout segments, and re-emit a legal DEF.

## Work

- `insert_cell(&mut self, name, macro_id/lib_cell_id) ->
  CellId`: appends the cell row + one `PinId` per macro
  pin (dirs/lib ids from `LibDb`), `PlacementStatus::
  Unplaced`, consistent SoA lengths.
- `create_net(&mut self, name) -> NetId`.
- `connect_pin(&mut self, pin: PinId, net: NetId)` /
  `disconnect_pin`: move a pin between nets keeping the
  **driver-first invariant** (`net_pin_offsets` layout),
  `pin_nets` back-pointer, and CSR consistent. Document
  cost (CSR rebuild per call is fine at E5 scale —
  record the choice, don't over-engineer).
- `insert_buffer_on_net(&mut self, net, buffer_macro,
  name, new_net_name) -> (CellId, NetId)`: the CTS-shaped
  convenience — driver keeps the original net, all sink
  pins move to the new net driven by the buffer.
- Round-trip proof: insert a buffer on a gcd clock net,
  place it via the existing legalizer,
  `write_def_full`, re-read the DEF — components/nets/
  pins must match the mutated in-memory design and
  `parse_def` must accept it.
- Invariants asserted in tests: driver-first ordering,
  `pin_nets` == net membership both ways, `io_pins`
  untouched, HPWL/parasitics unaffected on untouched
  nets.

## Done when

The API exists, is exercised by a gcd-scale test
(insert N buffers on `clk`, legalize, write+re-read
DEF), and `docs/` records the surface for the next
ticket (e5-cts-tree). No clock-tree algorithm in this
ticket.

## Files

- `crates/grus-core/src/design_db.rs` (or a new
  `design_db/mutation.rs` if the file wants splitting —
  match existing module conventions)
- `crates/grus-core/src/readers/def.rs` only if
  `write_def_full` needs a hook for inserted cells
- `docs/results.md`, `docs/status.md` (E5 row)
- `Cargo.toml` (minor bump — new API surface)
