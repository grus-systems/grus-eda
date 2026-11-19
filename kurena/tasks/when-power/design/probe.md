# Measurement plan — OpenSTA `when` semantics inside LibreLane 3.0.16

All reference runs inside `ghcr.io/librelane/librelane:3.0.16` via
`openroad -no_init -no_splash -exit` with Tcl **files** (never stdin
heredocs — AGENTS §7 rule 3: `!` is history expansion on stdin). Repo at
`/work`, PDK at `/pdk`. The committed script
`tests/designs/microbench/generate_when_ref.sh` produces every committed
number; probe stdout goes to `/tmp` (recorded in `docs/results.md`, not
committed); the variant VCDs are committed so every reported number is
re-derivable.

Every probe session shares the reads preamble (`fixture.md` §script):
tlef + lef + `tt_025C_1v80` lib + `mbw.def` + `mbw.sdc`, then
`read_vcd -scope tb/dut <variant>` + `report_power -digits 6` +
`report_activity_annotation` (annotation counts and the saif/vcd split are
free corroborating data — capture them).

## P0 — `when` inventory (host-side, recorded)

Already run; restate in `results.md` with the command and lib path:

- `internal_power` `when`: 26 blocks, 13 cells (`ebufn_*`/`einvn_*`/
  `einvp_*`), only `"A"`/`"!A"` — single literals, all with
  `related_pin` = the enable pin, all on `Z`.
- `leakage_power` `when`: 6244 blocks — literal conjunctions plus the
  non-pin literals `SHORT`/`DIODE`. ebufn_1 states/values:
  `!A&TE_B` 0.0025423, `!A&!TE_B` 0.002764, `A&TE_B` 0.0042767,
  `A&!TE_B` 0.0028469 (lib leakage units), vs flat
  `cell_leakage_power` 0.0031074740.
- `timing` `when`: none in this library.

## P1 — count-net + z-edge semantics

Variants (A held constant so `Z` can only gain edges from enable events):

- `mbw_a0.vcd`: `a`≡0, TE_B: 4 disable+enable cycles (8 edges). `z`
  sees 0→z→0 excursions — no clean edges on any model.
- `mbw_a0te16.vcd`: identical but 16 TE_B edges.
- `mbw_a1.vcd`: `a`≡1, TE_B: 8 edges → `z` gets 1→z→1 excursions
  (z→1 re-enables).
- `mbw_quiet.vcd`: `a`≡0, TE_B≡0 — the no-activity baseline
  (leakage-only Combinational row).

Discrimination, on the Combinational internal column:

| observation | verdict |
|---|---|
| `a0te16` internal > `a0` internal (∝ TE_B edges) | count net = **related pin** (TE_B) |
| `a0te16` ≈ `a0` and `a1` ≈ `a0` | count net = **owning pin** (Z), and OpenSTA does not count z↔1 |
| `a1` > `a0` (equal TE_B edges) | count net = Z **and** z→1 counts as an edge — a divergence vs our reader to record and size |

Also record whether `a1`'s `z` net gains annotation entries
(`report_activity_annotation`) — the z-edge question is about edge
counting, not annotation.

## P2 — marginal weighting vs ignore vs joint

`TE_B`≡0 in both variants so `z`≡`a` cleanly; identical edge *count* and
edge directions on `a`/`z`, different duty:

- `mbw_duty55.vcd`: `a` pulses [200,600], [1000,1400], [1600,1900] →
  T1 = 1100/2000 → P(A) = 0.55, 3 rises + 3 falls.
- `mbw_duty80.vcd`: same rise times; falls at 900/1550/1950 →
  T1 = 1600/2000 → P(A) = 0.80, same 3+3 edges.

Discrimination on Combinational internal:

| observation | verdict |
|---|---|
| shifts ∝ duty | **H1 marginal** — weight = P(`when` expr) |
| identical to `duty55` despite duty shift | H3 (ignore) or H2 (joint — every Z edge coincides with an A edge, so per-edge sampling sees ~½ regardless of duty) |
| ≈ sum of both `when` blocks unconditionally | H3 specifically |

H2 vs H3 disambiguation if needed: the `a1` variant (P1) — under
per-edge sampling on `z→1` re-enable edges, A is statically 1 → `when:A`
fires 100%; under ignore, both blocks contribute; under marginal, `P(A)=1`
gives the same answer as joint here — `a0`/`a1` also separates them
(P(!A|a0)=1 → only `when:!A` under marginal; under ignore both fire).

## P3 — SAIF marginal check

`vcd2saif.py mbw.vcd mbw.saif` (in-script, committed output), then
`read_saif -scope tb/dut` + `report_power -digits 6` +
`report_activity_annotation`. SAIF carries `T0`/`T1`/`TC` marginals only —
it cannot express joint correlation, so:

- VCD-driven and SAIF-driven `when`-block shares equal → consistent with
  the marginal model (the expected verdict).
- They diverge in the internal column specifically → joint information is
  consumed on the VCD path; SAIF-side `when` evaluation is then
  inexpressible and stays counted (scope change recorded in `results.md`).

Emit the SAIF table into `mbw.reference.json` as `power_saif` alongside
`power_vcd` (same layout as `mb.saif.reference.json`).

## P4 — `leakage_power` `when` states

Four static variants (initial `$dumpvars` state held to #2000, zero edges):
`mbw_s00` (a=0,teb=0), `mbw_s01` (a=0,teb=1), `mbw_s10`, `mbw_s11`.
The Combinational leakage column is a single-cell readout:

| measured | verdict |
|---|---|
| all four ≈ 0.0031074740 × `leakage_power_unit` | flat `cell_leakage_power` — nothing to implement |
| each ≈ its `when`-state value (0.0025423…0.0042767) | state-weighted — implement `Σ P(state)·value` (contingency, `overview.md`) |

Record all four tables regardless of verdict.

## P5 — results.md entry (required content)

Dated entry covering:

1. P0 inventory (counts, forms, lib path + the `awk` used).
2. The count-net verdict and z-edge handling (P1, per-variant tables).
3. The probability-model verdict — marginal vs joint vs ignored — with
   the discriminating numbers (P2, P3).
4. The leakage verdict (P4, four tables) and what was implemented as a
   consequence (flat → nothing; weighted → the liberty.rs contingency).
5. The `when` forms actually instantiated by the lib (only `"A"`/`"!A"`)
   and which grammar the implementation therefore claims coverage of.
6. The golden comparison (STRICT bounds used, measured deltas).
7. Fail-proof: break the `when` evaluation (e.g. force `weight = 1.0`),
   confirm `test_when_power_mbw_matches_opensta` goes red, restore;
   corrupt the golden's Combinational internal, confirm red, restore.
8. Which parts of `generate_when_ref.sh` actually ran (AGENTS §7 rule 9)
   and the full-suite checks (`cargo test --workspace`, clippy).
