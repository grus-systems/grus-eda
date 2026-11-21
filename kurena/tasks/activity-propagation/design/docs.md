# docs — what lands in `docs/results.md`, `docs/roadmap.md`, `tests/designs/README.md`

## `docs/results.md` — dated entry (use the actual run date)

Title: `### <date> — N7 — activity propagation probe (v0.2.13)` (append
`(v0.2.13)` or the implement version if Branch I lands). One entry; sections:

1. **Run record**: committed `tests/designs/microbench/generate_activity_ref.sh`
   inside `ghcr.io/librelane/librelane:3.0.16`, `sky130_fd_sc_hd`
   `tt_025C_1v80`, `mba.def`/`mba.sdc`; note `mba.none.openroad.txt` is a
   `grep -v`-filtered transcript, not verbatim; name which script sections
   actually ran (§7 rule 9) and how many sessions produced the JSON.
2. **P0 command surface**: verbatim `info commands` lists and the
   `set_power_activity`/`set_power_analysis` parse results; the accessor
   found or "no per-pin accessor — all rates below are implied from
   `report_power` columns on known net loads", and its units sanity check
   if found.
3. **Defaults (P1)**: `s_none` table verbatim + `s_global`/`s_input` deltas;
   the solved default {probability, density} per net class (input, internal
   combinational, sequential) and the clk-net rate vs `2/period` — each
   with its derivation named.
4. **Combinational verdict (P2)**: M1/M2/M0 one-liner + the discriminating
   implied values (`n_or` 0.552-vs-0.64, `d_xor` rule) and the
   `s_ports`-vs-`s_full` per-net delta table.
5. **Sequential verdict (P3)**: S1/S2/S3 one-liner + `a2x` sweep evidence +
   toggle/counter implied rates vs exact (`n_q0` 100, `n_q1` 50 toggles);
   the clk-pin internal SDC-rate re-confirmation.
6. **Fallback semantics (P4)**: annotation splits verbatim per session;
   the inconsistent-`n_and` precedence verdict (annotation beats
   re-derivation, or not); which nets fell back and to exactly what.
7. **x/z & constants (P5)**: `s_xz` deltas vs `s_ports`; conb/buf/`n_float`/
   `n_flt` record — including the honest "not measurable through
   `report_power` columns" note where that holds.
8. **Disposition**: Branch I — what was implemented, the mba golden test,
   bounds used; or Branch P — the AP1–AP4 plan summary (roadmap has the
   full text). Then: which checks ran (`cargo test --workspace`, clippy),
   the regen-idempotence result (`git status` clean after a rerun —
   §7 rule 1), and the fail-proof result if code landed.

## `docs/roadmap.md` — N7 edit

Replace the closing "Still open: matching OpenSTA's activity propagation
if a closer `report_power` agreement is ever required." line:

- **Branch I**: `- Activity propagation implemented at the measured
  OpenSTA model (<one-line verdict summary>) — `mba` golden coverage;
  gcd residual re-measured and recorded in `docs/results.md`.` → N7
  effectively closes; say "DONE" in the section header style used
  elsewhere (e.g. `### N6. ... — DONE`).
- **Branch P**: keep the "still open" fact but replace the hand-wave with
  the plan: `Still open: statistical activity propagation — semantics
  measured <date> (defaults → combinational → sequential → precedence;
  sequenced as AP1–AP4, each golden-covered on the `mba` fixture).
  See `docs/results.md` <date> and `kurena/tasks/activity-propagation/
  design/api.md` §Plan.` Do not create a new roadmap section — the plan
  lives in the design docs and the N7 line points at it.

## `tests/designs/README.md` — fixture line

Extend the existing `microbench` entry (not a new section): one sentence
— `` `mba.def`/`mba.sdc` is the activity-propagation fixture
(and/or/xor/inv cone + DFF/toggle/counter + `conb` + an undriven net);
`generate_activity_ref.sh` emits `mba.activity.reference.json` and the
filtered `mba.none.openroad.txt` probe transcript; the `mba_*.vcd` set
annotates ports/all/partial/x-z variants for the propagation probes in
`docs/results.md`. ``

## `README.md` / `AGENTS.md`

Untouched — the N7 power paragraph in `README.md` stays accurate under both
branches (propagation is a fill detail of the estimator, not a user-visible
mode change unless Branch I adds the no-annotation CLI form; if it does,
the README's power sentence gets one clause — decide then).
