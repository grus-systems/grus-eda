# Goal

AP2 of the activity-propagation sequence —
combinational propagation per the measured **M1 local-marginal**
model (authoritative: `kurena/tasks/activity-propagation/design/api.md`
§3; evidence `docs/results.md` 2026-11-21 N7 + golden
`tests/designs/microbench/mba.activity.reference.json`).

AP1 (`activity.rs::resolve_activity`) already seeds
constant/clock/global/annotation/input and fills leftovers with
`Default`. AP2 replaces `Default` on nets driven by combinational
cell outputs with a propagated `{prob_one, density}` —
origin `Propagated`.

## What lands

1. **Gate-function evaluator.** `LibPin.function` strings
   (`"(A&B)"`, `"!(A|B)"`, `"A^B"`, constants, arbitrary nested
   forms incl. repeated literals) evaluate under input
   independence. Compute, per output pin, over the resolved input
   `{p_i, d_i}`:
   - `P(f)` — exact under independence via Shannon cofactors
     (handles repeated literals and any expression shape; the
     per-operator product rules in api.md are the same math on
     the common forms);
   - `d(f) = Σ_i d_i · P(∂f/∂x_i)` — `∂f/∂x_i` the Boolean
     difference `f(x_i=0) ⊕ f(x_i=1)`, also via cofactors.
   A missing/constant function or unparseable form leaves the
   net `Default` (record how many, if any, the sky130 lib
   triggers). Keep the evaluator total (no panics) and bounded.

2. **Levelized fill.** Combinational cells evaluate in level
   order over the net graph. `sta::analysis::topological_levels`
   is private — either expose it or compute a Kahn order inside
   `activity.rs` (api.md leaves the mechanism open; say which
   landed). Sequential/latch cells are boundaries: their outputs
   are never produced here (AP3), and their input nets are
   ordinary sinks/sources.

3. **Input-gating (taint).** Propagation consumes only
   resolvable origins — `Annotated`, `Clock`, `Global`,
   `Input`, `Constant`, `Propagated`. A gate with any
   `Default` input keeps its output `Default` (the
   sequential-feedback cones stay unresolved until AP3 —
   `u_i0/Y`, `u_x1/X` must NOT become `Propagated` off the
   interim default value). Resolvable values compose across
   origins (e.g. an `Annotated` input and an `Input` input
   mix — `s_partial` exercises this).

4. **Golden coverage** — extend `power.rs`'s mba activity test:
   - Every net resolved `Propagated` matches the golden
     `{density, prob}` per pin at the recorded strict bound,
     for `s_none`, `s_input`, `s_ports`, `s_ports_a2x`,
     `s_partial` (flat `n_and` name is a dead entry →
     identical to `s_ports`), `s_full` (annotated inputs
     propagate into instance pins — values equal the
     annotated net), and `s_global*` is unchanged (no
     propagated pins exist there).
   - `EXPECTED_DIVERGENT` shrinks to exactly the AP3 set:
     `u_df/Q` (Q=D copy — golden labels it `propagated`
     but it is the sequential rule) + the 11 `unknown`
     pins. Assert the new set verbatim.
   - Source labels: our per-net `origin` vs the golden's
     per-pin `source` follows the recorded convention —
     an annotated net's *instance* pins read `propagated`
     in the golden (per-pin/per-net residual, AP4 scope);
     assert {d,P} strictly and the source label under that
     mapping rule, not literally.
   - `s_xz`: include only if the x/z seed already matches
     verbatim (x/z half-weight is AP4; if it already holds,
     pin it).
   - Fail-proof: disable propagation → red on a propagated
     pin; corrupt a propagated golden entry → red; restore.
     Record both in the entry.

5. **Downstream effects to record** (`docs/results.md`):
   `driven_nets_without_activity`/origin counts shrink on
   partial VCDs (Default → Propagated is the reason);
   group-column deltas vs golden for `s_none`/`s_input`/
   `s_ports` re-measured and the new residuals named
   (u_df/Q + unknown cone = AP3); any moved existing
   golden explained by named nets (fully-annotated
   fixtures stay byte-identical — assert).

## Files

- `crates/grus-core/src/activity.rs` (evaluator + levelized
  fill; a `bool`-expression parser may live here or in a
  sibling module)
- `crates/grus-core/src/power.rs` (test updates only)
- `docs/results.md`, `docs/roadmap.md` (N7 line → AP2 done)
- `kurena/tasks/ap2-combinational-propagation/` (design docs)
- `Cargo.toml` / `Cargo.lock`
