# api — where propagation plugs into `power.rs`, and the sequenced plan

Branch I (one-ticket implement) and Branch P (plan lands) share this
technical basis; Branch P commits §Plan into `docs/roadmap.md` instead of
code.

## What exists today

- `estimate_power` (`power.rs`) seeds `PowerCx.net_activity:
  Vec<Option<SignalActivity>>` once: `vcd.signals.get(net_name).copied()`.
  `None` = "no annotation entry" → `net_activity()` =
  `unwrap_or_default()` (zero toggles) and `net_prob_one()` = `None` → any
  `when` literal on that net is inexpressible (`conditional_power_blocks`).
  `driven_nets_without_activity` counts driver-having `None`s.
- Both readers feed the same `VcdActivity` (SAIF resolves into it), so the
  fill point is a single place — the `net_activity` slots in `PowerCx`.
- `LibPin.function: Option<SymbolId>` already stores output-pin Boolean
  strings (`"(A&B)"`, `"1"` on `conb` HI). A gate-function evaluator — the
  combinational propagation primitive — is new code.
- `sta::analysis::topological_levels` is private to the STA pass; the
  activity fill needs the same Kahn order over combinational arcs. Either
  it is exposed, or `power`/new `activity` module computes its own level
  order — decided at implement time (the module split matters more than
  the reuse mechanism).

## Proposed shape (implement branch / follow-on tickets)

1. **Resolved activity type.** Replace the `Option<SignalActivity>` slot
   with a resolved per-net record:

   ```rust
   enum ActivityOrigin { Annotated, Clock, Propagated, Sequential, Constant, Default }
   struct ResolvedActivity { prob_one: Option<f64>, eff_toggles: f64, origin: ActivityOrigin }
   ```

   `PowerCx.net_activity: Vec<ResolvedActivity>`; `net_activity()`/
   `net_prob_one()` read from it. Provenance is kept because OpenSTA's
   `report_activity_annotation` splits pins by source — our diagnostics get
   the same per-origin counts (`propagated_nets`, `default_nets`,
   `constant_nets`) so nothing is hidden; `driven_nets_without_activity`
   shrinks to genuinely unresolved nets (undriven, x-only) or retires —
   decided by the P5 measurement.

   Representation note: propagation yields `{P, density}` as rates, while
   `SignalActivity` is integer counts + window durations. Options: (a) carry
   `eff_toggles` as `f64` in `ResolvedActivity` (counts are already consumed
   via the `f64` `eff_toggles()`/`prob_one()` accessors — only `power.rs`
   touches them); (b) synthesize integer counts = rate × `duration_s`.
   (a) is honest about precision, (b) keeps `SignalActivity` untouched.
   Record the choice + reason in the implement commit.

2. **Seed order** (precedence measured by P4 / the SAIF `TC` finding):
   1. explicit annotation — VCD/SAIF per-net entry (partial annotation
      covers only its nets);
   2. SDC clock nets — rate from the SDC clock (clock activity follows
      SDC, not the annotation);
   3. propagated — combinational levelized fill per the measured M1/M2 rule;
   4. sequential — `findSeqActivity` rule per the P3 verdict;
   5. defaults — the measured input/internal/sequential defaults;
   6. constants — `function : "0"/"1"` pins and their downstream;
   7. undriven/x — per P5 (likely zero + counted, or a default — measured).

3. **Combinational fill.** Per gate, from `LibPin.function` + the measured
   {P,d} rules (M1 local marginals if P2 confirms — the expected verdict,
   matching OpenSTA's per-pin {probability, density} model that has no joint
   state). Inverting output pins (`inv`, `nand`, `nor`) compose `1−P`
   through the same evaluator. x/z seeds per P5.

4. **Sequential fill.** Per the P3 verdict (S1 D-rate / S2 uniform default /
   S3 clock-derived); feedback registers follow whatever rule the
   toggle/counter evidence pins down — not a guessed BDD walk.

5. **CLI + diagnostics** (implement branch only). `--vcd`/`--saif` become
   optional (clap `required_unless_present` today): neither ⇒ fully
   propagated; one ⇒ annotated seeds + propagated fallback for the rest —
   the measured precedence. The stderr diagnostic line gains the new
   per-origin counts. `mb.power.json`-style JSON gains `origin` per net
   only if a per-net dump exists — otherwise the report shape is unchanged.

6. **Visible semantic changes to record in `results.md`.**
   - `when` literals on propagated nets become expressible →
     `conditional_power_blocks` may drop on existing fixtures (mbw keeps 0;
     the mb.saif `q` case changes if `q` were unannotated — it is not).
   - `driven_nets_without_activity` on partial VCDs drops to ~0 by design
     (unannotated ≠ zero anymore) — the *reason* changes, recorded.
   - gcd `report_power` is re-compared in the propagation tickets; the
     residual table gets an update line.

## Plan — the sequenced tickets (Branch P deliverable)

One commit each, ordered so each is independently golden-covered
(kurena-sized ~50–150 lines):

- **AP1 — resolution skeleton + measured defaults.** `ResolvedActivity` +
  the fill pass seeded by annotation/clock/defaults only (no propagation);
  unannotated nets get the P1-measured default {P,d} instead of zero.
  Golden: `s_none` + `s_global`/`s_input` sessions on `mba`; CLI gains the
  no-annotation mode.
- **AP2 — combinational propagation.** Gate-function evaluator + levelized
  fill per the P2 verdict. Golden: `s_ports` implied-rate table.
- **AP3 — sequential rules.** The P3 verdict (toggle/counter evidence).
  Golden: `s_ports`/`s_full` sequential columns + `s_ports_a2x` sweep.
- **AP4 — precedence, x/z, constants, diagnostics.** Partial-annotation
  blend per P4, x/z per P5, per-origin diagnostic counts, `mba_partial`/
  `mba_xz` coverage, and the gcd re-comparison recorded in `results.md`.

Done criterion per step: `mba.activity.reference.json`'s corresponding
session block matched at the STRICT bounds measured in this ticket's
results entry; existing power goldens byte-identical.
