# async-arc-probe — Documentation updates

All prose English, `docs/` style. The R3/R4 rewrite needs no probe — it is a
correction of stale text; land it in the same commit as the rest.

## 1. `docs/roadmap.md` — R3 → Done

Replace the `### R3.` body with a DONE entry citing the committed evidence:

- Fixture `sta_edge/hold_violation/` (`hold.def` + `ideal.sdc` +
  `propagated.sdc`): 2 of 4 endpoints hold-violate in both modes, matched at
  `Tol::STRICT` by `test_sta_edge_hold_violation_{ideal,propagated}`
  (`analysis.rs` — v0.1.61, `docs/results.md` 2026-11-17 "R3").
- `StaReport::hold_violations` is printed by `grus-eda sta` next to WNS/TNS
  (`main.rs`) and serialised in `--json`; `compare_sta_edge` asserts the
  counts exactly when the golden carries them.
- Residual note worth keeping verbatim: the propagated source-slew fix
  (`set_input_transition`, not `set_clock_transition`) from the same entry.

## 2. `docs/roadmap.md` — R4 → Done

Replace the `### R4.` body:

- Fixture `sta_edge/clock_gate/` covers `and2`/`or2`/`nand2`/`nor2` plus the
  `dlclkp` ICG, with `dfrtn` flops behind the inverting gates so both edge
  polarities are exercised — STRICT in ideal and propagated
  (`test_sta_edge_clock_gate_{ideal,propagated}`, v0.1.66, `docs/results.md`
  2026-11-17 "Roadmap R4").
- The measured rule stays: gate kind comes from the output pin's Liberty
  `function` (AND/NAND = rising check, OR/NOR = falling, both edges swapped);
  muxes and partial functions are rejected by `gate_kind`, locked by
  `test_gate_kind_classifies_liberty_functions`. `dlclkp`'s GATE check is an
  ordinary register check and `CLK -> GCLK` uses the combinational arc.
- Residuals to state accurately: the four `CrossDomain` deferrals in
  `docs/multi-clock.md` §2 are unrelated and stay parked there; cells without
  a `function` keep the AND guess; `gating_enables` checks get no CRPR credit.
  (Correction during implementation: the "ideal clock cannot see cell-driven
  clock-net inversion" residual was already closed by `ideal_clock_parity` in
  v0.1.71 — `test_sta_edge_clock_gate_ideal` is un-ignored and STRICT — so it
  is not a live residual.)

## 3. `docs/roadmap.md` — R6 disposition (two branches)

**Branch A (expected — no propagation):** mark R6 Done with the probe
evidence: re-measured on `sta_edge/async_arcs` — OpenSTA 3.0.16 yields no
`Clear`/`Preset` arrival at `Q`/`Q_N` (unclocked-`CLK` discriminators
`u_ar`/`u_ap`/`u_aq` unreached; `u_d*/D` endpoints expected to appear in
OpenSTA's `-unconstrained` list exactly as in our `skipped_endpoints`).
(Correction during implementation: `-unconstrained` lists *no* unclocked
endpoints in OpenSTA 3.0.16 — the pins simply produce zero paths and no
warning; the roadmap entry states the measured behaviour.) Check coverage
now spans every async check shape in `sky130_fd_sc_hd` (`dfrtp`/`dfrtn`,
`dfstp`, `dfbbp`/`dfbbn` rising+falling, `dlrt*`/`dlrb*` latches). Keep
`Clear`/`Preset` in `recognized_check_arcs` (propagating would diverge).
Name residuals explicitly: `non_seq_*` on `dfbb*`/`sdfbb*` (per the Q4
verdict — evaluated by OpenSTA = open sub-item, not reported = nothing left),
`sdf*` scan cells covered by family equivalence only, and the
"future OpenSTA version" re-measurement clause from the 2026-11-17 entry.

**Branch B (propagates):** R6 stays open; append the measured rules (which
arc/transitions produce arrivals, whether the async pin needs a clock
arrival, required-time anchoring downstream) and a sequenced plan: (i)
`Clear`/`Preset` leave `is_deferred_check` and become data-propagation arcs
into `TimingGraph::edges`, (ii) async launch state seeded from the check pin's
arrival (which clock/parity/domain tags — per the probe), (iii) endpoint
coverage for downstream pins, (iv) STRICT golden on `async_arcs` + a
clocked-async variant. Implement via a follow-on ticket; this one ships only
the measurement, the fixture, and the docs.

## 4. `docs/results.md` — dated entry

Title: `## 2026-11-21 — async-arc probe: R6 re-measurement (+R3/R4 roadmap
correction)` (date = the run date). Contents per `probe.md` "Recording
format": committed script, census table, Q1–Q6 with verbatim lines, our-side
`--json` skip block + `--strict` verdict, the R6 disposition, and which
commands ran (or which parts of `generate_ref.sh`, per AGENTS.md §7.9). One
line noting R3/R4 were doc-only corrections with the test names cited.

## 5. `tests/designs/README.md`

Extend the `sta_edge` bullet (or add a per-case line in the established
style): `async_arcs` — hand-written, `sky130_fd_sc_hd` only; `dfrtp`/`dfstp`/
`dfbbp`/`dfbbn`/`dfrbp` flops, three unclocked-`CLK` discriminators whose
outputs are reachable only through `Clear`/`Preset` arcs; golden +
`probe_async_*.txt` from the committed `generate_ref.sh`.

## 6. `README.md` — no change

The prototype note already says async `Clear`/`Preset` are counted and shown
but exempt from `--strict` — true under both branches (Branch B changes this
in the follow-on ticket, not here).

## 7. Version

`[workspace.package] version` `0.2.9 -> 0.2.10` in root `Cargo.toml`;
`Cargo.lock` refreshed by `cargo check`. Same commit as the fixture + docs
(functional content = new test coverage + doc corrections; patch bump per
AGENTS.md §5).
