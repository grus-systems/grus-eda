# e5-insert-api — Mutation API

All methods live on `impl DesignDb` in
`crates/grus-core/src/design_db.rs`. `connect_pin`/`disconnect_pin` are
infallible index ops (an out-of-range `PinId`/`NetId` is a caller bug
and panics, same as `net_pins`/`cell_pins` today). The three
name/macro-boundary methods return `anyhow::Result` with a message that
names the offending name/id.

## Invariants (to document in rustdoc on each method)

- **I1 — SoA lengths.** All seven `cell_*` columns have length
  `num_cells()`; all thirteen `pin_*` columns (including
  `pin_io_rects`) have length `num_pins()`. `cell_pin_offsets` and
  `net_pin_offsets` have length `num_cells()+1`/`num_nets()+1`, are
  nondecreasing, start at 0, and end at `cell_pin_ids.len()`/
  `net_pin_ids.len()`.
- **I2 — `cell_pin_ids` partition.** Every instance pin appears in
  exactly one cell's range; `pin_cells[p] == Some(c)` iff `p` lies in
  `cell`'s range.
- **I3 — `net_pin_ids` partition + `pin_nets` back-pointer.** Every
  connected pin appears in exactly one net's segment; `pin_nets[p] ==
  Some(n)` iff `p` lies in `n`'s segment. `pin_nets[p] == None` iff `p`
  is in no segment.
- **I4 — driver-first.** If any pin in a net's segment has
  `pin_is_driver`, segment index 0 is a driver (i.e.
  `net_driver(net) == Some(seg[0])`).
- **I5 — `io_pins`.** Exactly the pins with `pin_cells == None`, in
  `PinId` order; the mutators never touch it (inserted cells produce
  only instance pins).
- **I6 — `net_def_spellings`.** `len() ∈ {0, num_nets()}`. The
  `guide.rs` emitter uses per-net spellings only when the lengths match
  (`congestion/guide.rs:469`); both states stay valid.

## Surface

```rust
impl DesignDb {
    /// Append an instance of `macro_id` named `name`, unplaced at (0,0) N.
    /// Pins are created one per `tech_db.macro_pins(macro)`; `pin_lib_pins`
    /// are resolved by pin name against the same-named `lib_db` cell, the
    /// `add_component` rule. Caller seeds `cell_x_dbu`/`cell_y_dbu` before
    /// legalization.
    pub fn insert_cell(
        &mut self,
        name: &str,
        macro_id: MacroId,
        tech_db: &TechDb,
        lib_db: Option<&LibDb>,
    ) -> Result<CellId>;

    /// Append an empty net named `name`.
    pub fn create_net(&mut self, name: &str) -> Result<NetId>;

    /// Move `pin` onto `net`. No-op if already there. A driver pin lands
    /// at segment index 0; a sink appends at the segment end.
    /// O(#net_pins + #nets) — see "Cost" below.
    pub fn connect_pin(&mut self, pin: PinId, net: NetId);

    /// Remove `pin` from its net, if any. Re-normalizes driver-first in
    /// the shrunk segment: if seg[0] is no longer a driver but a later
    /// member is, it moves to index 0.
    pub fn disconnect_pin(&mut self, pin: PinId);

    /// CTS-shaped convenience: `name` becomes a buffer instance whose
    /// input joins `net` and whose output drives a new net
    /// `new_net_name` carrying every former sink of `net` (in order).
    /// `net` keeps its driver. Returns the inserted cell and the new net.
    pub fn insert_buffer_on_net(
        &mut self,
        net: NetId,
        buffer_macro: MacroId,
        name: &str,
        new_net_name: &str,
        tech_db: &TechDb,
        lib_db: Option<&LibDb>,
    ) -> Result<(CellId, NetId)>;
}
```

### `insert_cell`

- Bail on a duplicate `cell_names` entry (linear scan, resolved through
  `db.strings` — `O(num_cells)`; a `cell_by_name` map is deferred, see
  "Rejected") and on `macro_id` out of range.
- Push one entry on every `cell_*` column: interned name, `macro_id`,
  `cell_lib_ids` = `lib_db.cell_by_name` lookup of the macro name (the
  `add_component` rule — `None` when `lib_db` is `None` or the cell is
  not in the Liberty), `Unplaced`, `x = y = 0`, `Orient::N`.
- For each `mpin` in `tech_db.macro_pins(mac)` push one pin row:
  `pin_names = mpin.name` (valid in `db.strings` because every reader
  sets `db.strings = tech_db.strings.clone()` — state this in rustdoc),
  `pin_dirs = mpin.dir`, `pin_cells = Some(cell)`, `pin_macro_pins =
  Some(MacroPinId(mac.pin_offset + i))`, `pin_lib_pins` = name match
  inside `lib.cell_pins(lcell)` (`LibPinId(lcell.pin_offset + idx)`, as
  `readers/def.rs:451-461`), `pin_nets = None`, `pin_offsets =
  mpin.offset_*_dbu`, `pin_io_layer = None`, `pin_io_rects = vec![]`,
  `pin_is_driver = (mpin.dir == PinDir::Output)`.
- Append each new `PinId` to `cell_pin_ids`, then push
  `cell_pin_ids.len()` onto `cell_pin_offsets` (I1).

### `create_net`

- Bail on a duplicate `net_names` entry (same linear scan).
- Push `intern(name)` to `net_names`; if `net_def_spellings.len() ==
  num_nets()` (pre-push), push `intern(&escape_def_name(name))` so a
  DEF-sourced design keeps I6's matching-length state — the emitted
  guide token then equals what `write_def_full` will write.
  (`escape_def_name` is `crate::readers::def::escape_def_name`.)
- Push `net_pin_ids.len()` onto `net_pin_offsets` — the new net's
  segment is empty (I1).

### `connect_pin` / `disconnect_pin`

- `connect_pin`: return early when `pin_nets[p] == Some(net)`. Otherwise
  `disconnect_pin(pin)` first, then insert into `net`'s segment — index
  `net_pin_offsets[n]` when `pin_is_driver[p]` (I4), else
  `net_pin_offsets[n+1]` — and set `pin_nets[p]`.
- `disconnect_pin`: no-op on `pin_nets[p] == None`. `Vec::remove` the
  pin from its segment, set `pin_nets[p] = None`, then apply the
  readers' normalization to the shrunk segment (`position(is_driver)` →
  `swap(0, pos)` when seg[0] is not a driver — same code shape as
  `readers/def.rs:946-950`).
- After the flat edit, rebuild `net_pin_offsets`: segment lengths from
  the previous offsets, ±1 for the moved pin, prefix-sum. A private
  `fn rebuild_net_pin_offsets(&mut self, counts: &[u32])` keeps this in
  one place. **Ordering pitfall**: the target insert index must be
  computed from offsets *after* the removal rebuild (or with the shift
  applied) — doing removal and insertion against one stale offset table
  corrupts the CSR when `old_net < new_net`.
- Multi-driver semantics, documented: a driver connected to an
  already-driven net lands at index 0 and becomes `net_driver`'s
  answer. Single-driver nets are all this ticket needs (STA counts
  drivers elsewhere).

### `insert_buffer_on_net`

- Bail when `net_driver(net)` is `None` (nothing to anchor the split)
  or when `buffer_macro` does not have exactly one `!is_power &&
  dir == Input` pin and one `!is_power && dir == Output` pin. The
  `is_power` test is load-bearing: `clkbuf_*` power pins are
  `DIRECTION INOUT` in LEF and would pass a dir-only filter.
- Sequence: `insert_cell` → `create_net` → `connect_pin(buf_out,
  new_net)` (driver → index 0) → `connect_pin(buf_in, net)` → for each
  pin in a **snapshot** of `net`'s segment except the driver and
  `buf_in` (in segment order), `connect_pin(p, new_net)`. Result:
  `net` = `{driver, buf_in}`, `new_net` = `{buf_out, old sinks…}`.
  Snapshots are required — iterating a segment while mutating it is the
  classic aliasing bug; `net_pins` borrows `&self`.
- Returns `(cell_id, new_net_id)`.

## Cost (recorded choice)

Each `connect_pin`/`disconnect_pin` performs one `Vec::remove` or
`insert` in `net_pin_ids` plus a full `net_pin_offsets` rebuild:
`O(total net-pin entries + num_nets)` per call. At E5 scale (TritonCTS
inserted 8 cells on gcd, 253 on picorv32; each `insert_buffer_on_net`
costs `fanout + 2` calls) this is trivial. A batch move (e.g.
`move_pins(&[PinId], NetId)` rebuilding the CSR once) is the upgrade
path if CTS profiling shows the per-call rebuild in a hot loop — say so
in rustdoc, don't build it now.

## Rejected / deferred

- **Whole-CSR rebuild from `pin_nets` per call**: equal complexity,
  loses intra-net order → untouched nets re-emit differently. Rejected
  (see `overview.md`).
- **`cell_by_name`/`net_by_name` maps on `DesignDb`**: nothing else
  needs name lookup at speed; tests and CTS can scan. Deferred.
- **Batch move API / connectivity diff structure**: over-engineering at
  this scale (spec says record, don't over-engineer).
- **Keeping a `Vec<Vec<PinId>>` shadow membership**: two sources of
  truth for the same invariant; the flat CSR plus `pin_nets` already
  defines membership exactly.

## Edge cases worth a rustdoc line

- `connect_pin` works on I/O pins (`pin_cells == None`) unchanged —
  gcd's `clk` net driver is the `clk` port and stays so after a buffer
  insert; `write_def_full` re-emits it as `( PIN clk )`.
- `insert_buffer_on_net` on a net whose only member is a driver
  produces `new_net = {buf_out}` and `net = {driver, buf_in}` — legal
  output, no special case.
- An empty net created by `create_net` emits `- name + USE SIGNAL ;`
  and `parse_def` reads it back with an empty segment.
- `lib_db = None` is legal (the DEF reader accepts it): `cell_lib_ids`
  and `pin_lib_pins` are `None` — inserted cells then carry no Liberty
  ids and are invisible to STA, which is the caller's choice to make.
