# mc-selectors — Overview

Fourth ticket of the multi-clock sequence (`docs/multi-clock.md` §3).
`mc-model`/`mc-eval`/`mc-xdomain` landed the clock table, domain-tagged
arrivals and measured cross-domain pairing. This ticket closes the
remaining unsupported SDC surface on the committed `sta_clocks`
fixture:

1. **Clock-object selectors in exceptions** (`probe.md` §1). Today
   `obj_sel` maps `Value::Clocks` to `None`, so `set_false_path` /
   `set_multicycle_path` / `set_max_delay` / `set_min_delay` with
   `-from`/`-to [get_clocks <name>]` land in `ignored_commands`. The
   measured rule (to be re-probed and recorded before coding): a clock
   selector anchors on the clocked object, not on the clock pin — `-from
   [get_clocks cka]` covers every launch key tagged to `cka` (registers
   whose clock pin `cka` reaches, input ports whose `set_input_delay
   -clock` resolves to `cka`); `-to [get_clocks ckb]` covers every
   endpoint whose capture is `ckb` (check/gate clock pins on `ckb`,
   output ports whose `set_output_delay -clock` resolves to `ckb`).
2. **`set_clock_groups -asynchronous -group A -group B`** (`probe.md`
   §2). Measured on `t2_grp` (committed): every cross-domain check
   disappears — the endpoint table shrinks 8→5 including I/O-anchored
   cross checks — while same-domain and same-domain-anchored I/O are
   untouched. Eval model: the command desugars into pairwise
   domain-keyed false-path rules inside the exception partition, so
   signature classes, precedence and skip accounting come free
   (`api.md` §3). `-logically_exclusive` / `-physically_exclusive` share
   the eval path or bail with a named error, per the probe.
3. **`create_clock -waveform {rise fall}`** (`probe.md` §3). The edge
   lattice becomes the waveform's two instants repeating per period —
   one scalar per parity replaces `parity · P/2` in the seed, pairing
   and phase-comparison code (`api.md` §4). Non-50% duty: `{0 0.5}` and
   `{0.3 1.7}` on a 2.0 ns clock, golden-covered.
4. **Golden coverage** on `sta_clocks`: `t2_grp` (verbatim shrink), the
   new selector scenarios, and the waveform scenarios — strict
   tolerances, committed artifacts, additive-only `probe.sh` regen.
5. Record in `docs/results.md`; tick `mc-selectors` in
   `docs/multi-clock.md` §3; version bump `0.2.3 → 0.2.4`.

## Approach sketch

- `ObjSel::Clocks(Vec<usize>)` — clock rows resolved at parse via the
  existing `resolve_clock` (unknown name still bails `unknown clock`,
  consistent with `set_clock_latency` and friends). `ExceptionCtx`
  gains two maps built once in `run_sta_partitioned`: `key_dom` (launch
  key → its clock row) and `ep_cap` (endpoint pin → its capture row).
  `resolve_from`/`resolve_delay_from`/`resolve_to` answer `ObjSel::Clocks`
  through them (`api.md` §1–§2).
- `Sdc::clock_groups` — parsed `(kind, groups, order)` triples.
  `run_sta_partitioned` synthesizes, per unordered group pair, two
  `ResolvedRule::Exception(FalsePath)` entries (`from` = launch keys of
  one domain, `to` = endpoints of the other, and mirror), inserted at
  the command's recorded `order`. `run_sta_detail`'s partition gate adds
  `|| !sdc.clock_groups.is_empty()` (`api.md` §3). Nothing inside
  `collect_endpoints` or `xdomain_defer` changes for groups.
- `Clock::waveform_ns: Option<[f32; 2]>` + `edge_offset(parity)`
  accessor; every `parity · P/2` site moves to the accessor. Arrival-
  referenced consumers (latch windows, pulse widths, delay-check edge-0
  anchors, MCP period multiples) already carry the instants in the
  seeded arrivals — untouched (`api.md` §4–§5).

## Golden-identity invariants

Single-design-clock, no-group, no-waveform SDCs must stay byte-identical
by construction:

- `ObjSel::Clocks` only exists on commands that previously counted into
  `ignored_commands`; existing stored rules are untouched.
- Clock groups only act when `sdc.clock_groups` is non-empty; the
  partition gate and the synthesized rules are both gated on it.
- `edge_offset` reduces to `parity · P/2` when `waveform_ns` is `None`;
  the same-domain `c0 <= g`/`c0 > g`-style tests become offset
  comparisons that are equivalent on the uniform grid (both instants
  `0 < P/2`), so the arithmetic is identical on every existing SDC.

One intended non-golden-visible change: `t2_grp` now parses (it was a
loud `unsupported command` bail); its `multi_clock_bails` case and the
`-waveform` case move to positive parse tests.

## Out of scope

- `mc-golden`: promotion of `sta_clocks` to `two_clock.reference.json`.
- `-through` (and the other `*_UNSUPPORTED_FLAGS` qualifiers): still
  counted in `ignored_commands`; only `-from`/`-to` get clock selectors.
- `set_clock_latency -early`/`-late`, `create_clock -waveform` with
  other than two instants, `create_generated_clock`, unknown `-clock`
  names — all stay loud parse errors.
- Bare-name clock resolution in exceptions (`-from cka` without
  `get_clocks`): probed (`probe.md` §1); implemented only if OpenSTA is
  measured to treat the bare name as a clock object.
- Waveforms on *virtual* clocks shifting the I/O delay anchor beyond
  the rise instant, and `set_clock_groups` precedence against
  explicit `set_false_path`/delay checks when the probe shows OpenSTA
  does not treat groups as same-position false paths — both recorded
  as parked items if the probe does not pin them.

## Risks

- The desugar assumes `-asynchronous` behaves exactly like pairwise
  false paths *at the command's file position*; if the probe shows
  groups always win over (or lose to) explicit constraints regardless
  of order, the synthesized insertion point changes or a dedicated
  mechanism replaces it — `probe.md` §2 makes this a gate, not a guess.
- `resolve_from`/`resolve_to` clock coverage uses *raw* clock rows
  (port `-clock` row, `clock_domain`, `output_clock_row`), not folded
  `launch_domain` tags: a virtual row names exactly the objects clocked
  by it. If the probe shows OpenSTA folding virtual anchors onto design
  grids for selector coverage, the maps keep the raw rows anyway —
  matching folded tags would only matter for uncommitted fixtures.
- Waveform parity semantics: parity `p` still means "source edge index
  mod 2" and edges still alternate rise/fall. A `-waveform` whose
  instants reorder the rise/fall within the period (`{1.7 0.3}`) is
  probed; the offset machinery handles any ordered pair but the
  implementer must not claim coverage the fixture does not exercise.
- `probe.sh` regeneration is additive-only for existing artifacts —
  `git diff` must show the 16 committed scenario blocks and every
  existing `probe_t2_*.txt` byte-identical (AGENTS.md §7 rule 1).

## Files

- `crates/grus-core/src/sta/sdc.rs`
- `crates/grus-core/src/sta/analysis.rs`
- `tests/designs/sta_clocks/`
- `tests/designs/README.md`
- `docs/multi-clock.md`
- `docs/timing-exceptions.md`
- `docs/results.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
