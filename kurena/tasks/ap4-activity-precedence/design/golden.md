# golden — `mba` AP4 test design

`test_activity_defaults_mba_matches_opensta` (`power.rs` `#[cfg(test)]`)
stays a 12-session loop — the session table is unchanged
(`s_input_ports` and `s_p40` entered in AP3). AP4 changes the *label
machinery*: `user` becomes a native strict compare, the `pins` arm is
pinned by equivalence, and one `origin_counts` pin is added.

## Sessions — unchanged (12)

The AP3 table stands verbatim, including
`("s_input_ports", input_ports: {1e7, 0.4}, none, none)` and
`("s_p40", default, none, "mba_p40.sdc")`. No 13th session: the
`-pins` surface is covered by the equivalence check below instead of
a golden session (none exists — no committed session ran
`set_power_activity -pins`).

## Golden `source` → rule (changes **bold** vs AP3)

- `clock`, `global`, `constant`, `input` — unchanged strict.
- `vcd`, `saif` — strict `{d,P}` + `annotated` iff the pin is its
  net's driver, else `PER_PIN_RESIDUAL` — unchanged; `"user"` **leaves
  this arm**.
- **`user`** — **new arm**: strict `{d,P}` + label `"user"` iff the
  pin resolves its net; else `PER_PIN_RESIDUAL`. The only `user`
  golden pins are `s_input_ports`'s `a`/`b` — both drivers → strict.
- `propagated`, `unknown` — unchanged.

`expected_label` shrinks to the pass-through it mostly is (only the
`"vcd" | "saif"` → `"annotated"` collapse remains); the arm
dispatch gains `"user"` handling that mirrors the `vcd` arm but
asserts `ActivityOrigin::User`.

`PER_PIN_RESIDUAL` stays `[("s_full", "y")]` — output port `y` holds
per-pin `vcd 3e6/0.25` in the golden while net `y` resolves its driver
`u_xor/X` (`propagated`). Recorded permanently: **the per-pin vs
per-net model gap is a named residual, not a target** — OpenSTA reads
activity per pin; `report_power` consumes the net's driver-pin value,
so the residual can only ever affect the pin-level read, not a watt
column.

Related recorded convention: a golden `propagated` pin on a seeded
net (e.g. `s_ports` `u_df/D` on annotated net `a`; `u_or/A`/`u_or/B`
on `n_and`) reads `propagated` in OpenSTA — non-driver pins carry the
net's *propagated-to* label, not the driver's seed label. Our per-net
label is the driver's; the existing relaxed `propagated` arm
(`["propagated", "annotated", "input"]`) covers it. Named residual,
unchanged.

## `origin_counts` — second pinned session

Add an `s_input_ports` pin alongside the `s_none` one:

```
{clock 1, constant 4, input 1, propagated 6, unknown 4, user 2}
```

vs `s_none`'s `{clock 1, constant 4, input 3, propagated 6,
unknown 4}`: `a`/`b` move `input` → `user` (−2/+2); undriven `n_float`
keeps `input` — the measured `-input_ports` port-only semantics.

## `dead_user_entries` — asserted 0

Fold into the existing per-session diagnostics assert (extend the
tuple): `dead_user_entries == 0` on every session — no session sets
`pins`, and `input_ports` resolves through its own arm, never through
`pin_overrides`.

## `-pins` coverage — the equivalence check

`resolve_activity` is deterministic per `(design, sdc, vcd,
overrides)`; `pins` and `input_ports` naming the same input ports
must yield identical nets (measured: both are `user` on `a`/`b`,
`input` elsewhere). After the session loop, resolve `s_input_ports`
a second way:

```rust
let via_pins = resolve_activity(&design, &lib, &sdc, &VcdActivity::default(),
    &ActivityOverrides {
        pins: vec![(pin_obj["a"], setting(1e7, 0.4)),
                   (pin_obj["b"], setting(1e7, 0.4))],
        ..Default::default()
    });
```

and assert `via_pins.nets` equals the `s_input_ports` session's
`resolution.nets` verbatim (`prob_one`/`eff_toggles`/`origin` per
net — the `s_partial`≡`s_ports` compare is the template; same-window
comment applies: both runs share `window_s == 1.0`). This transitively
pins `-pins` against the measured `user` semantics — a `pins` arm
that mislabelled, misseeded, or leaked onto non-port nets would
diverge here.

Note: `pin_obj` holds `PinId`s already — the call-site name→pin
resolution is demonstrated in-test (`"a"`/`"b"` → `pin_obj`), which
is exactly the contract api.md §2 specifies.

## Fail-proof (record all three in the results entry)

- **`user` arm dropped** (`User` relabeled `Annotated`, or both `user`
  seed arms removed) → `s_input_ports` red: `a`'s strict source
  compare fails (`annotated`/`input` vs golden `user`). Restore, green.
- **`pins` arm dropped** (`pin_overrides` lookup removed) → the
  `pins`≡`input_ports` equivalence assert red (`a`/`b` nets fall to
  `input`/`propagated` instead of `user`). Restore, green.
- **`s_p40` corrupted** (one density in the golden's `s_p40` block
  e.g. `a` `2.5e6 → 3e6`) → strict red at the density bound; `git
  checkout --` restore byte-identical, green.

## Moved goldens

- `mba.activity.reference.json` — read-only input; `git diff` empty.
- `mb.power.json` — regenerated via the recorded CLI command;
  expected `git diff` **empty** (mb has no `user`/`pins` overrides —
  CLI passes `ActivityOverrides::default()`; the `User` origin is
  unreachable there).
- Every other `*.reference.json` — `git diff` empty, asserted in the
  entry.

## Files

- `crates/grus-core/src/power.rs` — `"user"` match arm +
  `expected_label` shrink, `s_input_ports` `origin_counts` pin,
  `dead_user_entries` assert, `pins` equivalence check, doc comment
- `tests/designs/microbench/mba.activity.reference.json` — read-only
- `tests/designs/microbench/mb.power.json` — regenerated, asserted
  byte-identical
