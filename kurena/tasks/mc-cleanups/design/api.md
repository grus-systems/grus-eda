# mc-cleanups — Code changes (revision 2)

The A-items and D-items of the original design are **landed** in
`e6bd1a7` (see the 2026-11-20 `docs/results.md` entry for the measured
rules and the per-item list). This file specifies the review-memo
follow-up work R1–R5. `analysis.rs` is
`crates/grus-core/src/sta/analysis.rs`, `sdc.rs` is
`crates/grus-core/src/sta/sdc.rs`.

## R1 — per-corner `-clock` anchor in `delay_at` (medium)

> **Outcome (implementation)**: the probe refuted the per-corner
> hypothesis and confirmed the *merged entries* reading — OpenSTA keeps
> a port's delay rules as an entry list. Implemented: `delay_at` folds
> rules into `Vec<DelayEntry>`; a plain rule prunes entries anchored to
> a different `-clock` **port-wide** then appends, `-add_delay` appends
> without pruning, and each surviving anchored entry produces its own
> check (the reported row is the last anchor's, wns/tns/violations see
> the worst). `t2_clkcor` (`-max @cka` then bare `-min`) declocks the
> port on both corners; `t2_clkcor2`'s `yb` checks under both `cka`
> and `ckb`. Evidence and verbatim probe lines are in the
> 2026-11-20 `docs/results.md` revision entry.

### The bug

`sdc.rs:370` — `delay_at` folds matching `DelayRule`s into
`(min_v, max_v, clock)` and assigns `clock = r.clock` on **every**
matching rule regardless of `r.sense`. Sequence `set_input_delay -max
0.7 -clock cka` then `set_input_delay -min 0.1` (no `-clock`): the `-min`
rule's `clock = None` becomes the port's anchor, so
`declared_input_clock_row` returns `None` and the port is declocked for
**both** corners — `d_in` never launches even on the max side that the
`-max -clock cka` rule covers.

### Measured rule to pin (probe P1, `t2_clkcor`/`t2_clkcor2`)

Three hypotheses; the memo's "likely" and the model's shape both point
at per-corner:

- **Per-corner anchor**: each corner's anchor is the `-clock` of the
  last rule that wrote that corner (`Both` writes both). A `-min` rule
  without `-clock` clears only the min anchor.
- **Port-wide last-rule**: the port's anchor is the last matching
  rule's clock for both corners (current code — refuted if the probe
  shows `u_fa0/D` in the max table).
- **Merged entries** (`-add_delay` case): OpenSTA keeps each delay
  entry with its own clock and merges arrivals — approximated, not
  fully modelled (below).

### Implementation (per-corner branch)

**`sdc.rs`:**

1. `delay_at` returns `(min_v, max_v, min_clock, max_clock)`:
   ```rust
   match r.sense {
       DelaySense::Both => { apply(min); apply(max); min_clk = r.clock; max_clk = r.clock; }
       DelaySense::Min  => { apply(min); min_clk = r.clock; }
       DelaySense::Max  => { apply(max); max_clk = r.clock; }
   }
   ```
   `-add_delay` interaction: when `r.add` widens, the anchor follows the
   *winning* value — `widen(*cur, r.value_ns)` returns `*cur` → the
   previous anchor stands; returns `r.value_ns` → `r.clock` anchors.
   (If `t2_clkcor2`'s `yb` discriminator shows last-wins-anchor or
   merged entries instead, follow the measured rule — merged entries is
   approximated as winner-anchors and the residual divergence is
   recorded; do not grow a per-entry arrival model in this ticket.)
2. Doc updates: `delay_at`'s doc gets the per-corner anchor rule;
   `DelayRule::clock`'s doc ("`None` is the design clock") is stale
   since A9 — `None` is *no declared anchor*; the `design_row()`
   fallback is bookkeeping only. Restate.
3. Accessors become per-corner:
   - `declared_input_clock_row(port)` /
     `declared_output_clock_row(port)` →
     `declared_input_clock_rows(port)` /
     `declared_output_clock_rows(port) -> [Option<usize>; 2]`
     (`[min, max]`).
   - `input_clock_row` / `output_clock_row` gain a corner parameter
     (reuse `DelaySense` restricted to `Min`/`Max`, or a `bool` —
     pick one and apply uniformly): `usize`, declared anchor else
     `design_row()` bookkeeping fallback. Call sites ~L4185, ~L4343,
     ~L4460/4488, ~L4866 are each already corner-scoped.
   - `input_clock_latency_ns` / `output_clock_latency_ns` → per-corner
     (`(f32, f32)` pair or corner param — match the row accessors'
     shape).

**`analysis.rs`:**

4. `PortSeed` (~L2244): `dom: u8` → `dom: [u8; 2]` (`[max, min]`), and
   `per_pol` entries are computed per corner — each corner's
   `latency`, `edge_offset(0)` and delay value come from its own
   anchor. An unanchored corner stores `arr_max = NEG_INFINITY` /
   `arr_min = INFINITY` and `dom = u8::MAX`.
   - `dom_bit(tag >= 8)` already yields `0` (`analysis.rs:5128`) — no
     change needed there.
   - The `state_ports` slew-seed merge (~L2466) keeps both corners'
     values — suppressed ±∞ arrivals never win the merge; slew fields
     are what `propagate_slew` reads.
   - Gate (~L2488): `port_seeds.push` iff `declared.min.is_some() ||
     declared.max.is_some()`; otherwise the existing `slew_seeds` path.
5. `eval_sta` seed merge (~L2837): the `ok`-branch merge gains
   per-corner masks — `par_*`/`crpr_*`/`dom_*` args for an unanchored
   corner pass `0` instead of the existing constants; `dom_bit(seed.dom[i])`
   per corner. The `!ok` (filtered) branch is unchanged.
6. `key_dom` (~L3798) and `ep_cap` (~L3855): a port now declares up to
   two distinct anchor rows. Make both maps `HashMap<u32, Vec<usize>>`
   (dedup'd declared rows) and `keys_on_clocks` / the `ep_cap` lookup
   match on **intersection** — a `-from`/`-to` clock selector matches a
   port when *any* declared corner's row is in the set. The cut itself
   stays pin-granular (a `-from cka` cut removes the port's min-side
   launch even if it anchors to `ckb`): pin-level granularity is the
   model's floor — record the approximation in `docs/multi-clock.md`.
   When anchors agree (every existing fixture) this is identical to
   today.
7. `collect_endpoints` output-port loop (~L5949): restructure to
   per-corner —
   - `(min_row, max_row) = sdc.declared_output_clock_rows(port)`;
     both `None` → `continue` (unchanged `t2_noclk` gate).
   - Per declared corner: `out_row`, `out_clk`, `cap_tag =
     launch_domain(row)`, `latency` are read from *that corner's* row;
     the corner's candidate loop (existing `max_pick`/`min_pick`
     bodies, moved inside a per-corner block) uses them. `min_pick` /
     `max_pick` `.unwrap()` calls are guarded by the corner's
     declared-ness and `s.reached`.
   - `xdomain_defer` runs per corner on that corner's `cap_tag`/
     `out_clk`. A port whose candidates all vanish to deferral counts
     `cross_domain_endpoints += 1` once and gets its `SkippedEndpoint`
     row; a partially-deferred port emits the surviving corner only —
     not counted (endpoint exists). Record the choice.
8. `EndpointReport` (~L221): all eight `*_*_ns` fields → `Option<f32>`.
   A corner with no anchor produces no candidates and serializes as
   `null` — matching the hardened Tcl collector's `null` fields.
   - `endpoint_report(pin, max, min)` takes `Option<&Worst>` per
     corner.
   - `record_endpoint` (~L4944): emit when `pick_worst_polarity` is
     `Some` on **either** corner — today it requires both `(Some,
     Some)` and otherwise counts `unreached_endpoints`. The relaxed
     rule only fires where a corner truly has no candidates; every
     committed golden endpoint has both corners, so existing
     comparisons are unaffected (verify via the test).
   - `merge_corner_reports` (~L6155): endpoint-set equality holds (a
     split-anchor port appears in both lib-corner reports with the same
     missing corner). Field merges become `Option` copies;
     `wns`/`tns`/violation folds use `flatten`/`filter_map` —
     `is_some_and(|s| s < 0.0)` for the violation counts.
   - `parasitics.rs` test asserts (~L1097–1111): compare `Option`
     fields directly.
   - `EndpointReport` is `Serialize`d into the `--sta-json` output —
     corners become `null` there; record the schema change in
     `docs/results.md`.
9. `test_sta_clocks_match_opensta` (~L9322): golden entries can now
   carry `null` corner fields. Rework the per-endpoint loop:
   - Expected corner: `r["max_arr_ns"].is_null()` → expected `None`.
   - Ours: `pick_worst_polarity(&ep.max)` / `&ep.min` → `Option`.
   - `null` ↔ `None` must match both directions; for `Some` corners
     compare the four fields at existing tolerances. The `resolve_tied_
     polarity` path applies only inside a present corner.
   - `evaluated == 45`.
10. `sta_clocks_measure_only_scenarios` test (~L9518): no signature
    change needed; optionally add `pins` asserts for the promoted
    scenarios — not required since the golden covers them.

### Fallback branches

- Probe shows **port-wide last-rule** (our current behaviour): no code
  change; record "current behaviour matches" and keep the
  `EndpointReport`/collector hardening anyway (it is a latent crash,
  independently of the anchor rule).
- Probe shows **merged `-add_delay` entries**: implement the
  per-corner winner-anchors model for the replace path and record the
  residual approximation for the `-add_delay` mix — a per-entry arrival
  model is out of scope.

### Tests

- `sdc.rs` unit tests for the fold: mixed `-clock`/bare sequences both
  orders on input and output rules; `-add_delay` winner-anchor case —
  assert `declared_input_clock_rows` per corner.
- Scenario coverage via the promoted `t2_clkcor`/`t2_clkcor2` golden
  blocks.
- Fail-proof: revert `clock` fold to port-wide → `t2_clkcor` goes red
  on the `u_fa0/D`/`ya` corner-null cells.

## R2 — `[get_clocks]` no-arg committed evidence

No code change expected — `get_clocks` already evaluates `[]` to
`Value::Clocks(self.all_clock_names())` (`sdc.rs:898–904`) and
`t2_eclk2` (probe P3) either confirms it or exposes a real divergence
to fix. The `(measured)` comment at `sdc.rs:893` becomes honest —
extend it to cite `t2_eclk2`. If the probe surprises (e.g. OpenSTA
treats no-arg as an error), implement the measured surface in the same
commit.

## R3 — `t2_waverot`/`t2_wavez` in `SCENARIOS`

`probe.sh` list move only (probe P4). `probe_t2_*.txt` regenerate
identically (the report content does not change); `ref_t2_*.json`
blocks fold into the golden. If `t2_wavez` diverges in our eval —
`[0,0]` collapsed duty is the risky lattice — small fix in scope, else
demote and record (probe P4 fallback).

## R4 — `t2_eclk` overclaim wording

1. Extend `t2_eclk.sdc` per probe P2 so every form the code cites is
   actually exercised.
2. `sdc.rs` comment audit — every "`t2_eclk`"/"`t2_eclk` family"
   citation (~L1147 `set_clock_groups` comment, ~L1271/1273
   `set_clock_uncertainty` `-from`/`-to`, ~L1301/1302 positional,
   ~L1374/1375 `set_clock_latency`, ~L1401–1403 `propagated_clock`)
   must name a line the file now contains; reword any claim the probe
   still does not pin to "posture follows the measured
   `set_clock_latency` case".
3. `docs/results.md` `t2_eclk` bullet rewording — per-command claims:
   `set_clock_latency`/`set_clock_uncertainty`/`set_clock_transition`
   are table-visible (latency shifts arrivals, uncertainty shifts req,
   transition shifts clock-pin slews); `set_propagated_clock`'s
   zero-row application is evidenced by "no warning, no
   propagated-mode clock lines" only — say exactly that. Remove the
   "(ad-hoc stdin check in the same container)" parenthetical; the
   no-arg rule now cites `t2_eclk2`.

## R5 — `set_propagated_clock` non-clock-name drop

The `literal` flag in `propagated_clock` (`sdc.rs:1404–1447`) is new in
this ticket: collection-sourced names that resolve to neither a clock
`-name` nor a clock's source port drop silently (`None if !literal =>
continue`), while literal names keep the `unknown clock` bail.
`set_propagated_clock [get_clocks no_such]` in the extended `t2_eclk`
pins the posture (expect the `STA-0351` warn + continue same as
`set_clock_latency`). Record explicitly in `docs/results.md` and the
§1.7 empty-collections bullet of `docs/multi-clock.md`: the drop, the
measured evidence, and that OpenSTA's warning itself is unmodelled.

## Golden-impact analysis (revision)

| Change | Endpoint table | Golden diff |
|---|---|---|
| R1 fold | `t2_clkcor*` only — every existing SDC uses `-clock` on all rules → identical folds | append-only blocks |
| corner-optional `EndpointReport` | emits endpoints unreachable corners used to suppress — unreachable only in scenarios that produce partial corners (none committed until `t2_clkcor*`) | none on existing blocks |
| R2/R3 promotions | — | append-only blocks |
| collector union + `null` | — | none (all existing blocks have both corners) |

`record_endpoint`'s relaxed emit rule is the only behaviour change that
could touch an existing scenario — the suite verifies none does (all 39
existing blocks keep byte-identical entries).

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/sdc.rs`
- `crates/grus-core/src/sta/parasitics.rs` (test asserts)
- `tests/designs/sta_clocks/probe.sh`
- `tests/designs/sta_clocks/t2_*.sdc` (new + extended)
- `tests/designs/sta_clocks/probe_t2_*.txt`
- `tests/designs/sta_clocks/two_clock.reference.json`
- `docs/results.md`
- `docs/multi-clock.md`
- `docs/timing-exceptions.md`
- `tests/designs/README.md`
- `Cargo.toml`
- `Cargo.lock`
