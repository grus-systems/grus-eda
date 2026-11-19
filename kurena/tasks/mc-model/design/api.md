# mc-model — Rust-side design

All changes are in `crates/grus-core/src/sta/sdc.rs` unless noted. Line
numbers refer to HEAD (~v0.1.98); re-verify while editing.

## 1. Data model

```rust
/// One `create_clock` row: a design clock when `port` names a source
/// port, a virtual I/O clock when `port` is `None`.
#[derive(Clone, Debug, Default)]
pub struct Clock {
    /// `-name`, or the source port name when `-name` is absent — the
    /// handle `get_clocks`/`all_clocks` resolve.
    pub name: String,
    /// Source port of a design clock; `None` for a virtual clock.
    pub port: Option<String>,
    pub period_ns: f32,
    /// `set_clock_latency` — ideal network latency; a propagated clock
    /// ignores it.
    pub latency_ns: f32,
    /// `set_clock_latency -source`; sums with the network latency for a
    /// non-propagated clock, is the only latency applied when propagated.
    pub source_latency_ns: f32,
    /// `set_clock_uncertainty -setup`: subtracted from required times.
    pub setup_uncertainty_ns: f32,
    /// `set_clock_uncertainty -hold`: added to required times.
    pub hold_uncertainty_ns: f32,
    /// `set_clock_transition`: slew seeded on clock pins; 0 without the
    /// command.
    pub transition_ns: f32,
    /// `set_propagated_clock` on this domain.
    pub propagated: bool,
}
```

`Sdc` (sdc.rs:241-289 today) drops all single-clock scalar fields and
gains the table:

```rust
pub struct Sdc {
    /// One row per `create_clock`, in file order. Eval is single-domain:
    /// the first port-bearing row is the design clock.
    pub clocks: Vec<Clock>,
    // unchanged: derate_early/derate_late, input_delays,
    // output_delays, input_transitions, loads, driving_cells,
    // exceptions, path_delays, ignored_options, ignored_commands,
    // commands
}
```

Removed fields and their replacement accessors (single-domain reads —
only reached when `design_clock_count() <= 1`):

| old field | new accessor |
|---|---|
| `clock_name: Option<String>` | `clock_name() -> Option<&str>` — design row's `name` |
| `clock_port: Option<String>` | `clock_port() -> Option<&str>` — design row's `port` |
| `period_ns: f32` | `period_ns() -> f32` — design row's, `0.0` when no design row |
| `setup_uncertainty_ns` | `setup_uncertainty_ns() -> f32` |
| `hold_uncertainty_ns` | `hold_uncertainty_ns() -> f32` |
| `clock_latency_ns` | `clock_latency_ns() -> f32` (design row `latency_ns`) |
| `clock_source_latency_ns` | `clock_source_latency_ns() -> f32` |
| `io_clock_latency_ns` | virtual row's `latency_ns`, read via `latency_ns(Some(id))` or `clock(name)` — no dedicated accessor |
| `io_clock_source_latency_ns` | virtual row's `source_latency_ns`, same |
| `propagated_clock: bool` | `propagated_clock() -> bool` |
| `clock_transition_ns` | `clock_transition_ns() -> f32` |
| `design_clock_latency_ns()` | unchanged: design row `latency + source` |

New methods:

```rust
impl Sdc {
    /// First port-bearing row — the domain eval times against.
    /// `None` on an empty or virtual-only table (pre-`finish` state).
    pub fn design_clock(&self) -> Option<&Clock>;
    /// Number of design (port-bearing) clock rows. The eval gate.
    pub fn design_clock_count(&self) -> usize;
    /// A row by handle (`name`), or by source port. Used by tests and
    /// later tickets; parse-side code uses `resolve_clock`.
    pub fn clock(&self, name: &str) -> Option<&Clock>;
    /// `port` is a source port of any design clock — the `-no_clocks`
    /// predicate and the data-seeding exclusion.
    pub fn is_clock_port(&self, port: &str) -> bool;
    /// Effective latency an I/O delay is referenced to:
    /// `source_latency` when the row is propagated, else
    /// `latency + source_latency` (identical to the old
    /// `ClockRef::{Design,Virtual}` arms).
    fn latency_ns(&self, clock: Option<ClockId>) -> f32;
}
```

`PortSel::matches` (sdc.rs:21) signature changes from
`clock_port: Option<&str>` to the `Sdc` so `-no_clocks` checks
`is_clock_port` across every design row:

```rust
pub fn matches(&self, port: &str, is_input: bool, sdc: &Sdc) -> bool
```

Callers updated: `delay_at`, `last_value`, `Sdc::output_load_pf`,
`Sdc::driving_cell` (all `impl Sdc`, pass `self`), and
`analysis.rs:3078`/`:3141` (pass `self.sdc()`).

## 2. `ClockRef` → `ClockId`

```rust
/// Index into `Sdc::clocks`. Private: `DelayRule` is private.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ClockId(usize);
```

- `DelayRule.clock: ClockRef` → `Option<ClockId>`; `None` = design
  clock (the old `ClockRef::Design` default for `-clock`-absent rules).
- `delay_at` returns `Option<(f32, f32, Option<ClockId>)>` — still the
  last matching rule's clock.
- `input_clock_latency_ns` / `output_clock_latency_ns` call
  `latency_ns(clock)` unchanged in shape.
- `resolve_clock(name) -> Result<ClockId>`: first row with
  `c.name == name` (name covers the port-name fallback because
  `create_clock` stores `name.unwrap_or(port)`), else
  `bail!("unknown clock `{name}`")`. The explicit port match stays in
  `propagated_clock` only (see §4) — do not widen `resolve_clock` or
  `set_clock_latency 1.0 clk` on a `-name`d clock stops erroring.

## 3. Command-by-command semantics

### `create_clock` (sdc.rs:841-874)

- `[get_ports {p}]` → push a design row
  `Clock { name: name.unwrap_or(p), port: Some(p), period_ns, .. }`.
  The `clock_port.is_some()` guard is deleted. Multi-port lists keep
  `bail!("create_clock takes one source port")` (unmeasured form).
- `[]` → virtual row `Clock { name, port: None, .. }`; `name` still
  required. Any period accepted (the `finish()` equality check dies).
- `finish()` (sdc.rs:1696-1710) becomes: at least one port-bearing row,
  else `bail!("SDC defines no clock period")`; every row's
  `period_ns > 0` (a `create_clock -period 0`/negative previously hit
  the design-period check — now per-row, same spirit).

### `set_clock_latency` (sdc.rs:933-984)

- `-source` handling unchanged; `-early`/`-late`/other dash-flags still
  `bail!("unsupported option …")`.
- Targets: `[value]` → first port-bearing row (bail if none declared
  yet — ordering edge case, see overview risks); `[value, Clocks(ns)]`
  → apply to **every** resolved row (multi-name now accepted);
  `[value, Str(name)]` → single resolved row (bare write_sdc form).
- Write `row.latency_ns` or `row.source_latency_ns` per `-source`.
  Virtual rows take the same write — they are never propagated, so
  `latency_ns(Some(id))` sums both terms exactly like the old
  `io_clock_*` fields.

### `set_clock_uncertainty` (sdc.rs:879-927)

- Shape parsing unchanged (`-setup`/`-hold` dedup, value + optional
  clock list, bare name). `-from`/`-to` args land in `rest` and keep
  failing the shape match — the named error
  `set_clock_uncertainty takes a value and optional clock names`
  survives (mc-xdomain owns the inter-clock form).
- The resolution loop stops being decorative: collect `ClockId`s and
  write **each** row's `setup_uncertainty_ns` and/or
  `hold_uncertainty_ns` (no flag = both, as today). This is the
  wrong-target fix: `[get_clocks vclk]` writes the vclk row, never the
  design row.
- No clock target (`set_clock_uncertainty 0.25`) writes the design
  clock — the first port-bearing row at apply time, same rule as
  `set_clock_latency` (bail if none declared yet).

### `set_clock_transition` (sdc.rs:1034-1055)

- Resolve each name (or `all_clock_names()` when absent) and write
  `row.transition_ns` — including virtual rows. Delete the
  `ClockRef::Virtual` arm and the
  `ignored_options["set_clock_transition (virtual clock)"]` count.

### `set_propagated_clock` / `unset_propagated_clock` (sdc.rs:989-1030)

- Name collection unchanged (Str split, `Clocks`, `Ports`).
- Per name: `resolve_clock`; on `Err`, fall back to a port match
  (`clocks.iter().position(|c| c.port == Some(name))`) — the
  `get_ports {clk}` form — else propagate the `unknown clock` error.
- Per resolved row: `port.is_none()` (virtual) → `set` bails
  `set_propagated_clock does not apply to the virtual clock \`{name}\``,
  `unset` skips (today's no-op). Else
  `row.propagated = command == "set_propagated_clock"`. The
  "needs the design clock" check is unnecessary — a `set` containing
  only virtuals already bailed on the first virtual.

### `set_input_delay` / `set_output_delay` `-clock` (sdc.rs:1646-1675)

- `flag_arg(args, "-clock")` → `None` → `clock: None`;
  `Str(name)`/`Clocks([name])` → `Some(resolve_clock(name)?)`;
  `Clocks([_, ..])` keeps `-clock takes one clock`. Unknown names keep
  `unknown clock` — a `-clock ckb` on a two-clock SDC resolves to row 1
  (needed so the parse model is complete even though eval gates).

### `all_clock_names` (sdc.rs:1244-1252)

`self.sdc.clocks.iter().map(|c| c.name.clone()).collect()` — file
order, one entry per row (was: virtuals then design).

### `Interp`

`virtual_clocks: Vec<(String, f32)>` field deleted — rows append
directly to `sdc.clocks` during `create_clock`.

## 4. Eval-side (analysis.rs)

All single-clock reads become the domain-0 accessors; eval never sees a
multi-clock SDC because of the gate below. Sites (HEAD line numbers):

| site | change |
|---|---|
| `build_graph` ~1289 | `sdc.clock_port.as_deref()` → `sdc.clock_port()` |
| `build_graph` ~1388 | `sdc.propagated_clock` → `sdc.propagated_clock()` |
| `propagate_slew` ~1715 | `sdc.clock_port == Some(name)` → `sdc.is_clock_port(name)` |
| `propagate_clock` ~1849, 1863-1864 | `clock_port()`, `period_ns()`, `clock_source_latency_ns()` |
| `ideal_clock_parity` ~1990 | `clock_port()` |
| port seeding ~2353 | `sdc.clock_port == Some(name)` → `sdc.is_clock_port(name)` |
| ideal seeding ~2389-2466 | `propagated_clock()`, `design_clock_latency_ns()`, `period_ns()`, `clock_transition_ns()` |
| latch borrow ~2844-2867 | `period_ns()`, `setup_uncertainty_ns()` |
| delay-check anchors ~3599-3740 | `setup/hold_uncertainty_ns()`, `period_ns()`, `propagated_clock()` |
| MCP shift ~3775 | `period_ns()` |
| unreached gates ~3896, 3922, 4252, 4440 | `propagated_clock()` |
| pulse width ~4151 | `period_ns()` |
| check composition ~4277-4475 | `period_ns()`, `setup/hold_uncertainty_ns()`, `propagated_clock()` |
| output-port checks ~4516 | `period_ns()`, `setup/hold_uncertainty_ns()` |
| selector resolution ~3078, 3141 | `ps.matches(name, is_input, self.sdc())` — `matches` now takes `&Sdc` |
| test code ~7218, 8372 | `propagated_clock()`, `is_clock_port(port_name)` |

### The single-domain gate

Top of `run_sta_detail` (analysis.rs:2154), before `prepare_sta` — one
site covering `run_sta`, `run_sta_quiet`, `sta_pin_slews`,
`run_sta_inner`, `run_sta_with_state`, and `power` (which calls
`sta_pin_slews`):

```rust
if sdc.design_clock_count() > 1 {
    let names: Vec<&str> = sdc
        .clocks
        .iter()
        .filter(|c| c.port.is_some())
        .map(|c| c.name.as_str())
        .collect();
    anyhow::bail!(
        "sta: {} design clocks declared ({}); multi-clock analysis is not implemented yet",
        names.len(),
        names.join(", ")
    );
}
```

The message names the domains; the implementer pastes the verbatim CLI
stderr into `docs/results.md`. `main.rs` needs no change — the
`anyhow` error propagates to `Error:` output (`Commands::Sta` at
main.rs:697-748). Include `main.rs` in the file list only if the
message needs a CLI-level tweak; expect none.

## 5. What stays untouched

- `ObjSel`/`Value::Clocks` in `-from`/`-to` of exceptions and delay
  commands: still unresolvable → `ignored_commands` (mc-selectors).
- `set_clock_groups`: still `unsupported command`.
- `create_clock -waveform`, multi-port `create_clock`, `-early`/`-late`
  latency: still named bails.
- `remove_from_collection` on `Value::Clocks`: name filtering,
  order-agnostic — unchanged.
- `power.rs`: only reads `output_load_pf` — unchanged API.
