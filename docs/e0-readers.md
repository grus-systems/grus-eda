# E0: Readers & Binary Caching — Implementation Spec

This document breaks E0 into five small, independently verifiable implementation steps (`E0.1`–`E0.5`) for the implementer. Commit after completing and verifying each sub-step.

---

## 1. Done Criteria

| Sub-Step | What | Reference Command (`ghcr.io/librelane/librelane:3.0.16`) | Done When |
|---|---|---|---|
| **E0.1** | Workspace scaffold & core SoA/CSR structs | `cargo test --workspace` | Compiles cleanly with zero warnings |
| **E0.2** | `tests/designs/gcd` setup & reference artefacts | Yosys + OpenROAD + OpenSTA in container | `.json`, `.v`, placed `.def`, `.spef`, and golden count logs generated |
| **E0.3** | Liberty (`.lib`) parser + `bincode`/`mmap` cache | `sta`: `get_lib_cells`, `report_lib_cell` | Cell count, pin count, sequential cell count, and NLDM table dimensions match 100%; cached load `< 10 ms` |
| **E0.4** | LEF (`.tlef` + `.lef`) parser | `openroad`: `read_lef` | DBU/micron, routing layer count/pitch/R/C, site size, and macro/pin counts match 100% |
| **E0.5** | Yosys JSON, gate-level Verilog, and DEF reader/writer | `openroad`: `read_def` / `read_verilog` | Instance count, net count, primary I/O pin count, and row count match 100% on `gcd` (and `picorv32`) |

---

## 2. Step-by-Step Implementation Guide

### Step E0.1: Cargo Workspace & Core Types

1. Create root `Cargo.toml`, `crates/grus-core/Cargo.toml`, and `crates/grus-eda/Cargo.toml` exactly as specified in `docs/architecture.md`.
2. Implement `interner.rs`, `lib_db.rs`, `tech_db.rs`, and `design_db.rs`.
3. Add a unit test in `interner.rs` verifying round-trip `intern` $\rightarrow$ `resolve` and `rebuild_map`.

---

### Step E0.2: Test Design (`gcd`) & Container Reproduction Script

1. Locate the SkyWater `sky130_fd_sc_hd` PDK files inside the cached LibreLane container (`ghcr.io/librelane/librelane:3.0.16`) or OpenROAD-flow-scripts public `gcd` RTL (`gcd.v`, `gcd.sdc`).
2. Create `tests/designs/gcd/` and `tests/designs/README.md` documenting the exact container commands to generate:
   - `gcd.synth.json` (via Yosys `synth -top gcd; dfflibmap -liberty ...; abc -liberty ...; clean; write_json gcd.synth.json; write_verilog -noattr gcd.synth.v`)
   - `gcd.placed.def` (via OpenROAD `initialize_floorplan`, `place_pins`, `global_placement`, `detailed_placement`, `write_def`)
   - `gcd.reference.json` (golden counts dumped by an OpenSTA / OpenROAD Tcl script: number of library cells, macros, instances, nets, I/O ports, and rows).
3. Keep committed files under `tests/designs/` small (`< 1 MB` per file). Do **not** commit the full `.lib` or `.lef` files if they exceed 1 MB; instead, mount or reference them from the local PDK / container cache path (or pass via CLI / env var `SKY130_PDK_ROOT`) and document the exact path in `tests/designs/README.md`.

---

### Step E0.3: Liberty (`.lib`) Parser & Binary Cache (`readers/liberty.rs`)

#### Tokenizer Rules
Liberty syntax is a hierarchy of `group_type (args...) { statements... }`, attributes `key : value ;`, and complex attributes `values ( "v1, v2", "v3, v4" );`.
Implement a zero-copy byte-slice lexer (`&[u8]`) yielding tokens:
- `Ident(&str)` (unquoted identifiers and numbers: `[A-Za-z0-9_.\-+]+`)
- `Str(&str)` (double-quoted strings `"..."`)
- `LParen`, `RParen`, `LBrace`, `RBrace`, `Colon`, `Semi`, `Comma`
- Skip `/* ... */` block comments, `// ...` line comments, and backslash-newline `\` line continuations.

#### Group Extraction Rules
1. **Top-level `library(name)`**:
   - Parse `time_unit : "1ns"` ($\rightarrow$ `1.0`), `"100ps"` ($\rightarrow$ `0.1`), `"1ps"` ($\rightarrow$ `0.001`).
   - Parse `capacitive_load_unit (1, pf)` or `(1.000000, "pf")` ($\rightarrow$ `1.0`), `(1, ff)` ($\rightarrow$ `0.001`).
   - Parse `lu_table_template(template_name)`: record `variable_1` and `variable_2` (`input_net_transition`, `total_output_net_capacitance`, `constrained_pin_transition`, `related_pin_transition`) and default `index_1` / `index_2` vectors in a `HashMap<&str, TemplateDef>`.
2. **`cell(cell_name)`**:
   - Parse `area : <f32>;`.
   - If a child group `ff(...)` is present, set `is_sequential = true`.
   - If a child group `latch(...)` is present, set `is_latch = true`.
   - Ignore `pg_pin(...)`, `leakage_power(...)`, and `internal_power(...)` blocks by skipping balanced braces `{ ... }` in $O(1)$ stack depth.
3. **`pin(pin_name)`** (and `bus` / `bundle` pins if any):
   - Parse `direction : input | output | inout | internal;`.
   - Parse `capacitance`, `rise_capacitance`, `fall_capacitance` (multiply by `cap_unit_pf`; if `rise_capacitance` / `fall_capacitance` are omitted, fall back to `capacitance`).
   - Parse `clock : true;`, `max_capacitance`, `max_transition`.
4. **`timing()` inside `pin(target_pin)`**:
   - Parse `related_pin : "<pin_name>"` (note: if `related_pin` contains space-separated multiple pins, create one `LibTimingArc` per related pin).
   - Parse `timing_sense` (`positive_unate`, `negative_unate`, `non_unate`).
   - Parse `timing_type` (defaulting to `Combinational` if omitted).
   - For child groups `cell_rise(tpl)`, `cell_fall(tpl)`, `rise_transition(tpl)`, `fall_transition(tpl)`, `rise_constraint(tpl)`, `fall_constraint(tpl)`:
     - Look up `var1` and `var2` from `tpl` in `lu_table_template`.
     - Parse `index_1 ("...")`, `index_2 ("...")` (splitting on `,` and whitespace into `Vec<f32>`), falling back to the template's indices if omitted in the table instance.
     - Parse `values ("...", "...")` into a flat `Vec<f32>` of length `index_1.len() * index_2.len()`. Multiply every value by `time_unit_ns`, and scale capacitance/transition axes by `cap_unit_pf` / `time_unit_ns`.
     - For 1D tables (where `var2 == Other` and `index_2` is empty), set `index_2 = vec![0.0]` so `NldmTable` always has $M \times N$ dimensions ($N \ge 1$).

#### Binary Cache (`<lib_path>.grus.cache` or `$TMPDIR/grus-cache/<xxh3>.libcache`)
- Header struct:
  ```rust
  #[derive(Serialize, Deserialize)]
  struct CacheFile {
      magic: [u8; 8],       // *b"GRUSLIB1"
      source_size: u64,
      source_xxh3: u64,
      db: LibDb,
  }
  ```
- Compute `xxh3_64` over the `.lib` file bytes. If the cache file exists and `magic`, `source_size`, and `source_xxh3` match, memory-map via `memmap2::Mmap` and deserialize with `bincode::serde::decode_from_slice`, then call `db.strings.rebuild_map()` and rebuild `db.cell_by_name`.

---

### Step E0.4: LEF Parser (`readers/lef.rs`)

Parse `.tlef` (technology LEF) followed by `.lef` (macro cell LEF) into `TechDb`:

1. **`UNITS`**: `DATABASE MICRONS <int> ;` $\rightarrow$ `dbu_per_micron`.
2. **`SITE <name>`**: If `CLASS CORE ;`, parse `SIZE <w_um> BY <h_um> ;` and convert to `site_width_dbu = (w_um * dbu_per_micron).round() as i32`.
3. **`LAYER <name>`**:
   - Only record layers with `TYPE ROUTING ;` in `routing_layers`.
   - `DIRECTION HORIZONTAL | VERTICAL ;`
   - `PITCH <p_um> ;` (or `PITCH <px_um> <py_um> ;`), `WIDTH <w_um> ;`, `OFFSET <o_um> ;` $\rightarrow$ convert to `i32` DBU.
   - `RESISTANCE RPERSQ <f32> ;`
   - `CAPACITANCE CPERSQDIST <f32> ;` and `EDGECAPACITANCE <f32> ;`
4. **`MACRO <name>`**:
   - `CLASS <class> ;`
   - `SIZE <w_um> BY <h_um> ;` $\rightarrow$ `width_dbu`, `height_dbu`.
   - For each `PIN <pin_name>`:
     - `DIRECTION INPUT | OUTPUT | INOUT ;`
     - `USE SIGNAL | CLOCK | POWER | GROUND ;` (`is_power = matches!(use, POWER | GROUND)`).
     - Inside `PORT`, track the current `LAYER <layer_name> ;` and compute the bounding box union $[x_{\min}, y_{\min}, x_{\max}, y_{\max}]$ across all `RECT <x1> <y1> <x2> <y2> ;` statements. Store `offset_x_dbu = (x_min + x_max) / 2` and `offset_y_dbu = (y_min + y_max) / 2`.
   - Inside `OBS`, track `LAYER <layer_name> ;` and push each `RECT` (converted to `RectDbu`) onto `obs_rects` if the layer is a routing layer.

---

### Step E0.5: Yosys JSON, Structural Verilog & DEF (`readers/{yosys_json.rs, verilog.rs, def.rs}`)

#### 1. Yosys JSON (`yosys_json.rs`)
Deserialize the top module (or the single non-blackbox module):
```rust
#[derive(Deserialize)]
struct YosysFile {
    modules: indexmap::IndexMap<String, YosysModule>, // or BTreeMap
}
```
- **Ports (`ports`)**: Each port has `direction` (`"input"`, `"output"`, `"inout"`) and `bits: Vec<serde_json::Value>`. If port `P` has multiple bits, create scalar primary I/O pins named `P[0]`, `P[1]`, ...; if 1 bit, name it `P`.
- **Cells (`cells`)**: Skip cells with `hide_name` only if they have no `type` in `TechDb`/`LibDb` (e.g., `$scopeinfo`). For each standard cell instance:
  - Lookup `MacroId` in `TechDb` and `LibCellId` in `LibDb` by `cell.type`.
  - Match each port in `cell.connections` to the corresponding `MacroPin` / `LibPin`.
- **Nets (`netnames` & integer bit IDs)**:
  - Yosys bit values are either integers (`2, 3, ...`) or strings (`"0"`, `"1"`, `"x"`, `"z"`).
  - Map each unique integer bit ID used by ports or cell connections to a contiguous `NetId`.
  - Build CSR arrays `net_pin_offsets` and `net_pin_ids`. **Crucial invariant**: when populating `net_pin_ids[net_pin_offsets[n] .. net_pin_offsets[n+1]]`, place the **driver pin** (a primary `Input` port or a cell `Output` pin) at the very first slot `net_pin_offsets[n]`.

#### 2. Gate-Level Verilog (`verilog.rs`)
- Lexer must handle **escaped identifiers**: a token starting with `\` continues until the next ASCII whitespace character (`' '`, `'\t'`, `'\n'`, `'\r'`), and the leading `\` and trailing space are stripped so `\foo[0] ` interns as `"foo[0]"`.
- Expand multi-bit declarations `input [3:0] a;`, `output [3:0] y;`, `wire [15:0] w;` into scalar nets/ports (`w[0]`..`w[15]`).
- Parse cell instantiations `macro_name inst_name ( .PORT_A(net_expr), .PORT_B(net_expr) );` where `net_expr` can be a scalar identifier, an indexed bus bit `w[3]`, or a concatenation `{a, b[1:0]}`.

#### 3. DEF Reader & Writer (`def.rs`)
- Parse `UNITS DISTANCE MICRONS <dbu> ;` (scale coordinates if DEF `dbu` differs from LEF `dbu_per_micron`).
- Parse `DIEAREA ( xl yl ) ( xh yh ) ;`.
- Parse `ROW <row_name> <site_name> <orig_x> <orig_y> <orient> DO <num_x> BY <num_y> STEP <step_x> <step_y> ;`.
- Parse `TRACKS ( X | Y ) <start> DO <num> STEP <step> LAYER <layer_names...> ;`.
- Parse `COMPONENTS <n> ;`:
  - `- <inst_name> <macro_name> + ( PLACED | FIXED | UNPLACED ) ( <x> <y> ) <orient> ;`
  - Unescape DEF backslash sequences if present so instance names match Yosys JSON / Verilog.
- Parse `PINS <n> ;`:
  - `- <pin_name> + NET <net_name> + DIRECTION ( INPUT | OUTPUT | INOUT ) ... + PLACED ( <x> <y> ) <orient> ;`
  - Record primary I/O pin coordinates in `pin_offset_x_dbu` and `pin_offset_y_dbu`.
- Parse `NETS <n> ;`:
  - `- <net_name> ( PIN <io_name> ) ( <inst_name> <pin_name> ) ... + USE ( SIGNAL | CLOCK ) ;`
  - Skip `SPECIALNETS` (`VPWR`, `VGND`) for signal connectivity.
- **DEF Writer (`write_def`)**:
  - Given the input DEF text (or `DesignDb`), rewrite the `COMPONENTS` block with updated `+ PLACED ( <x_dbu> <y_dbu> ) <orient>` coordinates while preserving `DIEAREA`, `ROW`, `TRACKS`, `VIAS`, `SPECIALNETS`, `PINS`, and `NETS`.
