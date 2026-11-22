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
   shrinks to genuinely unresolved nets (`unknown` feedback cones — the
   only zero-activity class left) or retires. Measured: undriven nets get
   the `input` default, not zero; x/z seeds are half-weight edges.

   Representation note: propagation yields `{P, density}` as rates, while
   `SignalActivity` is integer counts + window durations. Options: (a) carry
   `eff_toggles` as `f64` in `ResolvedActivity` (counts are already consumed
   via the `f64` `eff_toggles()`/`prob_one()` accessors — only `power.rs`
   touches them); (b) synthesize integer counts = rate × `duration_s`.
   (a) is honest about precision, (b) keeps `SignalActivity` untouched.
   Record the choice + reason in the implement commit.

2. **Seed order** (measured 2026-11-21, per-pin precedence — activity is
   per-pin, not per-net):
   1. `constant` — `function : "0"/"1"` output pins and everything
      reachable through them: `{0, "constant"}`; immune to `-global`;
   2. `clock` — SDC clock nets: `{2/period, 0.5}`; beats VCD/SAIF
      annotation on the same pin (the `TC` finding) and `-global`;
   3. `-global` — overrides *every* non-clock non-constant pin, measured
      including VCD-annotated pins and `unknown` cones (the annotation
      split still counts the pin `vcd` — the read value goes `global`);
      `-activity` is clock-normalized, `-density` is per library time
      unit;
   4. annotation — VCD/SAIF/`set_power_activity -pins|-input_ports` on a
      pin whose name appears in the dump (flat names match port pins;
      scoped paths match inst pins; flat internal net names are dead
      entries);
   5. `input` — input ports and undriven nets: `{0.1·f_clk, 0.5}`,
      replaced by `set_power_activity -input`;
   6. `propagated` — combinational levelized fill per the measured M1
      rule, plus sequential Q=D;
   7. `unknown` — unresolvable sequential feedback cones: `{0, 0}`.

3. **Combinational fill** (M1 measured — local marginals, no joint
   state). Per gate, from `LibPin.function`:
   `P(f)` = the Boolean expression over per-input `P`s with `f` evaluated
   once per variable (AND: `∏p`; OR: `1−∏(1−p)`; XOR: `p⊕q`; INV: `1−p`;
   BUF: identity). `d(f)` = output-sensitization:
   `Σ_i d_i · P(f toggles on an i-edge)`, i.e. `P(other inputs put f in
   i-sensitive state)` — AND `Σ d_i·∏_{j≠i}p_j`; OR `Σ d_i·∏_{j≠i}(1−p_j)`;
   XOR `Σ d_i` (any input edge toggles); INV/BUF `d` passes through.
   Inverting pins compose `1−P` at the same density. x/z seeds per P5
   (half-weight edges in density; x/z time stays in the `P` denominator).

4. **Sequential fill** (measured — S1): a DFF Q pin copies its D pin's
   `{P, d}` verbatim (no clock-derived activity of its own; CLK pins stay
   `clock`). Sequential feedback cones that cannot be seeded stay
   `unknown` → `{0, 0}` — OpenSTA does not resolve the toggle/counter
   loops (no divide-by-2: the measured `mba` counters report 0 despite
   exact dumps of `n_q0` `5e7` / `n_q1` `2.5e7` existing in `s_full`).

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
