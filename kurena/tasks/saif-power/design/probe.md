# Measurement plan — what must run inside the container and land in results.md

All reference runs inside `ghcr.io/librelane/librelane:3.0.16` via
`openroad -no_init -no_splash` (stdin heredoc — remember AGENTS §7 rule 3:
`!` is history expansion there; write `[...] == 0`). Mount repo `/work`,
PDK `/pdk`. The 2026-11-19 planning probes below are already measured facts;
the task's job is to re-run the ones that produce **committed numbers** via a
**committed script**, then record everything in `docs/results.md`.

## P1 — field semantics (recorded, no golden)

Probe Tcl scripts under `tests/designs/microbench/` (a `probe_saif.tcl`
driven by `generate_saif_ref.sh`, or inline variants in the script — keep
the committed artifact set small). Confirm on the committed `mb.saif`:

- `TC` scaling: report_power linear in TC (probe: TC ×10 → switching ×10).
- `T0`/`T1` swap: changes leakage only (state-dependent), internal/switching
  identical → names the duty→leakage mapping.
- `IG`/`TX` non-zero: no `report_power` change.
- `report_activity_annotation` counts (`saif N` / `unannotated M`) for the
  committed fixture vs a ghost-scope variant.

## P2 — hierarchy resolution (recorded)

From the same script, `read_saif -scope tb/dut` on `mb.saif` prints
`Annotated N pin activities.` — record N and the `saif`/`unannotated` split,
and document the measured model:

- scope-NET names resolve to pins/ports of that instance scope (top scope →
  top ports only; internal net names dropped silently);
- `(INSTANCE _d_)` scopes annotate `_d_`'s pins via `NET` groups;
- `PORT` groups annotate nothing in this build;
- unmatched instance scopes and signal names: silently dropped (ghost probe
  confirms no warning).

## P3 — SAIF-vs-VCD agreement (the golden)

`generate_saif_ref.sh` (container self-invoking, `sta_edge` convention) runs
two `openroad` sessions on identical design inputs (`mb.def`, `mb.sdc`, same
lef/lib):

- `read_vcd -scope tb/dut mb.vcd` → `report_power` (the session must
  reproduce the committed `mb.openroad.txt` table — verify agreement,
  emit the `power_vcd` JSON block from the same run; no second raw file);
- `read_saif -scope tb/dut mb.saif` → `report_power > mb.saif.openroad.txt`.
  Add `-digits 6` to `report_power` only if the build accepts it
  (`fixture.md` precision contingency).

The Tcl then parses both tables and emits `mb.saif.reference.json`:

```json
{
  "power_vcd":  {"Sequential": {...}, "Combinational": {...}, "Clock": {...}, "total": {...}},
  "power_saif": {"Sequential": {...}, "Combinational": {...}, "Clock": {...}, "total": {...}}
}
```

Each group: `internal_w`, `switching_w`, `leakage_w`, `total_w` — parsed
from the table, not typed. `power_saif` is the STRICT-test golden;
`power_vcd` records the same-window agreement. Note `clk`'s TC is odd
(199), so the golden exercises the odd-TC rise/fall split — confirm the
committed `mb.saif` carries `TC 199` / `T0 1000` / `T1 990` on `clk`.

**If regen contradicts a recorded probe finding** (per-group numbers off
beyond print rounding, a claimed quirk absent, `read_saif`/`report_power`
surface different): the committed container run wins — emit the evidence
verbatim in `results.md`, adjust the reader/tolerance to the *measured*
semantics, and correct these design docs in the same change. Never
extrapolate a planning-probe claim past what the committed run shows.

## P4 — results.md entry (required content)

Dated entry covering:

1. The measured field→activity mapping table (P1) and hierarchy-resolution
   model (P2), including the silent-drop and PORT-group quirks.
2. SAIF-vs-VCD table: per-group + total, both tools' columns where
   meaningful (OpenSTA-SAIF, OpenSTA-VCD, ours-SAIF, ours-VCD).
3. Residual divergence with **named root causes** — expected: Sequential
   switching ~3-4% (same residual class as the VCD path; investigate whether
   it is the driver-pin output cap or wire-load estimate before loosening
   the bound), duty→state-dependent-leakage not modeled, `q`-net propagation
   vs exact count.
4. Which parts of `generate_saif_ref.sh` were actually run (AGENTS §7 rule 9).
5. Fail-proof check: corrupt one field of `mb.saif` (e.g. double `clk`'s
   TC), confirm the comparison test goes red, restore.
