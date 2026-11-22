# Measurement plan — OpenSTA activity propagation inside LibreLane 3.0.16

All reference runs inside `ghcr.io/librelane/librelane:3.0.16` via
`openroad -no_init -no_splash -exit` with Tcl **files** (never stdin
heredocs — AGENTS.md §7 rule 3: `!` is history expansion on stdin). Repo at
`/work`, PDK at `/pdk`. The committed `generate_activity_ref.sh` produces
every committed number; session stdout goes to `results.md` verbatim (only
`s_none`'s filtered transcript is committed as `mba.none.openroad.txt`).
Every session prints `report_power -digits 6` **and**
`report_activity_annotation` — the per-source pin split is free evidence.

Sessions share the reads preamble (`fixture.md` §script): tlef + lef +
`tt_025C_1v80` lib + `mba.def` + `mba.sdc`.

## P0 — command surface + per-pin accessor (discovery; decides the read method)

```
info commands *power*
info commands *activ*
info commands *annotation*
catch {set_power_activity} m; puts "ERR:$m"
catch {set_power_activity -help} m; puts "ERR:$m"
catch {set_power_activity -global -activity 0.2 -duty 0.4} m; puts "OK:$m"
catch {set_power_activity -input -activity 0.2 -duty 0.4} m; puts "OK:$m"
catch {set_power_activity -input_ports [all_inputs] -activity 0.2 -duty 0.4} m; puts "OK:$m"
```

Then hunt for a way to read one pin's {probability, density}: try
`get_property`/property names on pins, `sta::pin_*`, `sta::*activity*`
candidates the `info commands` output lists, and
`report_activity_annotation` flag variants (`-verbose`, `-report_unannotated`,
etc. — all inside `catch`, stderr recorded).

**Two outcomes:**

- **Accessor found** → P1–P5 read each fixture net's propagated
  {probability, density} directly. The accessor name, its units (density in
  toggles/sec vs normalized per-cycle), and one sanity check (clk pin reads
  the SDC rate) are recorded verbatim.
- **No accessor** → all numbers below are *implied* values inverted from
  `report_power` columns on the fixture's known net loads (switching =
  `0.5·C·V²·d`, `C` from wire=0 + lib pin caps + `set_load`), stated as
  "implied d(net) = …" beside each number. Fixture duties/densities are
  sized so each mechanism moves a column by ≫ print resolution.

Also record: `report_activity_annotation` on the un-annotated run — which
source categories exist besides `vcd`/`saif`/`unannotated` (`user`?
`constant`? `default`?), and what `set_power_analysis` resolves to
(the goal names it; `info commands` answers whether it exists in this build).

## P1 — no annotation at all (acceptance item 1)

Session `s_none`: reads only, then `report_power` + `report_activity_annotation`.

Record verbatim: full table, the annotation split, and warnings
(`n_float`'s undriven-net warning is evidence). Then the knob sessions:

- `s_global`: `set_power_activity -global -activity 0.25 -duty 0.4` (exact
  flag spelling per P0) → report. Column deltas vs `s_none` identify which
  nets the global default drives (combinational? sequential Qs? inputs?)
  and which it does not (clock nets should keep the SDC rate — the
  SAIF-`TC`-sweep finding says clock activity follows the SDC, not
  annotation).
- `s_input`: the input-port form P0 found (`-input` or `-input_ports
  [all_inputs]`) → report. Deltas vs `s_none` isolate the input-port
  default's reach.

**Implied-rate derivation** (no-accessor path): for each group column,
solve the candidate default density `d` from the measured switching watts.
The fixture separates the candidates:

| observation | verdict |
|---|---|
| all unannotated nets share one implied `d` | single global default — record value + duty |
| input-driven nets differ from internal/seq nets | separate input vs internal defaults — record each |
| comb columns scale with `-global`, seq columns do not | sequential has its own (`findSeqActivity`) default |
| `-input` moves only port-adjacent columns | input default is a seed, propagation still runs |
| `-input` moves nothing | inputs are not special; global default covers all |

Record the solved {P, d} per net class and the clk nets' implied rate vs
`2/period` — verbatim.

## P2 — combinational propagation (acceptance item 2)

Session `s_ports`: `read_vcd -scope tb/dut mba_ports.vcd` — only `clk`, `a`,
`b` annotated (a: P≈0.55/3R+3F, b: P=0.80/3R+3F; every internal net is a
propagation readout).

Candidate models the fixture discriminates:

- **M0 none**: internal nets get the flat default — `y`, `n_or`, `n_inv`
  implied rates equal each other and equal `s_none`'s.
- **M1 local marginals** (per-gate independence): `p_and = pa·pb = 0.44`;
  `p_or = p_and + p_inv − p_and·p_inv = 0.552`; `p_xor = pa⊕pb = 0.47`;
  densities per gate (`d_xor ≈ d_a + d_b`, `d_and ≈ d_a·pb + d_b·pa`,
  `d_inv = d_b`).
- **M2 cone/joint** (correlation retained): `p_or = P(a|!b) = 0.64` —
  `n_and` and `n_inv` are *correlated* (both from `b`), so M1 over/under
  counts. A 0.552-vs-0.64 split is ~16% of P — measurable via `n_or`'s
  implied probability (from its `when`-style marginal consumers or the
  accessor) and via `yc`'s switching.

| observation | verdict |
|---|---|
| implied p_or ≈ 0.552, d rules per gate | **M1 local marginals** |
| implied p_or ≈ 0.64 | **M2 cone/joint** |
| all internal nets at default | M0 — propagation is not per-gate math |

Cross-check: `s_full` (exact waveforms) vs `s_ports` per-net deltas — where
propagated ≠ exact and by how much. That delta table is the honest "how far
OpenSTA's model drifts on real logic" record, complementing gcd's.

## P3 — sequential propagation / `findSeqActivity` (acceptance item 3)

Same `s_ports`/`s_ports_a2x`/`s_full` sessions; the sequential nets are
`q` (D=`a` annotated), `n_q0`/`n_q0n` (toggle), `n_q1`/`n_c1d` (counter).

Candidates for register Q activity:

- **S1 follows D**: `d_q = d_a` (the annotated D rate).
- **S2 seq default**: a fixed `findSeqActivity` value — same on `q`, `n_q0`,
  `n_q1` regardless of inputs.
- **S3 clock-derived**: `d_q ∝` clk rate (e.g. half of clk's toggle rate).

| observation | verdict |
|---|---|
| `a2x` doubles `q`'s implied rate | **S1** — Q inherits D's density |
| `q`,`n_q0`,`n_q1` all share one implied rate across `s_none`/`s_ports` | **S2** — uniform seq default; record it |
| implied `d_nq0 ≈ clk/2`, `d_nq1 ≈ clk/4` | feedback divide captured — BDD-state model; record |
| `n_q0 = n_q1` despite different D cones | model ignores feedback structure — name which rule |

`s_full` supplies the exact rates (n_q0: 100 toggles, n_q1: 50, q: `a`'s
count shifted) — the propagated-vs-exact delta per seq net is recorded.
Sequential *internal* on the clock-pin blocks is expected to follow the SDC
clock (SAIF-`TC` finding) — re-confirm on this fixture and say so.

## P4 — partial annotation fallback (acceptance item 4)

Session `s_partial`: `mba_partial.vcd` annotates `clk`, `a`, `b` **plus**
`n_and` at a deliberately inconsistent duty (P≈0.80 vs the a&b-derived 0.44).

| observation | verdict |
|---|---|
| `n_or`'s implied value shifts toward the annotated `n_and` | internal annotation wins over re-derivation |
| `n_or` unchanged vs `s_ports` | downstream re-derives from inputs — annotation is boundary-only |

Also record the `vcd N / unannotated M` split: which pins/nets OpenSTA
counts as annotated vs falling back. Contingency variant `mba_partial2`
(internals annotated, ports bare) answers the mirror question — do
unannotated input ports get the input default while internals hold
annotation? The "which nets propagate vs stay zero" acceptance wording is
answered by the split + the implied-rate table per net, not by a binary.

## P5 — x/z and undriven nets (acceptance item 5)

- `s_xz`: `a` carries an x-run (two half-weighted edges). Does the seed's
  probability/density still read as the clean-edges value (x edges
  half-weighted, matching the measured edge rule) or does x/z time poison
  propagation? Compare vs `s_ports`.
- Constants and undriven nets are exercised by *every* session: `n_hi`/
  `n_lo` → `n_bh`/`n_bl` (constant through a buf), `n_float` (no driver),
  `n_flt` (driven by the floating input). With an accessor: read their
  {P, d} directly (`P=1/0, d=0`? `P` unknown/default? `x`?). Without one:
  state honestly that constant nets contribute ~nothing to `report_power`
  columns (d=0 ⇒ no switching), so the record is the annotation split's
  provenance categories + any warnings — the gap is named, not hidden.

## P6 — results.md entry (required content)

Dated entry (`docs.md` has the outline) covering:

1. P0 command surface verbatim — the `set_power_activity` spelling that
   parsed, `set_power_analysis` verdict, accessor found or not (and units).
2. The default-activity numbers (P1): input rate, internal/sequential
   default, clk-net treatment — each with its derivation method.
3. The combinational verdict (P2): M1/M2/M0 with the discriminating numbers.
4. The sequential verdict (P3): S1/S2/S3 with the `a2x` sweep and
   toggle/counter evidence; clk-pin internal re-confirmation.
5. P4 fallback semantics: split counts + the inconsistent-`n_and` verdict.
6. P5: x/z handling and the constants/undriven record (incl. limitations).
7. Which parts of `generate_activity_ref.sh` actually ran (AGENTS.md §7
   rule 9) and the full-suite checks (`cargo test --workspace`, clippy).
8. Disposition: implemented-with-golden, or the roadmap plan landed —
   with the plan's step list if the latter.

**Fail-proof for the implement branch** (AGENTS.md §7 rule 7): disable the
fill pass or force all unannotated nets to default → the mba golden test
goes red; corrupt one `s_ports` group number in the JSON → red; restore.
