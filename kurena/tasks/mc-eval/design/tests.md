# mc-eval — Test design

All in `analysis.rs::tests`, next to `compare_sta_edge`. Tolerances:
`Tol::STRICT` (the committed 1%-rel / 5 ps bands). The probe JSON schema
is the same `two_clock.probe.json` shape the probe task emits —
deserialize it, do not hand-transcribe numbers.

## 1. `compare_sta_clocks` — the golden comparator

```rust
/// Multi-clock comparison: per-endpoint fields for the evaluable
/// (same-domain) pin set, plus pinned deferral assertions.
/// `wns`/`tns` are NOT compared — OpenSTA's aggregates include the
/// cross-domain paths we deliberately defer.
fn compare_sta_clocks(
    def: &str,          // "two_clock"
    scenario: &str,     // "t2_int"
    evaluable: &[&str], // pins expected in report.endpoints
    deferred: &[&str],  // pins expected skipped CrossDomain
);
```

Loads `tests/designs/sta_clocks/{def}.def` + `t2_<scenario>.sdc`, runs
`run_sta_detail`, loads `{def}.probe.json[scenario]`, then:

- `report.endpoints` contains exactly `evaluable` (set equality, count
  assertion — OpenSTA's table has the deferred pins too, so compare
  membership against our own list, not the JSON's).
- For each `evaluable` pin present in the JSON: strict compare of
  `max/min` `arr`/`req`/`slack`/`slew` — same field discipline as
  `compare_sta_edge`.
- For each `deferred` pin: absent from `report.endpoints`; present in
  `report.skips.skipped_endpoints` with `reason == CrossDomain`; the
  `related` string names the launch and capture clock names.
- `skips.cross_domain_endpoints == deferred.len()`;
  `skips.unreached_endpoints` unchanged from single-domain semantics.

## 2. Golden test matrix (`two_clock.def`)

| test | scenario | evaluable | deferred | what it discriminates |
|---|---|---|---|---|
| `two_clock_int` | `t2_int` | 5 same-domain | `u_fb0/D`, `u_fx/D`, `u_fy/D` | both domains seed; `Pb=1.0` vs `Pa=2.0` required times (`u_fb1/D` req 0.885446, not 1.885446) |
| `two_clock_frac` | `t2_frac` | 5 | 3 | non-integer `Pb=1.4` lattice on domain B |
| `two_clock_prop` | `t2_prop` | 5 | 3 | both domains propagated, per-row `propagated` |
| `two_clock_prop1` | `t2_prop1` | 5 | 3 | mixed: `cka` propagated + `ckb` ideal consumers |
| `two_clock_lat` | `t2_lat` | 5 | 3 | launch-side latency inside arrivals (`u_fb1/D` arr carries ckb's 0.2, not cka's 0.6+0.1) |
| `two_clock_unc` | `t2_unc` | 5 | 3 | capture-side per-row uncertainty (`u_fb1/D` req shifts by ckb's 0.20) |
| `two_clock_mcp` | `t2_mcp` | 5 | 3 | cross MCP deferral; same-domain rows unaffected |
| `two_clock_mcps` | `t2_mcps` | 5 | 3 | **same-domain MCP**: `u_fb1/D` req shifts `(2−1)*Pb` — capture-row period |
| `two_clock_md` | `t2_md` | 5 | 3 | delay-check-covered cross endpoints still defer (uncovered side can't pair); partitioned path survives |
| `two_clock_mdl` | `t2_mdl` | 5 | 3 | same + per-domain latency/uncertainty rows read |
| `two_clock_mdo` | `t2_mdo` | 5 | 3 | same pins, but `yb` now evaluates through the output-port `-to` delay-check branch on anchor `ckb` |
| `two_clock_latch` | `t2_latch` | all 4 endpoints + wns/tns/violations | 0 | borrow window on `ckb`'s lattice; full-table compare |

`two_clock_latch` uses the full comparator (its DEF has no cross paths):
endpoints + `wns`/`tns` + `violations` + pulse rows — same shape as
`compare_sta_edge`.

## 3. Parse-bail tests (unchanged surface, new assertions)

`t2_uncx` (`set_clock_uncertainty -from/-to`) and `t2_grp`
(`set_clock_groups`) still bail at parse — assert the verbatim error
strings (already covered by `multi_clock_bails` in `sdc.rs`; add an
integration-level pin that `two_clock.def` + those files bail identically
so the "deferred, not silently folded" promise is tested end to end).

## 4. Hazard pin

`one_clock_on_two_clock_def`: parse `t2_int.sdc` minus the `ckb` line
(or an inline SDC string), run on `two_clock.def` → `u_fa0/D`,
`u_fa1/D`, `ya` evaluate; `u_fb0/D`, `u_fb1/D`, `u_fx/D`, `u_fy/D`,
`yb` are absent from `report.endpoints` and named in
`skipped_endpoints`. Reasons differ per pin — `u_fb0/D`, `u_fb1/D`,
`u_fx/D` get `NoClock` (capture pin unreached), `u_fy/D`/`yb` get the
no-arrival skip (`u_fb0`'s launch is dead: its clk pin's domain tag is
`u8::MAX`, so no `LaunchSeed` is emitted — api.md §3). Assert
membership, not one reason for all. Asserts the undeclared-domain
fold is structurally gone.

## 5. Fail-proof mutation (E0 rule 7)

During development, break the capture-row read on purpose:

```
// in collect_endpoints: let cap = &sdc.clocks[clock_domain[clk]] ->
let cap = sdc.design_clock().unwrap();   // domain-0 read, the bug this ticket removes
```

`two_clock_int` must go red: `u_fb1/D`'s req shifts `0.885446` →
`1.885446` (Pa−Pb = 1.0 ns). Record the red run + restore in
`docs/results.md` ("fail-proof" subsection, same format as the
`mc-model` entry). A second cheap mutation — `same_domain` returning
`true` unconditionally — turns red via the count assertion
(`cross_domain_endpoints == 3` expected, 0 reported): use whichever is
run; one is required, both is better.

## 6. Replaced test

`sta_bails_on_a_two_clock_sdc` is deleted — it pins the removed gate.
Its parse-side assertion (`design_clock_count() == 2`) is already
covered by `sdc.rs` tests; its eval assertion is superseded by
`two_clock_int`.

## Files

- `crates/grus-core/src/sta/analysis.rs` (test module only)
