# mc-cleanups — Code changes

One section per item. Code references are against the worktree at design
time; `analysis.rs` is `crates/grus-core/src/sta/analysis.rs`, `sdc.rs` is
`crates/grus-core/src/sta/sdc.rs`, `saif.rs` is
`crates/grus-core/src/readers/saif.rs`, `power.rs` is
`crates/grus-core/src/power.rs`.

## A1 — `grid_lcm`: honest tolerance guard (`analysis.rs` ~L5083–5101)

Current loop:

```rust
loop {
    if y <= 0.0 {
        return None;
    }
    let r = x % y;
    if r <= tol {
        return Some(a as f64 / y * b as f64);
    }
    if y <= tol {
        return None;
    }
    x = y;
    y = r;
}
```

The `if y <= tol` mid-loop branch is dead: reaching it requires
`r > tol`, and `y` still holds the pre-assignment divisor — which is
`b > tol` on iteration 1 (`tol = 1e-6·min(a,b)`) or a previous `r > tol`
afterwards.

**Change:** delete the mid-loop branch and move the tolerance guard to
the loop top so the divisor check stays honest:

```rust
loop {
    if y <= tol {
        return None;
    }
    let r = x % y;
    if r <= tol {
        return Some(a as f64 / y * b as f64);
    }
    x = y;
    y = r;
}
```

`y <= tol` at the top can now fire only on the initial `b` (a period
below the f32-resolution tolerance — defensive, unreachable for real
SDCs) — that is the honest statement of the guard, and it still
protects `x % y` against a near-zero divisor. Amend the function doc to
say non-convergence below `tol` returns `None`. No test needed beyond
the existing `grid_lcm` unit tests; semantics unchanged for every
reachable input.

## A2 — Empty clock list on `set_clock_*` (`sdc.rs`)

**Today's actual surface** (design correction — see `overview.md`):

- `[get_clocks {}]` → `plain_names` bails (`names.is_empty()`) →
  `Value::Opaque` → every `set_clock_*` arm shape-bails (loud, but with
  a generic message that does not name the empty collection).
- `Value::Clocks([])` — reachable via
  `[remove_from_collection [all_clocks] [get_clocks cka]]` or
  `[all_clocks]` evaluated before any `create_clock` — is inconsistent:
  - `set_clock_latency` / `set_clock_transition` / `set_clock_uncertainty`
    (positional and `-from`/`-to`): iterate zero rows, **silently write
    nothing**.
  - `set_propagated_clock`: bails (`"set_propagated_clock takes clock
    objects"` on `names.is_empty()`).
  - `set_clock_groups -group {}`: already parses as the empty group
    (measured `t2_grpe`).

**Probe** (`t2_eclk`, `probes.md` P2): `set_clock_latency` /
`set_clock_uncertainty` / `set_clock_transition` /
`set_propagated_clock` / `set_clock_groups` on `[get_clocks {}]` and on
`[get_clocks no_such]` — warn, silent no-op, or error.

**Implementation** follows the measured rule:

- If OpenSTA accepts an empty collection silently → emit
  `Value::Clocks(vec![])` for an empty `get_clocks` argument instead of
  `Opaque` (the empty *list* is a valid empty object collection; an
  empty argument list that is not a collection stays a shape error).
  `set_clock_*` arms keep iterating — zero rows is a no-op — and
  `set_propagated_clock`'s `names.is_empty()` bail is dropped for the
  collection path (a no-op empty application), keeping the bail for a
  missing argument.
- If OpenSTA warns → still emit `Value::Clocks(vec![])`, and count the
  command under `Sdc::ignored_commands` (the parse-side "accepted but
  unapplied" surface — `unresolved_exception_selectors` is eval-time and
  not reachable here). Update the `set_clock_*` docs to say an empty
  selection counts as an ignored command.
- If OpenSTA errors → keep the bail, but make the message name the
  empty collection rather than the generic shape error.

Note: `[get_clocks no_such]` already resolves to a `Clocks` collection
of one name; `resolve_clock` bails `unknown clock`. If OpenSTA warns
and continues there instead, the fix routes `set_clock_*` unknown-name
resolution through `clock_rows`-style filtering (warn + empty row set)
— record the difference in `docs/results.md`.

`get_ports {}`/`get_pins {}`/`get_cells {}`/`get_nets {}` analogues are
out of scope — their empty form already lands in `resolved_any=false`
→ `unresolved_exception_selectors`, which is a consistent named
diagnostic.

## A3 — pin `-from` overwrite semantics documented (`analysis.rs` ~L4019, `ResolvedPathDelay` ~L3559)

Measured rule (already in evidence, `md_overlap*`): within one
min/max kind, covering `set_max_delay`/`set_min_delay` rules overwrite
per-k last-wins — including `from_pins`. Today `delay_max_pins` /
`delay_min_pins` are folded the same way: the last covering rule's
`from_pins` **replaces wholesale**, so a pin `-from` rule followed by an
ordinary covering rule clears the startpoint set (and vice versa). The
`pin_from_sig_{min,max}` flag is independent: any signature-class rule
with `from_pins` sets it, and combined with `delay_max_n.is_none()` it
kills the clocked side entirely.

No behaviour change. Two comment fixes:

- At the `delay_max_pins`/`delay_min_pins` declaration (~L4019): state
  that the fold is last-wins wholesale — a later covering rule's
  `from_pins` (including `None`) replaces the previous set.
- On `ResolvedPathDelay::from_pins` (~L3562): same statement — the
  recorded pin set is the last covering rule's, per `delay_min`/
  `delay_max` folding.

## A4 — PROBE_ONLY asserts the real parse bail (`analysis.rs` ~L9399)

Current:

```rust
for name in ["t2_grpn", "t2_wave3", "t2_wave4", "t2_waverev"] {
    assert!(
        load_sdc(&dir.join(format!("{name}.sdc"))).is_err(),
        "{name}: PROBE_ONLY form must stay a named bail"
    );
}
```

A missing `.sdc` file satisfies `.is_err()`. Change to a per-name
expected-substring map:

| scenario | expected bail substring |
|---|---|
| `t2_grpn` | `set_clock_groups needs -asynchronous, -logically_exclusive or -physically_exclusive` |
| `t2_wave3` | `create_clock -waveform takes an even number of edge instants` |
| `t2_wave4` | `create_clock -waveform accepts at most two instants here` |
| `t2_waverev` | `create_clock -waveform instants must be strictly increasing` |

Assert `format!("{err:#}").contains(expected)` (anyhow's alternate
format carries the full context chain, so the `SDC line N` wrapper is
included and the root cause matches). A missing file yields a
`Failed to read SDC`/`No such file` chain — fails the assert loudly
under the repo's no-silent-skip rule. If item A5 changes a bail message
(e.g. post-wrap validation reuses the "strictly increasing" wording),
update the expected substring in the same change.

## A5 — `-waveform` post-wrap validation (`sdc.rs` ~L1041–1075)

Current: raw instants are checked strictly increasing, then each is
`rem_euclid(period_ns)`-wrapped and the pair stored. Post-wrap the pair
can reorder (`{1.9 2.1}` on 2.0 → `[1.9, 0.1]`) or collapse
(`{0 2.0}` → `[0, 0]`). The strict-increase check runs pre-wrap only.

**Probe** (`t2_waverot` `{1.9 2.1}`, `t2_wavez` `{0 2.0}`, `probes.md`
P5): does OpenSTA accept, warn, or error — and if it accepts, what edge
instants result (infer from a clock-anchored reported time).

**Implementation** per the measured rule:

- OpenSTA wraps and keeps the pair as-is → keep `rem_euclid`; record
  that `edge_offset(0) > edge_offset(1)` is then a legal state (rise
  later than fall within the period — an 80%-duty phase-shifted clock
  is still a coherent lattice), add a unit test pinning the stored
  pair.
- OpenSTA errors (STA-0372-class on the wrapped form) → validate the
  wrapped pair: `wrapped[1] > wrapped[0]` strictly, bail otherwise with
  a message naming the post-wrap collapse/reorder.
- OpenSTA sorts the wrapped instants → store `wrapped.sort()` — but
  prefer the bail/keep options unless the probe clearly shows sorting;
  sorting changes the rise/fall assignment and is the least likely
  model.

Either way the pre-wrap strict-increase check stays (it is measured
STA-0372 behaviour for raw instants).

## A6 — stale names and comments (`analysis.rs`)

1. `sta_evaluates_two_clock_same_domain` — already renamed to
   `sta_evaluates_two_clock` (v0.2.3); the function asserts cross-domain
   pairing. Audit `rg same_domain` in `analysis.rs`/`docs/` and fix any
   residual misnomer; likely no-op — record honestly.
2. Hold-pairing comment ~L4597–4601: the `xdomain_defer` re-diagnosis
   lists `"non-coincident hold parity"` as a defer cause — stale since
   `t2_wave`: hold pairing is now min non-negative `launch − capture`
   gap over the LCM window (`hold_edge_pair` ~L5162), and the actual
   min-side defer is "a foreign `(domain, parity)` candidate with no
   non-negative capture edge at-or-before a launch edge". Restate the
   comment to match the code. Grep `coincident` for other stale claims
   in the same file; leave statements that are still true (e.g. the
   virtual-clock equal-period comment at ~L5057 — equal grids *are*
   coincident).
3. `SkippedEndpoint` struct doc (~L353): extend the count list to
   `incomplete_checks` + `unreached_endpoints` + `cross_domain_endpoints`
   (matching the `cross_domain_endpoints` field doc at ~L304 and the
   `cross_domain_endpoints += 1` site at ~L4606).

## A7 — `-to <resolved-but-invalid>` warn/count (`analysis.rs` `resolve_to` ~L4700)

Measured today (`t2_selpto`): `-to clk2` resolves the port object, the
rule stays dead, OpenSTA warns `STA-1551 'clk2' is not a valid
endpoint`; ours is silent. `resolve_to`'s `ObjSel::Names` arm returns
`resolved_any = true` on the port match, so nothing is counted.

**Probe** (`t2_toinv`, `probes.md` P7): which resolved-but-invalid `-to`
objects warn — input port (`clk2`, `d_in`), `get_pins` on a
non-endpoint pin (`u_fa0/Q`, `u_ba/A`), `get_cells` with no endpoint
pins, `get_ports` collection form — and does the warning repeat per
name/object.

**Implementation** per the measured surface:

- If OpenSTA warns per resolved-but-invalid `-to` object → count each
  such object in `SkipReport::unresolved_exception_selectors`
  (`resolve` already counts zero-resolved sides; extend the `resolve_to`
  `ObjSel::Names`/`Ports` arms so a match that is resolved-but-not-an-
  endpoint also increments `unresolved`). Update the field doc to
  "selector sides that resolved to no launch source or no endpoint —
  plus `-to` objects that resolve but cannot be an endpoint (OpenSTA
  `STA-1551`)".
- `-from clk1` stays silent-dead (OpenSTA warns nothing there —
  `t2_selport`).
- If the probe shows only *port* objects warn (pins/cells stay silent),
  scope the counting to port matches on `-to`.
- Diagnostic-only: the endpoint table does not change → goldens stay
  byte-identical; `--strict` picks up the newly counted side, matching
  OpenSTA's warn posture.

## A8 — port-vs-clock namespace priority (`analysis.rs` `resolve_from`/`resolve_to`/`resolve_delay_from`)

With a `-name`d clock, `resolve_clock` is name-only and a bare
`-from clk1` resolves to the *port* object (measured). With an
**unnamed** clock — `create_clock -period 2.0 [get_ports clk1]` gives the
clock the name `clk1` — a bare `-from clk1` collides: our `ObjSel::Names`
arms *union* the port match and the `clock_row(name)` match. OpenSTA
resolves one object.

**Probe** (`t2_nsfrom`, `t2_nsto`, `probes.md` P8): unnamed clocks on
`two_clock.def`, `set_false_path -from clk1` and `-to clk2` — which
object wins; `[get_clocks clk1]` as the unambiguous control.

**Implementation** per the measured priority (one of):

- Port wins → keep port resolution first and *drop* the `clock_row`
  union when the name matched an io pin (and the clock-name path only
  fires when no object matched).
- Clock wins → `clock_row(name)` short-circuits before the io-pin scan
  in the `-from`/`-to` names arms (bare `-from clk1` then behaves like
  `[get_clocks clk1]`).

Apply the same priority to all three resolver arms (`resolve_from`,
`resolve_to`, `resolve_delay_from`) so the rule is one sentence in
`docs/multi-clock.md` §1.7. This is a namespace-priority fix, not a
namespace broadening: no name resolves in *more* places than OpenSTA's.

## A9 — `-clock`-less `set_input_delay` under `-waveform` (`sdc.rs` `input_clock_row` ~L543, `analysis.rs` ~L2449)

Current: a `set_input_delay` without `-clock` resolves through
`DelaySpec::no_clock()` → `input_clock_row` falls back to
`design_row()` (first `port.is_some()` row) → `launch_edge =
clocks[row].edge_offset(0)`, i.e. the design clock's *rise* instant.
Under `-waveform {0.3 1.7}` that is `0.3`; OpenSTA's anchor for a
`-clock`-less input delay is unmeasured.

**Probe** (`t2_noclk`, `probes.md` P9): `cka` on `-waveform {0.3 1.7}`,
`set_input_delay -max 0.7 [get_ports d_in]` without `-clock`; read the
`d_in` launch arrival off `u_fb0/D` — discriminates `0.0` vs `0.3` (vs
fall `1.7`). Mirror for `-clock`-less `set_output_delay` (`ya` required
time anchor).

**Implementation** per the measured instant:

- `edge_offset(0)` (rise) → no code change; record the measurement.
- `0.0` (unclocked/reference instant) → the fallback path needs its own
  edge: split `input_clock_row`'s no-clock fallback from the launch-edge
  accessor — e.g. the port-seed edge becomes `0.0` (and the output-side
  anchor `0.0 + period`) when the covering rule names no clock, while
  `launch_domain`/uncertainty/latency keep reading the `design_row()`
  row (those are per-row attributes, not edge instants — confirm in the
  probe whether latency still applies).

## A10 — SAIF nits (`readers/saif.rs`)

Three small fixes, all in the parser; the measured field mapping is
unchanged.

1. **Fractional `DURATION`**: keep rejecting — the grammar's `<int>`
   reading and OpenSTA's own rejection (`DURATION 1990.0` probe) are the
   measured rule. Fix the *error path*: `v.parse::<u64>()` on `"1990.0"`
   reports `bad SAIF DURATION` indistinguishably from a non-numeric
   token. Parse via `f64` first and bail with `SAIF DURATION must be an
   integer, found \`{v}\`` (or keep `u64` parse and word the context as
   integer-required). Same treatment for the `(TIMESCALE 100 ns)` value.
2. **Negative-value mislabel** (~L229): the combined
   `v.fract() != 0.0 || v < 0.0` bail prints `fractional` for `-3`.
   Split: `v < 0.0` → `negative SAIF {key} value \`{v}\``; else
   `v.fract() != 0.0` → `non-integral SAIF {key} value \`{v}\``.
3. **Latin-1 tokenizer** (`Atom`/`next_atom`): `s.push(bytes[i] as char)`
   widens each byte to a Latin-1 char, corrupting multibyte UTF-8 names
   (and splitting escapes). Accumulate `Vec<u8>` instead and
   `String::from_utf8` at atom close — the input `&str` is valid UTF-8
   and every byte of a sequence is pushed contiguously, so the only
   failure path is a `\`-escaped continuation byte, which should bail
   with context rather than silently mangle. If the probe-side argument
   is "document as intended", the Vec<u8> fix is still the right call —
   it is strictly more correct for the same cost and no semantic change
   for ASCII fixtures.

Unit tests: a `1990.0`/`"abc"`/`"-5"` DURATION battery pinning the three
messages, a negative `T0`, and a UTF-8 net name round-trip.

## Golden-impact analysis

| Change | Endpoint table | Skip counts | Parse surface |
|---|---|---|---|
| A1 | — | — | — |
| A2 | — | `ignored_commands` may +1 where OpenSTA warns | parse may accept what was a shape bail, or bail where it was silent |
| A3/A6 | — | — | — |
| A4 | — | — | — |
| A5 | — | — | `t2_eclk`-style SDCs only; existing `.sdc` inputs unchanged |
| A7 | — | `unresolved_exception_selectors` may +1 on `-to` invalid objects | — |
| A8 | changes only when a bare `-from`/`-to` name collides with an unnamed clock's name | — | — |
| A9 | `d_in`-style arrivals under `-waveform` only if measured ≠ `edge_offset(0)` | — | — |
| A10 | — | — | parse errors only on previously-mangled or mislabelled inputs |

No existing fixture exercises A8's collision (all `t2_*` SDCs use `-name`d
clocks) or A7's counted form (`t2_selpto` asserts nothing about skip
counts — its endpoint table is unchanged). `test_sta_clocks_match_opensta`
compares endpoint rows only, so goldens stay byte-identical unless a probe
forces an endpoint-visible change — and any such case is itself the
measured requirement.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/sdc.rs`
- `crates/grus-core/src/readers/saif.rs`
- `crates/grus-core/src/power.rs` (A6-adjacent doc only — see `docs.md` D4)
- `tests/designs/sta_clocks/probe.sh`
- `tests/designs/sta_clocks/t2_*.sdc` (new probe scenarios — `probes.md`)
- `tests/designs/sta_clocks/probe_t2_*.txt` (new outputs)
- `docs/timing-exceptions.md`
- `docs/results.md`
- `docs/multi-clock.md`
- `tests/designs/README.md`
- `Cargo.toml`
- `Cargo.lock`
