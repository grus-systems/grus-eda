# docs — `results.md` revision, `roadmap.md` N7 fix, README provenance

The AP4 results entry landed in `22b96d6` and its numbers stand. The
revision repairs the *record* per the review memo — it does not
re-open the comparison.

## 1. `docs/results.md` — amend the AP4 entry

Inside the existing `### 2026-11-22 — N7 — AP4` entry:

- Append a `**Revision (v0.3.1) — reproducibility/provenance pass**:`
  block containing, in order:
  1. **Card question (a)** — `power_vcd_check.tcl`: whether it re-ran
     this session, the container invocation, and the verbatim
     `Annotated N pin activities` / `STA-1453` / `report_power` lines;
     or the corrected attribution to the 2026-11-18 record
     (gcd.md §card-question).
  2. **Card question (b)** — where `dead_annotation=1286` was read
     from pre-revision (non-CLI `ActivityResolution` surface — name
     the mechanism) and the verbatim `activity:` line now emitted by
     the committed CLI, plus the independent VCD census cross-check
     on the count's composition (gcd.md §provenance).
  3. **`gcd.vcd` provenance** — the paragraph from gcd.md: Icarus
     `$version`/`$date`, the `gcd_tb.v` + `gcd.synth.v` +
     `sky130_fd_sc_hd.v` recipe, scope/composition (223 + 1286), and
     whether the sim re-ran this session.
  4. **Diagnostics movement lines** — verbatim `activity:` (both
     runs) and `power:` eprintln values; `dead_annotation_entries`
     noted as a new surface with no 2026-11-18 counterpart.
  5. **Re-verified table** — the same three-column table;
     bit-identical expected (report-only change).
- Correct the **Moved goldens** line: `mb.power.json` regen now shows
  a named diff — `+"dead_annotation_entries": <n>` and
  `+"dead_user_entries": 0` under `diagnostics`, no watt column
  moved. (The "git diff empty" claim described the pre-revision
  surface; record both.)
- **Residual notes — restate so the entry stands alone** (memo low):
  - `unknown` nets carry `prob_one: Some(0.0)` — expressible `when`
    literals at weight 0; OpenSTA's own `when` weight on `unknown`
    unmeasured (recorded in AP3, restated).
  - `user`-vs-dump precedence is a **chosen** order — no committed
    session mixes `set_power_activity` with `read_vcd` on the same
    pin (unmeasured).
  - Duplicate `-pins` last-wins is **unmeasured** (repeated
    `set_power_activity` semantics assumed).
  - `-pins` vs `-input_ports` order chosen (unmeasured); per-pin vs
    per-net `s_full` `y` residual; OpenSTA's split counts `user`
    under `input N` while ours reports `user N` per net.
- **Checks run** updated: the R1 commands (cli runs, regen, container
  run status) plus `cargo test --workspace`, clippy `-D warnings`,
  `cargo test --release -p grus-core sta`.

## 2. `docs/roadmap.md` — fix the stale N7 paragraph

Current stale text (the "Status (2026-11-19)" block, mid-paragraph):

> On `gcd` the totals diverge because OpenSTA propagates activity
> statistically (BDD + `findSeqActivity`) rather than reading per-net
> VCD counts; our exact per-net counts are reported instead, and the
> residual is documented in `docs/results.md`.

Replace with the now-measured record (draft — tighten to fit the
paragraph's flow):

> On `gcd` the earlier "statistical propagation vs exact counts" gap
> turned out to be mostly VCD-reader corruption (same-named
> `pin_signals` folding, dropped short `b` writes) — with the reader
> fixes and the AP1–AP4 propagated model, `report_power` lands within
> ~2% of OpenSTA on every group under the same VCD seeds
> (`docs/results.md` 2026-11-22); the named residuals are the
> zero-wire-parasitics switching term and the state-weighted `when`
> leakage attribution.

The already-landed `**AP4 done**` tail of the status stays (it is
accurate); only the stale divergence sentence is replaced. Also fix
any residual-notes enumeration to name: the per-pin/per-net `s_full`
`y` gap, `unknown` `prob_one`/`when` semantics note, the unmeasured
`user`-vs-dump / `-pins`-vs-`-input_ports` / duplicate-`-pins`
choices, and the OpenSTA `input N` split-counting difference.

## 3. `tests/designs/README.md` — gcd `power/` provenance bullet

The README documents artifact provenance but has no `power/` entry.
Add a bullet under the gcd section (matching its terse style):

- `power/` — `gcd_tb.v` (committed Icarus testbench: LFSR stimulus,
  `$dumpvars(0, dut)`), `power_vcd_check.tcl` (committed OpenROAD
  reference: `read_vcd -scope gcd_tb/dut` + `report_power`),
  `power_check.tcl` (no-annotation variant). `gcd.vcd` is gitignored
  (~18 MB): Icarus Verilog output of `gcd_tb.v` + `gcd.synth.v` +
  `sky130_fd_sc_hd.v` cell models; regenerate with the recipe in
  `docs/results.md` 2026-11-22 (AP4 revision block).

## Files

- `docs/results.md` — AP4 entry revision block + Moved-goldens
  correction
- `docs/roadmap.md` — stale N7 paragraph rewrite
- `tests/designs/README.md` — gcd `power/` bullet
