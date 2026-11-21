# AP4 — `user` overrides, final precedence, gcd re-comparison: overview

Closing ticket of the AP1–AP4 sequence
(`kurena/tasks/activity-propagation/design/api.md` §Plan; measured
semantics: `docs/results.md` 2026-11-21 N7 probe; golden:
`tests/designs/microbench/mba.activity.reference.json`, 12 sessions ×
43 pins, already committed).

## Status — base implementation committed, revision pending

The feature work of this design **already landed** in `22b96d6`
(v0.3.0): `ActivityOrigin::User`, `ActivityOverrides::pins`,
`dead_user_entries`, the final seed precedence, the two VCD-reader
fixes the gcd re-comparison exposed, the 12-session golden assertions,
the gcd `report_power` table, and the first `docs/results.md` entry.
The card was rejected on reproducibility and provenance grounds — the
feature code stands; this revision adds a diagnostics surface, one
test hardening, and documentation corrections. Everything below marked
**R1** is the remaining work.

## Review memo → revision items (R1)

1. **Stale N7 paragraph in `docs/roadmap.md`** — the status text still
   says "OpenSTA propagates activity statistically … our exact
   per-net counts are reported instead". That root cause is dead: the
   AP4 results entry names VCD-reader corruption (flat-name
   `pin_signals` folding + dropped short `b` writes) as the actual
   cause of the 2026-11-18 gap, and our model now propagates too.
   The paragraph must be rewritten (docs.md §2).
2. **`dead_annotation=1286` is not reproducible from the committed
   CLI** — `dead_annotation_entries` lives on `ActivityResolution`
   only; `grus-eda power` cannot print it. Revision: carry both dead
   counters on `PowerDiagnostics` and print them on the `activity:`
   diagnostic line (api.md §7) — the memo's preferred option, which
   makes the figure CLI-derivable. An independent VCD census
   cross-check is specced in gcd.md §provenance (1509 `pin_signals`
   declarations: cell output pins resolve as drivers; non-driver cell
   pins, cell internals, and io-unmatched dut wires are dead).
3. **`gcd.vcd` provenance** — the entry must record where the file
   came from (Icarus `$version`/`$date` header lines, the
   `gcd_tb.v` + `gcd.synth.v` + PDK verilog-models recipe, whether it
   was re-simulated this session) and the diagnostics movement lines
   (gcd.md).
4. **Card question, answered in the entry** — whether
   `power_vcd_check.tcl` actually re-ran this session (re-run it in
   the LibreLane container and quote verbatim, or correct the claim to
   the 2026-11-18 record) and where `1286` was read from
   (`ActivityResolution` via a non-CLI path; now the CLI line —
   gcd.md §card-question).
5. **Lows** — the results entry must restate the residual notes
   (`unknown` `prob_one`/`when` semantics, `user`-vs-dump precedence
   unmeasured, duplicate `-pins` last-wins unmeasured), and
   `dead_user_entries` gets exercised (`== 1` via a dead `pins`
   entry on `u_or/A`) rather than only asserted `== 0` (golden.md).

## What landed (base, kept)

1. **`ActivityOrigin::User`** — `input_ports` no longer folds into
   `annotated`; the golden `user` label compares natively.
   `ActivityOverrides::pins: Vec<(PinId, ActivitySetting)>` is the
   `-pins <list>` surface; both `user` arms sit below
   `global`/`clock`/`constant` and above dump annotation — the
   measured precedence.
2. **Call-site `-activity` normalization, pinned** — absolute-rate
   `ActivitySetting::density_per_s`; `s_p40` pins the
   clock-normalized default inside `resolve_activity`.
3. **Golden coverage** — `s_input_ports`/`s_p40` verbatim;
   `origin_counts` `{clock 1, constant 4, input 1, propagated 6,
   unknown 4, user 2}`; the `pins`≡`input_ports` equivalence; all 12
   sessions strict; three fail-proofs recorded.
4. **gcd `report_power` re-comparison** — every group within ~2% of
   OpenSTA under the same VCD seeds; the VCD-reader root cause named.
5. **VCD-reader fixes** (implementation-found, committed):
   per-declaration `pin_signals` streams (same-named `Y`/`X`/`Q` no
   longer fold) and left-extending short `b` writes — two unit tests
   pin them.

## Revision decisions recorded

- **`dead_*` counters ride `PowerDiagnostics`** (api.md §7) — it is
  the only report surface the CLI and `--json` serialize; the
  `activity:` line appends `dead_annotation=`/`dead_user=`
  unconditionally (diagnostics, not origin buckets — a nonzero count
  can never hide behind the zero-skip).
- **`mb.power.json` expected diff is named, not empty** — the two new
  serialized diagnostics keys are the whole diff; the "git diff
  empty" record from the v0.3.0 entry is corrected (docs.md §1).
- **`dead_user_entries` exercised, not just asserted** — a `pins`
  entry on the sink pin `u_or/A` yields `dead_user_entries == 1`
  while the per-net vector stays identical to `s_input_ports`:
  stronger than `== 0` (it proves dead entries cannot perturb
  resolution).
- **Honesty over re-execution** — for `gcd.vcd` and
  `power_vcd_check.tcl` alike, the entry records the exact command
  and whether it ran this session; carried-over artifacts are named
  as such, never implied runs.
- **Version `0.3.0 → 0.3.1`** — patch: additive diagnostics fields
  and a print-line extension (0.x rule: features bump `patch`).

## Files

- `crates/grus-core/src/power.rs` — `PowerDiagnostics` dead fields +
  population; `via_pins` dead-entry exercise
- `crates/grus-eda/src/main.rs` — `activity:` line appends the dead
  counters
- `docs/results.md` — AP4 entry revision block + corrections
- `docs/roadmap.md` — stale N7 paragraph rewrite
- `tests/designs/README.md` — gcd `power/` provenance bullet
- `tests/designs/microbench/mb.power.json` — regenerated, named diff
- `Cargo.toml` (0.3.1), `Cargo.lock`
- `kurena/tasks/ap4-activity-precedence/design/` — this document set
