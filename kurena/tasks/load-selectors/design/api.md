# load-selectors — Rust-side design

The probes in `probe.md` decide the assignment semantics marked
"measured" below. The parser/data-model changes do not depend on those
answers; the final capacitance mapping does.

## 1. SDC model

Add a net collection value next to `Value::Pins`:

```rust
enum Value {
    Str(String),
    Ports(PortSel),
    Clocks(Vec<String>),
    Cells(CellSel),
    Pins(Vec<String>),
    /// `get_nets`: net names, symbolic until the design is known.
    Nets(Vec<String>),
    Opaque,
}
```

Keep load rules typed rather than widening `ObjSel` (exception
`-from`/`-to` selectors have different coverage semantics and `get_nets`
should still count as an unresolvable exception selector):

```rust
/// Object sets `set_load` can annotate, resolved once `DesignDb` is known.
#[derive(Clone, Debug)]
pub enum LoadSel {
    Ports(PortSel),
    /// `get_pins` `instance/pin` specs. Top-level ports stay a port
    /// selection (`get_ports`), matching the existing `get_pins` model.
    Pins(Vec<String>),
    /// `get_nets` name list.
    Nets(Vec<String>),
}

#[derive(Clone, Debug)]
pub struct LoadRule {
    pub sel: LoadSel,
    pub value_pf: f32,
}
```

`Sdc::loads` becomes `Vec<LoadRule>`. `Sdc::output_load_pf` filters only
`LoadSel::Ports` and keeps the current last-matching-port rule, so
`power.rs` and every existing caller keep their API and behaviour.

Dispatch changes in `Interp::command`:

- `"get_nets" | "get_net"` -> `Value::Nets(plain_names(args))`; malformed
  arguments or any flag -> `Value::Opaque`.
- `"set_load"` -> a new `load_rule` helper instead of `value_and_ports`.
- `"set_fanout_load"` -> a new counted command branch.

`remove_from_collection` handles `Value::Nets` the same way as pins: only
same-namespace enumerable collections subtract by name; mixing
`Value::Nets` with ports/cells/pins/clocks returns `Opaque`.
`obj_sel(Value::Nets)` returns `None`, preserving the exception command's
whole-command `ignored_commands` diagnostic.

## 2. `set_load` parsing

Use `positionals` with the existing `PORT_VALUE_FLAGS`,
`IGNORED_VALUE_FLAGS`, and `-pin_load` boolean flag. The positional form
remains exactly `[Value::Str(number), object]`. `load_rule` returns
`Result<Option<LoadRule>>`:

- `Value::Ports` -> `Some(LoadSel::Ports)`
- `Value::Pins` -> `Some(LoadSel::Pins)`
- `Value::Nets` -> `Some(LoadSel::Nets)`
- `Value::Opaque`, `Value::Cells`, `Value::Clocks` -> `None`, caller
  increments `ignored_commands["set_load"]`
- anything else, wrong arity, non-numeric value, or an unknown option ->
  parse error, as today

`[all_fanouts]` and `[all_fanins]` intentionally remain `Opaque` for now:
they are cone queries, not enumerable object lists, and a partial cone
approximation would change timing without proving the domain. The command
they are inside counts the skip. Add explicit branches for their accepted
argument forms only if the probe shows the cone expands to a
representable object set; otherwise leave the generic
`is_object_selector` fallback and count `set_load`.

The probe may find additional accepted qualifiers (`-min`, `-max`,
`-rise`, `-fall`, `-wire_load`, `-subtract_pin_load`, etc.). Do not apply
a corner- or transition-restricted load to both transitions. Add such a
flag only if the measured behaviour is implemented; otherwise consume it
explicitly and count the whole command in `ignored_commands["set_load"]`,
rather than letting `positionals` abort the SDC. `-pin_load` keeps its
current accepted status; if it changes the measured placement of net or
pin loads, carry a `pin_load` bit on `LoadRule`.

## 3. `set_fanout_load` parsing

Validate the same `numeric value + one object` shape (plus only flags the
probe proves safe to ignore). A malformed command still errors. For the
expected outcome — fanout units are a design-rule/load attribute, not a
capacitance the delay model evaluates — accept the command and increment
`ignored_commands["set_fanout_load"]`. If the probe shows OpenSTA moves
endpoint arrival or required values, the plan must be revised rather than
guessing a delay-model mapping.

If the command only feeds `report_check_types -max_fanout`, add
`"set_fanout_load"` to the `SkipReport::is_clean` exemption after
recording that measurement. If OpenSTA rejects it in 3.0.16, leave it
non-exempt: accepting it is a tolerated parse compatibility skip, not a
modelled constraint.

## 4. Design-time resolution

Resolve once in `prepare_sta` before `pi_models` and
`wire_delays_per_net` are built. Store the result on `Context` so every
capacitance consumer sees the same table:

```rust
struct ResolvedLoads {
    /// Additional or replacement pin capacitance per pin, as measured.
    pin_loads_pf: Vec<Option<f32>>,
    /// Net-level annotation per net, interpreted by the measured rule.
    net_loads_pf: Vec<Option<f32>>,
    unresolved: usize,
}
```

The concrete representation can split `Option` into `Add`/`Replace` if
the measured semantics need both (`pin` and `net` are measured
separately). Resolve names through `DesignDb` only:

- `LoadSel::Ports` needs no resolution; `output_load_pf` still answers
  it.
- `LoadSel::Nets` resolves exact net names against `net_names`/
  `strings`. Glob-like or missing names are not guessed; count them.
- `LoadSel::Pins` resolves `instance/pin` specs against a
  `pin_by_path` map. Factor the existing `instance/pin -> PinId`
  construction out of `prepare_exceptions` or share a small helper; do
  not build a second inconsistent map.
- A pin with no attached net is still a valid object if the measured
  semantics attach capacitance at the pin; otherwise it is unresolvable
  and counted.

Ordering policy: rules are applied in file order. For the same measured
target, a later `set_load` replaces the earlier annotation rather than
accumulating annotations (subject to the probe). If a `get_pins` rule and
a `get_nets` rule collapse to the same measured target, apply the same
last-wins target overwrite OpenSTA exhibits; if they are different
domains, keep both.

Mixed collections need the probe's verdict before implementation:

- If OpenSTA warns and applies resolvable objects, apply the resolved
  subset and count each missing object.
- If OpenSTA ignores the whole command on a missing object, drop the
  whole rule and count it once.
- If duplicate pins on one net accumulate pin-load annotations, keep the
  measured multiplicity; if it is set-once semantics, deduplicate by
  target.

Any selected object the wire model cannot represent under the chosen
parasitic mode increments `unresolved_load_selectors`; do not fall back
to a partial approximation there.

## 5. Capacitance application

`sink_cap(pin, pol)` remains the single per-pin capacitance source. The
measured pin semantics choose one of these forms:

- additive pin annotation: `intrinsic_or_port_cap + pin_annotation`
- replacement pin annotation: `pin_annotation` when present, otherwise
  the existing intrinsic/port cap
- net-attached annotation: the annotation contributes to the attached
  net's load at the measured node rather than replacing the pin's own
  capacitance

`net_load(net, pol)` applies the measured net rule. Candidates:

1. **extra driver-node cap**: add `net_annotation` once, and include it
   in the pi model at the driver end;
2. **wire-cap override**: replace `tree.total_wire_cap_pf` with the
   annotation before summing pin caps, and mirror the same replacement in
   the pi/wire-delay model;
3. **total-net override**: use the annotation as the complete net load
   (only viable if the measured result treats Liberty pin caps as
   included in the override).

`build_pi` and `wire_delays` must use the same `sink_cap`/resolved table
they already share indirectly. If the measured net semantics cannot be
inserted into the RC pi model without changing Elmore timing incorrectly,
apply the load in zero-wire mode only and count `unresolved_load_selectors`
when `parasitics.is_some()`; do not silently use a different semantic in
the two modes. The `sta_exceptions` golden only exercises zero-wire, so a
parasitic restriction must be recorded explicitly in `docs/results.md`.

## 6. Skip accounting

Add to `SkipReport`:

```rust
/// `set_load` net/pin names that could not be resolved or whose measured
/// target the wire model cannot represent.
pub unresolved_load_selectors: usize,
```

Update:

- `nonzero()` — emits `unresolved_load_selectors=N`
- `is_clean()` — requires it to be zero
- `merge_skip_reports()` — `max`, like the other global counters
- partitioned report composition — do not overwrite a
  `sdc_ignored_commands` entry inserted during resolution; copy merge the
  maps or keep the count in the new field

Parser-level unsupported selectors stay in
`sdc_ignored_commands["set_load"]`; resolution-level misses stay in
`unresolved_load_selectors`. That split preserves both "command was not
applied" and "why" in the JSON/text summary.

## 7. Unit and integration tests

` sdc.rs` tests:

- `get_nets`/`get_net` produce `Value::Nets`, including through
  `set load_nets [get_nets ...]` variable assignment.
- `set_load` accepts `get_ports`, `get_pins`, and `get_nets` and stores
  the correct `LoadSel`; existing port last-wins tests stay unchanged.
- `all_fanouts`, `all_fanins`, `get_cells`, `all_clocks`, and opaque
  `get_nets` forms make `set_load` parse and increment
  `ignored_commands["set_load"]`.
- `set_fanout_load` parses a numeric+object form and increments its
  command count; missing/non-numeric values still fail.
- `remove_from_collection` on net collections subtracts by name; mixed
  namespaces become opaque.

`analysis.rs` tests:

- The existing `test_sta_exceptions_match_opensta` loop picks up all
  non-error `ld_*` scenarios at `Tol::STRICT`.
- Extend the diagnostics test to assert `set_load`,
  `set_fanout_load`, and `unresolved_load_selectors` counts and the
  measured `is_clean()` verdict.
- If a fanout-load scenario is counted but timing-neutral, assert its
  report values equal `base` within STRICT while the skip count is still
  present.

## 8. Docs and version

- `docs/results.md`: dated entry with the current failure, probe matrix,
  measured rules, applied vs counted scenarios, exact command/script run,
  and fail-proof result.
- `docs/roadmap.md`: remove the completed `set_load`/`set_fanout_load`
  selector bullet from N2; leave path-delay follow-ups intact.
- `docs/e3-sta.md`: update the `set_load` SDC subset line to describe the
  measured supported object domains and counted skips.
- `docs/timing-exceptions.md`: update the skip-surface section if a new
  counter or `is_clean` exemption changes the user-visible summary.
- `Cargo.toml` patch bump; regenerate `Cargo.lock` if it changes.

## Files

- `crates/grus-core/src/sta/sdc.rs`
- `crates/grus-core/src/sta/analysis.rs`
- `tests/designs/sta_exceptions/`
- `docs/results.md`
- `docs/roadmap.md`
- `docs/timing-exceptions.md`
- `docs/e3-sta.md`
- `Cargo.toml`
- `Cargo.lock`
