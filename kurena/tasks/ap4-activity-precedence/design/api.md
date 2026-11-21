# api — `User` origin, `pins` override, final seed precedence

AP4 touches `activity.rs` only in the seed chain — `resolve_activity`'s
signature is unchanged. The propagation pass, constant cone, and
producer tables are untouched.

## 1. `ActivityOrigin::User`

```rust
/// `set_power_activity -pins` / `-input_ports` — an explicit user
/// set, measured source label `user` (P0b). Sits below
/// `global`/`clock`/`constant` and above dump annotation in the
/// seed order.
User,
```

`label()` returns `"user"`. The `input_ports` arm's `// golden label
user maps here` comment goes away — the origin is now honest.

`ActivityOrigin` stays `Copy`; the doc comment on `Annotated` narrows
to "VCD/SAIF dump entry" (user annotations no longer fold into it).

## 2. `ActivityOverrides::pins`

```rust
/// `-pins <list>`: per-pin user overrides. The call site resolves
/// pin names to `PinId`s (name resolution is a caller concern, same
/// as the `-activity` rate normalization). An entry annotates its
/// net iff the pin is the net's driver — identical to the dump-
/// annotation rule (`annotation_net_of`): input ports and instance
/// output pins drive; sink pins and output ports are dead `user`
/// entries, counted in `dead_user_entries`. Last entry wins on a
/// duplicate pin.
pub pins: Vec<(PinId, ActivitySetting)>,
```

`ActivityOverrides` derives drop `Copy` (`Clone, Debug, Default` only)
— a heap field is unavoidable for a per-pin list. Call sites pass
`&ActivityOverrides` everywhere; the mba test's `pin_obj` name→`PinId`
map is the name-resolution precedent.

Inside `resolve_activity`, build once before the seed loop:

```rust
let pin_overrides: HashMap<PinId, ActivitySetting> =
    overrides.pins.iter().copied().collect(); // collect keeps the last dup
```

`dead_user_entries: usize` on `ActivityResolution` counts entries
whose pin resolves no net (`pins` entry on a non-driver pin, or a pin
with no net) — incremented in the seed loop when the driver lookup
misses… no: dead entries are counted by *difference*: an override pin
that never surfaces as a net driver is dead. The clean form is a
one-time pass: `overrides.pins` entries where
`!design.pin_is_driver[p.0 as usize] ||
design.pin_nets[p.0 as usize].is_none()` — counted once, up front,
before the seed loop. (A driver entry is never dead even if a
higher-precedence arm claims its net first — the pin *was* resolved;
matching `annotation_net_of`'s contract.) Diagnostic only; it is *not*
added to `PowerDiagnostics` — same surface as `dead_annotation_entries`.

## 3. Final seed order

Per net, first match wins (measured labels in parens):

| # | origin | arm |
|---|---|---|
| 1 | `Constant` | `constant_cone` mark |
| 2 | `Clock` | port-driven `create_clock` source |
| 3 | `Global` | `overrides.global` |
| 4 | **`User`** | `pin_overrides.get(&driver)` — per-pin |
| 5 | **`User`** | `overrides.input_ports` + io-pin driver |
| 6 | `Annotated` | dump `pin_signals` driver match |
| 7 | `Input` | undriven net or io-pin driver |
| 8 | `Default`→propagation→`Unknown` | fill |

Arms 4–5 replace today's single `input_ports` arm:

```rust
} else if let Some(&u) = driver.and_then(|d| pin_overrides.get(&d)) {
    ResolvedActivity {
        prob_one: Some(u.prob_one),
        eff_toggles: u.density_per_s * window_s,
        origin: ActivityOrigin::User,
    }
} else if let Some(ip) = overrides.input_ports.filter(|_| {
    driver.is_some_and(|d| design.pin_cells[d.0 as usize].is_none())
}) {
    ResolvedActivity {
        prob_one: Some(ip.prob_one),
        eff_toggles: ip.density_per_s * window_s,
        origin: ActivityOrigin::User,
    }
}
```

Precedence evidence (all in the 2026-11-21 probe entry):

- `constant`/`clock`/`global` above `user`: `s_input_ports`'s `clk`
  reads `clock`; `-global` after `-input`/`-input_ports` still won on
  every non-clock non-constant pin (p0b, committed probe); constants
  are immune to `-global` hence to `user` (order-inherited, measured
  class behavior).
- `user` above dump annotation: chosen order from AP3 — no committed
  session mixes `set_power_activity` with `read_vcd` on the same pin;
  recorded, unmeasured.
- `user` above `input` default: `s_input_ports` — `a`/`b` read `user`,
  undriven `n_float` keeps `input` (`input_ports` applies to io-pin
  drivers only; `pins` on a port the net doesn't drive is dead, not
  `input`-replacing).

## 4. The `-activity` / `-density` contract (unchanged, re-pinned)

`ActivitySetting` carries absolute rates; conversions are the call
site's (doc comment already states this — the comment is kept and the
test's literal `setting(1e7, 0.4)` values *are* the normalization:
`0.2 × 5e7` and `0.03 × 1e9`):

- `-activity a` → `density_per_s = a × f_clk`, `f_clk = 1e9 / period_ns`
  — **clock-normalized**;
- `-density d` → `density_per_s = d × 1e9` — per library time unit
  (sky130: ns).

`s_p40` is the pin: under `mba_p40.sdc` (period 40) the internal
`f_clk/10` input default halves to `2.5e6` and `clock` to `5e7` — the
normalization *inside* `resolve_activity` tracking the SDC, which is
exactly the arithmetic a caller applies to `-activity`. No helper is
added; a `ActivitySetting::clock_normalized` convenience stays
optional and is **not** in this ticket's scope (keeps the API surface
minimal — the literal call sites document the rule).

## 5. Doc/comment updates

- `activity.rs` header fill-order line: gain `→ \`-pins\` →
  \`-input_ports\`` between `global` and annotation; the `input_ports`
  arm comment loses "maps to `Annotated`".
- `power.rs` header (line ~17): the precedence comment gains the
  `user` arms in the same position.
- `main.rs` `ORIGIN_ORDER`: add `"user"` (position after `global`,
  matching the measured label order) — diagnostic completeness only;
  the CLI still passes `ActivityOverrides::default()`, so `user=` can
  never print today. One-line change, no new flags.

## 6. Version

`Cargo.toml` `0.2.17 → 0.3.0` — **minor**, not patch: `ActivityOverrides`
loses `Copy` (public type, breaking under the workspace's 0.x rule —
"breaking changes bump `minor`"). `Cargo.lock` refreshes via
`cargo check`.

## Files

- `crates/grus-core/src/activity.rs` — `User` variant + label, `pins`
  field, `pin_overrides` map, `dead_user_entries`, two `User` seed
  arms, doc updates
- `crates/grus-core/src/power.rs` — header precedence comment
- `crates/grus-eda/src/main.rs` — `ORIGIN_ORDER` + `"user"`
- `Cargo.toml`, `Cargo.lock`
