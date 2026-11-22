# e5-cts-tree — API

## `crates/grus-core/src/cts/` (new)

`mod.rs` — public surface, re-exported types, tests. `tree.rs` — the
constructor internals of `algorithm.md`.

```rust
pub struct CtsConfig {
    /// Buffer macros, strongest → weakest (`sky130_fd_sc_hd__clkbuf_8`,
    /// `clkbuf_4`, `clkbuf_2`). First entry is the capacity fallback.
    pub buf_list: Vec<String>,
    /// Macro driving the top level (`sky130_fd_sc_hd__clkbuf_16`).
    pub root_buf: String,
    /// Soft cap on loads per buffer output pin; groups split above it.
    pub max_loads: usize,          // 8
    /// Layer used for the wire-cap estimate in buffer sizing.
    pub wire_layer: String,        // "met2" — the STA placement-RC layer
}
impl Default for CtsConfig { /* as annotated; mirrors -buf_list/-root_buf */ }

#[derive(serde::Serialize)]
pub struct CtsReport { /* schema in report.md */ }

/// Build a depth-balanced buffer tree on every design clock's root net,
/// legalize the inserted cells only (pre-existing cells pinned, then
/// restored), and return the census report.
///
/// `lib` is required (buffer `max_cap` sizing + `pin_lib_pins` so the
/// inserted cells are STA-visible). `sdc` supplies the clock model —
/// the constructor never opens an SDC file itself.
pub fn run_cts(
    db: &mut DesignDb,
    tech: &TechDb,
    lib: &LibDb,
    sdc: &Sdc,
    cfg: &CtsConfig,
) -> Result<CtsReport>;
```

Internal to `tree.rs`:

```rust
// `port_pin` from the draft was dropped — the driver lookup makes it
// redundant (a driverless root is refused, a port-driven root carries
// the port as its driver) — no other consumer surfaced.
struct ClockRoot { clock_name: String,
                   root_net: NetId, driver_pin: PinId }
fn clock_roots(db: &DesignDb, sdc: &Sdc) -> Result<Vec<ClockRoot>>;
fn partition(loads: &[(PinId, (i32, i32))], max_loads: usize)
    -> Vec<Vec<(PinId, (i32, i32))>>;
fn pick_buffer(group: &[(PinId, (i32, i32))], db, tech, lib,
               buf_list: &[String], c_pf_per_um: f32)
    -> String;                    // macro name from cfg.buf_list
fn buffer_signal_pins(db: &DesignDb, cell: CellId, tech: &TechDb)
    -> (PinId /*in*/, PinId /*out*/);   // non-power Input/Output scan
fn legalize_inserted_only(db: &mut DesignDb, tech: &TechDb,
                          inserted: &HashSet<CellId>) -> Result<()>;
```

`lib.rs` gains `pub mod cts;` next to `pub mod power;`.

## `crates/grus-core/src/sta/analysis.rs` — the surfaced accessor

The informational skew/insertion assertions need per-pin clock arrival
times after a DEF re-parse. `run_sta` discards `pin_timing` before
returning `StaReport`, so the cts test needs the same internal entry
point the propagated-clock tests already use:

```rust
// was: fn run_sta_with_state(...)   (#[cfg(test)], module-private)
#[cfg(test)]
pub(crate) fn run_sta_with_state(
    design: &DesignDb, lib_db: &LibDb, sdc: &Sdc,
    parasitics: Option<&NetParasitics>,
) -> Result<(StaReport, Vec<PinTimingState>)>;
```

plus `pub(crate)` on `struct PinTimingState` and its `arrival_max` /
`arrival_min` fields (they stay `#[cfg(test)]` — the cts tests are
`#[cfg(test)]` too, so no production surface changes). The cts test
collects `<cell>/<pin>`-keyed arrivals with `Context::pin_name`-style
formatting: `format!("{}/{}", resolve(cell_names[c]), resolve(pin_names[p]))`.

No `DesignDb` change: `io_pins`, `pin_nets`, `net_pins`,
`net_driver`, `pin_pos_dbu`, `cell_macros`, `cell_status`,
`cell_x/y_dbu`, `pin_lib_pins`, `net_by_name` are all public already.
`TechDb::macro_by_name` + `LibDb::cell_by_name` cover macro/Liberty
lookup by name.

## `crates/grus-eda/src/main.rs` — `cts` subcommand

```rust
/// Clock-tree synthesis: buffered clock tree + legalized DEF (E5)
#[command(name = "cts")]
Cts {
    #[arg(long)] tlef: PathBuf,
    #[arg(long)] lef: PathBuf,
    #[arg(long)] lib: PathBuf,
    #[arg(long)] def: PathBuf,
    #[arg(long)] sdc: PathBuf,
    #[arg(long)] def_out: PathBuf,
    #[arg(long)] report: Option<PathBuf>,   // census JSON; required by the
                                            // acceptance's per-run report
},
```

Flow (mirrors `sta`/`power`): load `TechDb` → `parse_def` → `load_lib_db`
(needed before `run_cts`, for `pin_lib_pins` on new cells) → `load_sdc`
→ `grus_core::cts::run_cts(&mut design, &tech, &lib, &sdc, &config)`
→ `write_def_full` to `--def-out` → print census line
(`buffers=N, depth=D, sinks=S, hpwl=… µm (M ms)`) → write
`--report` JSON with `serde_json::to_string_pretty`. No `--strict`
variant this ticket.

## CLI parse test additions (`main.rs` tests)

- `cts` with all args parses; missing `--def-out`/`--sdc` errors.
- Extend `cli_end_to_end_gcd_flow_and_validation` or a sibling test:
  `cts` on `gcd.placed.def` + `gcd.cts.sdc` writes a DEF that
  `parse_def` + `verify_legality` accept and a report JSON that
  deserializes — the e2e acceptance at the binary level (in-process
  `run(Cli::try_parse_from(…))`, same as the existing `sta`/`place`
  arms — no subprocess).
