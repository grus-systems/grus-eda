# Design documents (`docs/design/`)

Per-step design documents and implementation tickets. Each step has one overview
document at this level and a folder with the numbered tickets; external review
requests and their results live in the same folder and are records, not current
guidance. Current status is in [`../status.md`](../status.md); the measurement
log is [`../results.md`](../results.md).

## Step 4 — three designs through all stages (done)

| File | What |
|---|---|
| [step4-designs.md](step4-designs.md) | Overview: aes, ibex, jpeg through all stages; F2 map correlation |
| [step4/02-aes-generate-ref.md](step4/02-aes-generate-ref.md) | Ticket 02: aes reference generation script |
| [step4/03-aes-tests.md](step4/03-aes-tests.md) | Ticket 03: aes stage tests, the E1 map test, picorv32 E2 |
| [step4/04-jpeg-generate-ref.md](step4/04-jpeg-generate-ref.md) | Ticket 04: jpeg reference generation script |
| [step4/05-jpeg-tests.md](step4/05-jpeg-tests.md) | Ticket 05: jpeg stage tests (E0–E3) |
| [step4/06-async-endpoints.md](step4/06-async-endpoints.md) | Ticket 06: asynchronous set/reset pins as timing endpoints |
| [step4/07-ibex-generate.md](step4/07-ibex-generate.md) | Ticket 07: ibex reference generation script |
| [step4/08-ibex-tests.md](step4/08-ibex-tests.md) | Ticket 08: ibex stage tests (E0–E3) |
| [step4/09-sdc-tcl-subset.md](step4/09-sdc-tcl-subset.md) | Ticket 09: SDC Tcl subset and clock latency (ibex E3 unblocker) |
| [step4/10-latch-and-clock-gating.md](step4/10-latch-and-clock-gating.md) | Ticket 10: latch endpoints and clock-gating checks (ibex E3 unblocker) |

## Step 5 — review fixes F3 and F6 (done)

| File | What |
|---|---|
| [step5/11-f3-sta-loud-failures.md](step5/11-f3-sta-loud-failures.md) | Ticket 11 (F3): STA stops dropping things silently |
| [step5/12-f6-reader-strictness.md](step5/12-f6-reader-strictness.md) | Ticket 12 (F6): readers reject malformed input |

## Review fixes F1, F7, F8 — from the 2026-11-11 project review (done)

| File | What |
|---|---|
| [review-fixes.md](review-fixes.md) | Overview: F1, F7, F8 from the 2026-11-11 project review |
| [review-fixes/02-f1-convergence.md](review-fixes/02-f1-convergence.md) | F1: honest convergence report for global placement |
| [review-fixes/03-f7-cli.md](review-fixes/03-f7-cli.md) | F7: make the CLI advertise only what works |
| [review-fixes/04-f8-docs.md](review-fixes/04-f8-docs.md) | F8: bring the documents back in line with the code |

## Step 6 — routed SPEF and the E3.4/E3.5 decision (done)

| File | What |
|---|---|
| [step6/13-routed-spef-generation.md](step6/13-routed-spef-generation.md) | Ticket 13: routed SPEF for gcd — route, extract, `routed_spef` golden mode |
| [step6/14-routed-spef-sta-test.md](step6/14-routed-spef-sta-test.md) | Ticket 14: our STA on the routed SPEF vs OpenSTA, gcd |
| [step6/15-estimator-scoring-e34-e35-decision.md](step6/15-estimator-scoring-e34-e35-decision.md) | Ticket 15: score both estimators against routed truth; the E3.4/E3.5 decision |

## Step 7 — E3.5 scale evidence (done)

| File | What |
|---|---|
| [step7/16-picorv32-routed-spef.md](step7/16-picorv32-routed-spef.md) | Ticket 16: a routed SPEF for picorv32 — E3.5 scale evidence and the coupling cross-check |
| [step7/17-dcalc-routed-table.md](step7/17-dcalc-routed-table.md) | Ticket 17: DMP vs `report_dcalc` on the picorv32 routed SPEF (76 rows) |

## E3.5 — driver model, waveform slew and extreme-load investigation (done)

| File | What |
|---|---|
| [e35/01-rd-and-wave-slew.md](e35/01-rd-and-wave-slew.md) | Working record: `RD_SCALE` sweeps, waveform slew, ramp-fit degeneracy, stage-by-stage and wire-delay measurements |
| [e35/opensta-known-issues.md](e35/opensta-known-issues.md) | Upstream OpenSTA issue/PR search (`#227`, `#468`, `#582`) |
| [e35/probe.patch](e35/probe.patch) | Measurement-only env-var probe patch (not committed to source) |

## E3.6 — DMP solve speed (done)

| File | What |
|---|---|
| [e36-dmp-speed.md](e36-dmp-speed.md) | Design: make the DMP effective-capacitance solve fast |
| [e36/01-implement.md](e36/01-implement.md) | Ticket 01: speed up the DMP solve per the design |
| [e36/03-discrete-charge.md](e36/03-discrete-charge.md) | Ticket 03: closed form of the trapezoid recurrence for `pi_charge` |

## F5 — legalizer pass 1 speed (done)

| File | What |
|---|---|
| [f5-pass1-speed.md](f5-pass1-speed.md) | Design: speed up refinement pass 1 of the legalizer |
| [f5/02-implement.md](f5/02-implement.md) | Ticket 02: spatial index for legalizer refinement pass 1 |
| [f5-pass1-eval.md](f5-pass1-eval.md) | Design (follow-up 2): exact fast evaluation for legalizer pass 1 swaps |
| [f5/04-implement-eval.md](f5/04-implement-eval.md) | Ticket 04: extrema cache for pass 1 swap evaluation |

## Phase 2 continuation — STA and legalizer speed (in progress)

| File | What |
|---|---|
| [phase2-speed.md](phase2-speed.md) | Overview: measured baseline, corrected premises, ticket list |
| [phase2/01-dmp-inner-solvers.md](phase2/01-dmp-inner-solvers.md) | Ticket 01: Brent/secant inner solvers in `sta/dmp.rs` |
| [phase2/02-sta-call-audit.md](phase2/02-sta-call-audit.md) | Ticket 02: `driver_response` call audit |
| [phase2/03-legalize-pass3.md](phase2/03-legalize-pass3.md) | Ticket 03: exact legalizer Pass 3 speedup |
| [phase2/04-timinggraph-csr.md](phase2/04-timinggraph-csr.md) | Ticket 04: TimingGraph flat CSR (structural, last) |

## F9 — machine-independent tests and CI (done)

| File | What |
|---|---|
| [f9-test-support-ci.md](f9-test-support-ci.md) | Design: machine-independent tests and CI |
| [f9/02-implement.md](f9/02-implement.md) | Ticket 02 (Part A): test_support module, Liberty cache relocation, bench |
