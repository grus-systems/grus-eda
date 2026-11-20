# `when`-qualified `internal_power` — design overview

## Context

`power.rs::pin_internal_power` skips every `internal_power` block carrying a
`when` state expression and counts it in
`PowerDiagnostics::conditional_power_blocks`. This is the last open item of
roadmap N7.

**Inventory (measured on the pinned lib,
`sky130_fd_sc_hd__tt_025C_1v80.lib`, host-side `awk` — recorded in
`probe.md` P0):**

- `internal_power` `when`: exactly **26 blocks across 13 cells**
  (`ebufn_{1,2,4,8}`, `einvn_{0,1,2,4,8}`, `einvp_{1,2,4,8}`), all on the
  output pin `Z`, all of one of two forms: `when : "A"` and `when : "!A"`.
  Structure per cell is identical: Z carries (1) an unconditional block with
  `related_pin : "A"`, (2) a block `related_pin : "TE_B"`/`"TE"` with
  `when : "!A"`, (3) the same with `when : "A"`. No `|`/`^`/paren forms
  appear inside `internal_power` anywhere in the library.
- `leakage_power` `when`: 6244 blocks library-wide — conjunctions of up to
  ~8 literals (e.g. `!A&TE_B`, DFF state expressions), plus the non-pin
  literals `SHORT`/`DIODE`. ebufn/einvn/einvp each declare 4 states over
  `A`×enable, spread about −18%/+38% around their `cell_leakage_power`.
- `timing` `when`: none in this library.
- No committed design instantiates a `when`-carrying cell
  (`gcd.synth.json` defines but does not instantiate them; aes cells are
  synthesized inside OpenROAD). The fixture must carry its own instance.

## Candidate semantics — the probe decides (probe.md)

Three hypotheses for how OpenSTA applies a `when` block; the fixture
variants discriminate them:

- **H1 marginal**: contribution = table(trigger slew, own-net load) ×
  own-net transitions × `P(when)`, with `P(when)` = product over literals
  of the condition pin's measured state probability (VCD time-in-state /
  SAIF `T0`/`T1` duty). This is the expected verdict: OpenSTA's activity
  model stores only per-pin {probability, toggle density} — no joint
  distributions exist for it to sample.
- **H2 joint/per-edge**: the block fires for the share of trigger edges
  occurring while the condition holds. VCD could express this; SAIF
  marginals cannot.
- **H3 ignore**: `when` dropped, both blocks summed unconditionally.

The second open axis is the **count net**: by the previously measured rule
(power-owning pin's own net — `docs/results.md` 2026-11-18 DFF case) the
enable blocks multiply by `Z`'s transitions, not `TE_B`'s. The probe
re-verifies this on a tristate cell because the prior measurement covered
only a `related_pin : "CLK"` form (variant `a0`/`a1` in `probe.md` P1).

## Implementation if H1 holds (expected)

- `VcdActivity` gains per-signal **state durations** (`t0_s`/`t1_s`), so
  `P(pin=1) = t1/(t0+t1)` exists for every annotated net — VCD computes it
  from the waveform, SAIF fills it from `T0`/`T1`. Signature of
  `estimate_power` is unchanged.
- `lib_db.rs` gains a `when`-expression parser: conjunction of literals
  (`lit (& lit)*`, `lit` = optional `!` + pin name). Anything else
  (`|`, `^`, parens, unknown pin) → inexpressible → counted, as today.
- `power.rs` multiplies each `when` block's contribution by the measured
  marginal probability; `conditional_power_blocks` semantics narrow to
  *inexpressible* forms (bad grammar, unresolved pin, or no measured state
  on the condition net). Unconditional path untouched.

If the verdict is H2 (joint), SAIF `when` blocks stay inexpressible
(documented) and the VCD side needs pairwise edge correlation — the design
then changes to carry per-edge condition snapshots; `steps.md` notes where
that branch diverges. If H3, only `leakage`/`internal` bookkeeping changes.
The fixture is built so all three verdicts are measurable from the same
files.

## `leakage_power` `when` — probe-and-record, conditional implement

Current model is flat `cell_leakage_power` (`LibCell::leakage_power_w` —
the `leakage_power` `when` blocks are not even parsed). The earlier SAIF
duty-swap probes on mb moved no leakage column, weak evidence for flat;
ebufn's ±20–38% state spread is a much stronger discriminator (`probe.md`
P4, four static-state runs). Verdict goes to `docs/results.md`.

- If flat: nothing to implement.
- If state-weighted: `readers/liberty.rs` must parse `leakage_power`
  `when`+`value` pairs into `LibCell` (LibDb serialization bump), and
  leakage becomes `Σ P(state)·value` on the same marginal model. This adds
  `readers/liberty.rs` to the Files list — decide at implementation time
  and record the scope change in `results.md` and the commit.

## Honest-scope notes

- `P(pin)` from `t1/(t0+t1)` conditions on *defined* state time; x/z time
  enters neither term (Z's tristate excursions are the main producer of
  non-0/1 time — record OpenSTA's z-edge handling via `probe.md` P1).
- VCD constant signals now emit a `signals` entry (`rise=fall=0`, real
  durations): `driven_nets_without_activity` becomes "not annotated"
  rather than "no clean edges" — a semantic refinement, recorded in
  `results.md` (no committed test asserts a nonzero count; `mb.power.json`
  carries 0).
- A `when` literal on an unannotated net is inexpressible → counted
  (OpenSTA would statistically propagate a probability instead — the
  same documented activity-model gap as the gcd power divergence, not a
  new class).
- `|`, `^`, and parenthesized `when` forms never occur in this library's
  `internal_power`; they stay counted-inexpressible. Conjunctions are
  supported from the start (the leakage contingency needs them; cost is a
  product loop).

## Risks

- **Clock-less SDC**: `mbw` has no sequential cell; `mbw.sdc` is
  `set_input_transition` + `set_load` only. If `sta_pin_slews` or OpenSTA
  `report_power` needs a clock, add `create_clock -name vclk -period 20`
  with no source (fallback order documented in `fixture.md`). Do **not**
  put a clock on `a`/`teb` — OpenSTA overrides clock-net activity with
  SDC-derived rates and the probe would measure nothing.
- **z-edge semantics**: OpenSTA may count `z→1`/`1→z` as transitions where
  our reader counts only clean 0↔1 edges. If it does, `Z`'s count and the
  verdict on count-net shift — measured by the `a0`/`a1` variants before
  the golden is committed.
- **Fixture sensitivity**: `mbw` is a single cell, so the `when`
  contribution must exceed the tolerance to be assertable and for the
  fail-proof break to go red — the duty and edge counts in `fixture.md`
  are sized so the two `when` tables (which differ in magnitude/sign)
  move Combinational internal by far more than the bound.
- **Provenance**: `mbw.*` files are authored in-repo (like `mb.vcd`), not
  copied from any upstream design; `generate_when_ref.sh` emits the golden
  inside the container (AGENTS §7 rule 1).

## Files

- `crates/grus-core/src/power.rs`
- `crates/grus-core/src/lib_db.rs`
- `crates/grus-core/src/readers/vcd.rs`
- `crates/grus-core/src/readers/saif.rs`
- `tests/designs/microbench/` (new `mbw.*` + `generate_when_ref.sh`)
- `docs/results.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
- Contingency only (leakage verdict = state-weighted):
  `crates/grus-core/src/readers/liberty.rs`
