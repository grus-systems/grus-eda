# async-latch-delay — Implementation shape (per verdict)

All changes live in `run_sta_partitioned`'s composition loop
(`analysis.rs:3373-3605`). The signature-class machinery, selector
resolution, `Worst`/`EndpointPolarities` composition, and the skip pass
need no structural change — a delay check on a new endpoint kind is a
new arm of the same composition.

## Verdict: applied on async-check pins

`u_ff0/RESET_B`-type endpoints are `Check { latch: false }` carrying
`recovery_arc`/`removal_arc` (never `setup_arc`/`hold_arc`) — they fail
the `delay_supported` predicate at `analysis.rs:3426-3432`.

- **Gate**: widen the `Check { latch: false }` arm from
  `c.setup_arc.is_some() || c.hold_arc.is_some()` to
  `c.late_arc().is_some() || c.early_arc().is_some()`
  (`late_arc() = setup_arc.or(recovery_arc)`, `early_arc() =
  hold_arc.or(removal_arc)` — `analysis.rs:84-90`).
- **Max side**: if the measured rule is the FF formula verbatim
  (`req = capture_min[check.edge] − unc_setup − t_late + N + crpr`),
  the existing block (`analysis.rs:3456-3521`) needs no change — it
  already reads `check.late_arc()` and `check.clk`, so the recovery
  table and the async pin's clock pin flow through. If measurement
  shows a different rule, add the branch here with the measured
  formula, not an approximation.
- **Min side**: `req_hold_base + N` is already generic for non-port
  kinds (`analysis.rs:3536-3546`, `w.req += n`); the removal required
  time is `pols.min`'s base. Same caveat: measured rule only.
- **Per-transition tables**: the loop requires `constraint_table(late,
  pol)` per transition (`analysis.rs:3462-3470`); a missing fall
  recovery table just skips that polarity — matches the endpoint's
  ordinary candidate generation.

## Verdict: applied on latch-D pins

`Check { latch: true }` currently falls through to `false`.

- **Gate**: `Check { latch: true } => true` in `delay_supported`.
- **Max side**: three measured-rule branches, pick what the probe shows:
  - *closing-anchored (FF verbatim)*: no new code — `check.late_arc()`
    is the latch's setup arc and `check.clk` its gate pin;
  - *opening-edge anchored*: add a `check.latch` branch mirroring the
    borrow code's opening-edge computation
    (`analysis.rs:4111-4121`: `arrival_min[open_pol]` aligned by whole
    periods to the closing edge), then substitute N for the period
    term per the measured formula;
  - *edge-referenced*: `w.req = n − unc_setup`-style, like the
    output-port arm at `analysis.rs:3441-3455`.
  The delay check's slack is rigid (`slack = req − arr`) — no borrow
  clamp — which composing a `Worst{req, slack}` gives for free.
- **Min side**: the generic `w.req += n` block covers `hold_base + N`;
  a different measured rule gets its own branch.
- **`-to` coverage**: `resolve_to` already maps `get_pins u_lp/D`,
  `get_cells u_lp`, and `all_registers` (latch cells are `seq_cells`)
  to the latch endpoint pins — no selector work expected. Confirm the
  `delay_allregs` golden rows match, especially `u_iso/D` whose
  baseline slack is a pure borrow clamp.

## Verdict: ignored or errored

No `analysis.rs` behaviour change for that kind:
`unsupported_path_delay_endpoints` keeps counting it, the endpoint
keeps its clock-check values — which already reproduces OpenSTA's
"ignored" (values unchanged). For "errored", record the error in
`docs/results.md`; our engine still counts the endpoint rather than
failing, which is the louder behaviour and consistent with the counted-
skip contract. Update the predicate's comment and the
`unsupported_path_delay_endpoints` doc comment (`analysis.rs:337-341`)
to name the measured verdict, and the scope lists in
`docs/path-delays.md` §3.

## Mixed verdicts

Per-side and per-kind independence: the gate is `(kind, side)`-local in
the composition loop, so "applied on max, ignored on min" or "applied
on async, ignored on latch" each map to exactly the branches above —
no all-or-nothing switch.

## Tests & diagnostics

- New `compare_sta_edge` calls per `fixture.md` §3 at `Tol::STRICT`.
- `unsupported_path_delay_endpoints` shrinks for applied kinds; keep a
  diagnostic assertion — e.g. run the `delay_allregs` SDC and assert
  the count covers exactly the still-deferred kinds (clock-gate always;
  async/latch only if ignored).
- Fail-proof (AGENTS.md §7.7): for each applied kind, revert the gate
  or substitute a wrong req candidate (e.g. edge-referenced when
  capture-anchored was measured) and confirm the new scenarios go red;
  restore. For "ignored" verdicts the committed scenario is the
  fail-proof: it must stay byte-identical to baseline.
- `merge_corner_reports` (`analysis.rs:4476-4478`) merges the counter by
  max — unchanged.

## Docs deltas

- `docs/path-delays.md` §3: move applied kinds from "v1 diagnoses" /
  "Explicitly out of scope" into "v1 supports" with the measured req
  rule; reword ignored/errored kinds as measured outcomes, not
  unmeasured limits. §5 item 1: strike the async-reset and
  latch-borrow halves with the verdict and pointer to §1 (applied) or
  the results entry (ignored); keep the two-clock-domain half open
  with the blocker restated (multi-clock SDC unsupported). Add the
  measured req rule to §1 alongside the FF/output-port formulas.
- `docs/results.md`: dated entry with the probe record and, for applied
  kinds, our values vs golden and the fail-proof result.
- `docs/timing-exceptions.md`: only if the skip surface changes — its
  out-of-scope list names multicycle endpoint limits and delay-command
  qualifiers, not delay endpoint kinds; expected unchanged.
- `Cargo.toml` `0.1.94` → `0.1.95` (+ `Cargo.lock`) in the same commit
  as the functional change — or, if nothing applied, in the
  fixture+docs commit.
