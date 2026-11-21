# AP4 — `user` overrides, final precedence, gcd re-comparison: overview

Closing ticket of the AP1–AP4 sequence
(`kurena/tasks/activity-propagation/design/api.md` §Plan; measured
semantics: `docs/results.md` 2026-11-21 N7 probe; golden:
`tests/designs/microbench/mba.activity.reference.json`, 12 sessions ×
43 pins, already committed).

AP1–AP3 landed the resolution skeleton and measured defaults
(v0.2.14), combinational propagation (v0.2.15), 3-valued constant
marking (v0.2.16), and sequential Q=D + `unknown` (v0.2.17). All 12
golden sessions already compare strict per-pin — with one tolerated
indirection: the golden's `user` source (`s_input_ports` `a`/`b`) is
collapsed onto `ActivityOrigin::Annotated` in both code and test
mapping. AP4 makes `user` a first-class origin, adds the `-pins`
per-pin override arm (the last measured `set_power_activity` surface),
and lands the end-to-end proof on a real design: a gcd `report_power`
re-comparison against the OpenSTA numbers recorded on 2026-11-18.

## What lands

1. **`ActivityOrigin::User`** — `ActivityOverrides::input_ports` stops
   emitting `Annotated`; the golden `user` label becomes a native
   strict compare instead of a test-side collapse. `ActivityOverrides`
   gains `pins: Vec<(PinId, ActivitySetting)>` — the
   `set_power_activity -pins <list>` surface: per-pin overrides,
   origin `User`, resolved against the net's driver pin exactly like a
   dump annotation (a `pins` entry whose pin is not a net driver is a
   dead `user` entry, counted). Both `user` arms sit in the seed chain
   below `global`/`clock`/`constant` and above dump annotation — the
   measured precedence (api.md §3).
2. **Call-site `-activity` normalization, pinned** — the API keeps its
   absolute-rate contract (`ActivitySetting::density_per_s`); `-activity
   a` → `a × f_clk` is the caller's arithmetic, `-density d` → `d ×
   1e9`. `s_p40` (period-40 SDC, committed in AP3) pins the
   clock-normalized *default* inside `resolve_activity`: `input` seed
   `2.5e6`, `clock` `5e7` — verbatim already; AP4 keeps it and
   records the contract.
3. **Golden coverage** — `s_input_ports` and `s_p40` per-pin
   `{d, P, source}` verbatim (both were pinned in the golden since the
   generation pass; `s_input_ports` now asserts `user` natively, not
   via collapse). `s_input_ports` `origin_counts` pinned at
   `{clock 1, constant 4, input 1, propagated 6, unknown 4, user 2}`.
   New `pins`≡`input_ports` equivalence assert pins the `-pins` arm
   against the measured session transitively (golden.md).
   `dead_user_entries` asserted 0 on every session. All 12 sessions
   still match; fail-proof recorded for the `user` arm, the `pins`
   arm, and a corrupted `s_p40`.
4. **gcd `report_power` re-comparison** — `grus-eda power` on
   `gcd.placed.def` + `gcd.sdc` + `gcd.vcd` (the same input set the
   committed `power_vcd_check.tcl` feeds OpenSTA) now resolves activity
   with the propagated model instead of exact per-net counts.
   `docs/results.md` gets a new N7 entry: old ours / new ours /
   OpenSTA columns, which columns moved toward OpenSTA, which still
   diverge, named residuals (gcd.md). A no-annotation run is recorded
   alongside for the record.
5. **Roadmap + docs** — `docs/roadmap.md` N7 rewritten: AP1–AP4 done,
   the "OpenSTA propagates statistically, ours reads exact counts"
   intentional-divergence paragraph replaced by the now-measured
   model, residual notes enumerated (`docs.md`).

## What does not land

- **Per-pin activity surface.** The model stays per-net; a `user`
  (or `vcd`) annotation on a non-driver pin has no representable
  effect. `s_full` output port `y` remains the one per-pin residual
  (`PER_PIN_RESIDUAL` stays `[("s_full", "y")]`), as does the
  sink-pin-on-annotated-net label convention (golden.md §labels).
- **CLI flags** for `-global`/`-input`/`-input_ports`/`-pins` — API
  only (spec calls CLI optional). `main.rs` only gains `"user"` in
  `ORIGIN_ORDER` so the diagnostics split line can't silently drop a
  user-counted net if a flag ever lands.
- `unset_power_activity`, `read_power_activities`, `-duty` on dump
  annotations — not part of the measured set.
- OpenSTA's `report_activity_annotation` `input N` bucket counting
  (its split counts `user` pins under `input`) — our `origin_counts`
  keeps its own per-net shape (recorded difference, unchanged).
- Sequential expression-form `next_state`/`data_in` (26 cells) — the
  AP3 census residual, untouched.

## Decisions recorded

- **`pins` keyed by `PinId`, not name.** The call site resolves
  Tcl-style names to pins — the same "normalization at the call site"
  contract as `-activity` → absolute rate. The mba test's existing
  `pin_obj` map provides the lookup; a name-resolving helper is a
  later convenience, not this ticket.
- **`Vec<(PinId, ActivitySetting)>`, last wins** on duplicate pins —
  mirrors repeated `set_power_activity` commands (unmeasured,
  recorded).
- **`pins` checked before `input_ports`** — the specific selector
  beats the `[all_inputs]` blanket when both name an input port
  (unmeasured — no committed session mixes them; recorded).
- **`user` beats a dump annotation** on the same net — the AP3 chosen
  order restated (unmeasured); loses to `-global`, `clock`,
  `constant` (measured: the p0b side-probe and `s_input_ports`'s
  `clk`).
- **`ActivityOverrides` loses `Copy`** — the `Vec` field forces it.
  Public struct → breaking API change → version bumps **minor** to
  `0.3.0` per the workspace 0.x versioning rule (recorded so the bump
  is deliberate, not reflexive patch).
- **gcd comparison input**: `--vcd gcd.vcd` is the apples-to-apples
  run — same annotation seeds as the committed OpenSTA reference Tcl;
  a no-annotation run is the secondary line (gcd.md).

## Files

- `crates/grus-core/src/activity.rs` — `User` origin, `pins` arm,
  `dead_user_entries`, doc updates
- `crates/grus-core/src/power.rs` — test rework (golden.md), header
  fill-order comment
- `crates/grus-eda/src/main.rs` — `ORIGIN_ORDER` gains `"user"` (no
  new flags)
- `docs/results.md`, `docs/roadmap.md`
- `Cargo.toml` (0.2.17 → 0.3.0), `Cargo.lock`
- `kurena/tasks/ap4-activity-precedence/design/` — this document set
