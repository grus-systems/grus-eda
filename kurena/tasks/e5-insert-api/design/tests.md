# e5-insert-api — Tests

All tests live in `#[cfg(test)] mod tests` at the bottom of
`crates/grus-core/src/design_db.rs` (the file has no test module today;
the readers/`interner.rs` pattern puts them in-file). PDK fixtures via
`crate::test_support::{sky130_hd, design_dir, require}` — panic on
missing inputs, never skip (AGENTS.md §2).

Fixture load (the established pattern from `sta/analysis.rs` tests):

```rust
let tech = load_tech_db(&tlef, &lef, &mut interner)?.0;      // tlef+lef under sky130_hd()
let lib = load_liberty(&sky130_hd().join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib"))?;
let db = parse_def(&design_dir("gcd").join("gcd.placed.def"), &tech, Some(&lib))?;
```

`Some(&lib)` on the parse and on `insert_*` is deliberate: it proves
`pin_lib_pins`/`cell_lib_ids` resolution on inserted cells, which the
CTS ticket's STA pass will need.

## Invariant checker

`fn assert_db_invariants(db: &DesignDb)` — a test-local helper asserting
I1–I6 from `api.md`, including **both directions** of the back-pointer:
for every `p ∈ net n`'s segment, `pin_nets[p] == Some(n)`; and for
every `pin_nets[p] == Some(n)`, `p ∈ n`'s segment. Also `io_pins` ⊆
`pin_cells == None` pins and driver-first on every non-empty segment.
Called after every mutation step in every test.

## T0 — `test_connect_disconnect_unit` (no PDK)

Hand-built two-net design, reusing the construction recipe in
`interner.rs::test_design_db_construction` (I/O ports + one cell,
`net_pin_offsets` written literally). Cheap coverage of the mechanics
the gcd test won't isolate:

- `create_net` → empty segment, `offsets` still well-formed.
- `connect_pin` of a sink → segment end; of a driver → index 0
  (driver-first on insertion, not just at parse).
- `disconnect_pin` of a driver from a two-driver segment → remaining
  driver moves to index 0; `pin_nets` cleared.
- `connect_pin` moving a pin between nets → absent from old segment.
- `connect_pin`/`disconnect_pin` no-ops (already-on / unconnected).
- `assert_db_invariants` after each step.

## T1 — `test_insert_buffers_on_clk_net` (gcd)

`gcd.placed.def`'s `clk` net is `( PIN clk )` + 35 `_*_ CLK` sinks —
the driver is a primary I/O port, which doubles as the I/O-pin edge
case. Two insertions exercise both a fresh net and mutation of a pin
that was itself just inserted:

1. Look up `clk` via a test-local linear scan of `net_names` (no
   `net_by_name` API — see `api.md` "Rejected"). Look up the
   `sky130_fd_sc_hd__clkbuf_16` `MacroId` via
   `tech_db.macro_by_name` (the cell TritonCTS actually picked on gcd —
   `docs/results.md` 2026-11-22).
2. `insert_buffer_on_net(clk, clkbuf16, "clkbuf_0_clk",
   "clknet_0_clk")`. Assert:
   - `num_cells`/`num_nets` + 1; `num_pins` + `macro.pin_count`
     (assert against `tech_db.macro_pins(mac).len()` — do not hardcode;
     `clkbuf_16` carries A/X plus `INOUT` power pins in LEF).
   - `clk` segment = `{PIN clk, clkbuf_0_clk/A}` with `PIN clk` first
     (`net_driver` = the port pin).
   - `clknet_0_clk` segment = `{clkbuf_0_clk/X}` + the 35 former sinks,
     in their original order; `net_driver` = `clkbuf_0_clk/X`.
   - `io_pins` length and contents unchanged.
   - Buffer power pins (`mpin.is_power`) have `pin_nets == None`.
   - `assert_db_invariants`.
3. Chain a second buffer: `insert_buffer_on_net(clknet_0_clk, clkbuf16,
   "clkbuf_1_clk", "clknet_1_clk")` — moves `clkbuf_0_clk/A` (a pin the
   API itself created) onto `clknet_1_clk`; assert structure +
   `assert_db_invariants` again.

## T2 — `test_mutation_def_roundtrip` (gcd)

Continue from the T1-mutated design (or rebuild it in the test — pick
whichever reads cleaner; both inserts are three calls):

1. Seed each inserted cell's `cell_x_dbu`/`cell_y_dbu` at the
   `pin_pos_dbu` of the net it was inserted on's driver (Abacus needs a
   finite start point; `(0,0)` would drag it to the row origin).
2. Build `gp_x_um`/`gp_y_um` = `cell_*_dbu as f32 / dbu_per_um` for all
   cells; `legalize_placement(&mut db, &tech, &gp_x_um, &gp_y_um)` →
   `verify_legality`. Assert each inserted cell ends `Placed`.
   **Note**: all `Placed`/`Unplaced` Core cells are movable to Abacus —
   other cells may shift; this test asserts membership, not positions.
3. `write_def_full(&db, &tech, tmp)` then `parse_def(tmp, &tech,
   Some(&lib))` — must succeed with **no writer workaround**; the
   strict parser is the validity check.
4. Compare re-read vs mutated DB: `num_cells`/`num_pins`/`num_nets`
   equal; per-net membership equal as name sets (`(inst, pin)` for
   instance pins, `(PIN, port)` for I/O — resolve through `strings`,
   don't rely on `PinId`/`NetId` equality); per-cell pin sets equal;
   `net_driver` names equal on `clk`/`clknet_0_clk`/`clknet_1_clk`.

## T3 — `test_untouched_net_emission_identical` (gcd)

Scope: emitted text, mutation only — **before** legalization (Abacus
may shift placed cells; membership byte-identity is T2's job).

1. `write_def_full` the freshly parsed DB → string A.
2. Run the T1 insertions → `write_def_full` → string B.
3. For every net except `clk`, the `- <name> ( conns ) + USE SIGNAL ;`
   line in A appears **byte-identical** in B (the writer emits one line
   per net — split on `\n`, match on the `- <name>` prefix; escaped
   names like `ctrl.state.out\[1\]` match because both emits use
   `escape_def_name`). Same for every pre-existing `COMPONENTS` line
   and the whole `PINS` section.
4. Control net (use `ctrl.state.out[1]`, a 3-pin net on the first
   `+ USE SIGNAL` line after `clk`): per-net HPWL computed inline
   (`net_pins` + `pin_pos_dbu`) and every member pin's `pin_pos_dbu`
   identical pre/post. Guide parasitics are a pure function of those
   inputs (`NetRcTree` builds from member-pin shapes and positions), so
   byte-identical connectivity + positions entails identical parasitics
   — record this interpretation in `docs/results.md` rather than
   re-running the guide pipeline in a unit test.

## T4 — fail-proof record (manual, in `docs/results.md`)

Per AGENTS.md §7.7, prove `assert_db_invariants` can fail. In the T1
test body (or a scratch run — results.md must name which):

- **Corruption A**: swap `clknet_0_clk`'s segment so a sink sits at
  index 0 → checker panics on I4. Restore.
- **Corruption B**: set `pin_nets[sink] = None` while leaving the pin
  in the segment → checker panics on I3. Restore.

Record both red runs (command + assertion text) in `docs/results.md`.

## Coverage caveat (write into `results.md`)

Tests exercise DEF-sourced designs only. Verilog/Yosys-JSON–sourced
`DesignDb`s share the same CSR invariants (their readers build
driver-first CSR the same way), but mutation on them is untested here —
same class of gap as AGENTS.md §7.4, so it is stated, not claimed.
