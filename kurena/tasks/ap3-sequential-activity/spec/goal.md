# Goal

AP3 of the activity-propagation sequence — sequential
activity rules per the measured **S1** verdict (authoritative:
`kurena/tasks/activity-propagation/design/api.md` §4; evidence
`docs/results.md` 2026-11-21 N7 + golden
`tests/designs/microbench/mba.activity.reference.json`).

Measured rule: a DFF's Q pin copies its D pin's `{P, density}`
verbatim — the golden labels these copies `propagated`
(OpenSTA prints no `sequential` source). Sequential feedback
cones that can never be seeded (toggle flop, counter) stay
`unknown` → `{0, 0}`; OpenSTA does not resolve the loop
(no divide-by-2 even when exact rates exist in a dump).

After AP2, `EXPECTED_DIVERGENT` holds exactly these 13 pins:
`u_df/Q` + `qd` (same net `q`) and the 11 `unknown` pins on
nets `n_q0`, `n_q0n`, `n_q1`, `n_c1d`. AP3 resolves all of
them — the divergent set goes to **empty** and every mba
session matches verbatim.

## What lands

1. **Data-pin identification.** Liberty `ff`/`latch` blocks are
   parsed today only into `is_sequential`/`is_latch` flags
   (liberty.rs) — `next_state`/`data_in`/`enable` pin names are
   dropped. Store the data-pin reference on `LibCell` (e.g.
   `LibCell::seq_data_pins` or equivalent). Scope: the plain
   single-data-pin form (`next_state : "D"`) is what mba and
   the measurement cover; complex `next_state` expressions
   (scan mux `D*!SE + SCD*SE`, set/reset terms) are a named
   residual — record the forms sky130 actually carries and
   how many are covered.

2. **Q = D copy.** A sequential output pin's net gets the
   resolved activity of the net on the identified data pin,
   origin `Propagated` (the golden's label — `Sequential`
   stays unused unless a distinct source is measured).
   Multi-output cells (`Q`/`QN`): copy density verbatim;
   probability is `P` for the non-inverted output, `1−P` for
   the inverted one — decide by the lib's convention (record
   which rule landed; mba's `dfxtp` is single-Q so this is
   measured-adjacent, not measured).

3. **Fixed point + unknown conversion.** Interleave with the
   combinational pass: a resolved Q enables more combinational
   propagation (Q feeding gates feeding other flops' D), so
   run to a fixed point (Kahn/worklist — the measured model
   resolves everything resolvable). Then every still-`Default`
   net becomes `unknown` `{0, 0}` — measured semantics for
   the sequential-cone leftovers; the same conversion also
   covers any unparseable-function leftovers (unmeasured,
   noted). `unknown` propagates as taint: combinational cells
   fed by `unknown`/`Default` inputs stay unresolved
   (u_i0/Y, u_x1/X — already the AP2 rule).

4. **Golden coverage** — `EXPECTED_DIVERGENT` goes to
   `{}` (empty); the mba activity test then asserts **all 12
   sessions**' per-pin `{density, prob}` + mapped `source`
   verbatim at the strict bound, including:
   - `s_ports`/`s_ports_a2x`/`s_full` sequential columns —
     `u_df/Q`+`qd` follow `a` exactly (3e6→6e6 density on a2x
     at the same 0.55 — the S1 discriminator already pinned
     in the golden);
   - the 11 `unknown` pins → `{0,0}` verbatim;
   - group columns for the sessions become fully assertable —
     record the achieved bound (residuals should now be at
     print precision; keep the honest bound and record it).
   Fail-proof: disable the sequential pass → red on
   `u_df/Q`/the unknown set; corrupt a sequential golden
   entry → red; restore. Record both.

5. **Docs** (`docs/results.md`): AP3 entry — the 13-pin
   residual closed, group-column residuals now, sky130
   `next_state`/`data_in` coverage census (which forms parse,
   which are residual — e.g. `sdfxtp` scan mux, `dfrtp`
   set/reset, latches `dl*`/`dlx*`), multi-output inversion
   rule, `prob_one`/`when` on `unknown` nets (unmeasured —
   state the chosen value). Roadmap N7 → AP3 done. Moved
   goldens enumerated (fully-annotated fixtures stay
   byte-identical — assert).

## Files

- `crates/grus-core/src/activity.rs` (sequential pass + fixed
  point + unknown conversion)
- `crates/grus-core/src/readers/liberty.rs`,
  `crates/grus-core/src/lib_db.rs` (data-pin field)
- `crates/grus-core/src/power.rs` (test updates)
- `docs/results.md`, `docs/roadmap.md`
- `kurena/tasks/ap3-sequential-activity/` (design docs)
- `Cargo.toml` / `Cargo.lock`
