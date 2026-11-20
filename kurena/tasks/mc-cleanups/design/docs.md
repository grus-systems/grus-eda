# mc-cleanups — Documentation and record fixes

All doc work in one pass. Rule: documentation claims must match what the
code does and what was actually run — audit before editing, record
no-op findings honestly in the `docs/results.md` entry.

## D1 — `docs/timing-exceptions.md`: missing skip counters

The skip-surface section (~L238–260) lists `sdc_ignored_commands`,
`sdc_ignored_options`, `unresolved_exception_selectors`,
`false_path_endpoints`, and `unsupported_multicycle_endpoints`, plus a
blanket "skipped count includes unreached endpoints". Missing counters
that now exist in `SkipReport`:

- `unresolved_load_selectors` (load-selectors ticket): `set_load`
  selectors that resolve nothing, and per-pin/per-net annotations that
  cannot be carried into the parasitic path. Add a bullet explaining
  what is counted (name misses at `prepare_sta` resolution;
  annotations dropped because the parasitic mode cannot apply them).
- Audit the same section for `unsupported_path_delay_endpoints` and
  `cross_domain_endpoints` — if absent, add one-line bullets; both are
  documented in `multi-clock.md`/`path-delays.md` already, so the fix
  is a pointer plus the count, not a rewrite.
- If item A7 lands as a count, the `unresolved_exception_selectors`
  bullet must mention the `-to` resolved-but-invalid form (OpenSTA
  `STA-1551`) — update it in the same commit as the code change.

## D2 — `docs/results.md` entries

Three recorded gaps plus this ticket's own entry:

1. **"15 parseable" count** (~L3626): the `mc-xdomain` entry says "All
   15 parseable `t2_*` scenarios". `t2_grp` was parse-gated at that
   probe — the correct count is **14 parseable + `t2_grp` parse-gated**
   (15 total scenario names, `t2_grp` produced an SDC error and no ref
   block). Fix the sentence; do not touch the surrounding numbers.
2. **when-power `awk` command** (~L3182): the entry says "`awk` scans of
   the pinned lib" but does not record the command. Re-derive the
   counts the entry claims — `internal_power` `when:` 26 blocks over 13
   cells (`ebufn_{1,2,4,8}`, `einvn_{0,1,2,4,8}`, `einvp_{1,2,4,8}`);
   `leakage_power` `when:` 6244 groups — with a literal `awk`
   invocation against
   `sky130_fd_sc_hd__tt_025C_1v80.lib`, run it, and paste the exact
   command into the entry. A candidate shape to start from (verify and
   adjust — the entry must record what was *actually* run, per repo
   rule 9):
   ```bash
   awk '/^[[:space:]]*internal_power *\(/ {ip=1; lp=0}
        /^[[:space:]]*leakage_power *\(/ {ip=0; lp=1}
        ip && /when[[:space:]]*:/ {ipn++}
        lp && /when[[:space:]]*:/ {lpn++}
        END {print "internal", ipn, "leakage", lpn}' <lib>
   ```
   If the lib's `when` syntax or block layout defeats a one-pass count,
   use whatever pipeline actually produces the numbers and record that
   verbatim.
3. **saif-power `cargo test --workspace` lines**: the v0.1.96 entry
   (~L3144) already lists `cargo test --workspace (287+3 pass, 2
   ignored)` in "Checks run". Audit against the sibling-entry
   convention (`cargo test -p grus-core <area>` focused lines,
   release-mode lines where entries record them): add a line **only**
   for a command that was genuinely run and is missing; if the entry
   is complete, record the audit as a no-op in this ticket's entry.

## D3 — `tests/designs/README.md` fixture list

The `sta_clocks`-era fixture directories missing entries (verify with
`ls tests/designs` at implementation time — the list is: `microbench/`,
`sta_exceptions/`, `repro_extreme_ceff/`; `sta_clocks` itself is
documented at ~L63):

- `sta_exceptions/`: hand-written exception-partitioning fixture —
  describe `base.def`, the `ex_*.sdc`/`pm_*`/`md_*` scenario classes,
  which script regenerates `sta.reference.json` (`probe.sh` —
  confirm the filename), and that `compare_sta_edge`-family tests
  deserialize the golden at STRICT.
- `microbench/`: VCD/SAIF power fixture — `mb.def`/`mb.lib`-era inputs,
  `mb.vcd`/`mb.saif`, `power_vcd_check.tcl` + `power_saif` goldens and
  `generate_saif_ref.sh`/`vcd2saif.py`; reference `docs/results.md` N7
  entries. Confirm actual filenames by listing the directory.
- `repro_extreme_ceff/`: one-line description of the regression fixture
  (extreme-slew `Ceff` reproduction — confirm purpose from its README
  or the referencing test, then describe only what is verifiable).

Same style as the existing entries: provenance first (hand-written
vs flow output), what the golden pins, which script regenerates it.

## D4 — `power.rs` `driven_nets_without_activity` doc (~L49)

The field doc says it counts nets that "have a driver and no VCD/SAIF
annotation". Since when-power (v0.1.97) the meaning changed: an
unannotated driven net's `net_activity` is `None`, which also makes
`net_prob_one` fail — so a `when` literal on a pin of that net becomes
inexpressible and lands in `conditional_power_blocks` instead of
producing a `P(when)`. Restate the doc: "driven nets with no VCD/SAIF
annotation — these contribute no switching activity *and* make `when`
conditions on their pins inexpressible (counted there too)". Also
record the meaning change in this ticket's `docs/results.md` entry.

## D5 — `t2_mdx` description audit

`kurena/tasks/mc-xdomain/design/fixture.md:57` already reads
"pin-startpoint `-from`" — the spec's "delay-check rewrite on both
sides" phrasing is no longer present. Audit `rg 'rewrite|both sides'`
across `kurena/tasks/*/design/` and `docs/`: `steps.md:47` and
`api.md:269` use "delay-check rewrite" for the compose/uncertainty
accessor (correct usage — keep). If no stale `t2_mdx` description is
found, record the item as a no-op verification in `docs/results.md`.

## D6 — `docs/multi-clock.md` sync

- ~L23: "fifteen `t2_*.sdc` scenarios on `two_clock.def` plus
  `t2_latch.sdc`" → the current golden set (39 `two_clock` scenarios +
  `t2_latch`; 40 total) — or reword to "the `t2_*` scenario set" to
  stop the count going stale.
- §1.2 (~L62–64): "no cross-domain hold pair selection beyond the
  coincident grid was exercised" was true at the first probe but is
  superseded by §1.7's `t2_wave` finding in the same document — add a
  forward pointer ("see §1.7, `t2_wave`") or amend the sentence.
- §1.7: append the newly measured rules from `probes.md` (`-to`
  invalid-object warning, namespace priority on a name collision,
  `-clock`-less I/O anchor under `-waveform`, empty clock collections,
  post-wrap `-waveform` handling) in the same bullet style as the
  v0.2.5 revision.

## D7 — `docs/results.md` entry for this ticket

One dated entry covering:

- every probe scenario run, its file name, the verbatim OpenSTA
  warning/error lines (with STA codes), and the discriminator read-off;
- the rule implemented per item (or "no change — current behaviour
  matches", "no-op — already correct");
- code nits landed (A1, A3–A6, A10) and the docs edits (D1–D6);
- verification actually run: `cargo test --workspace` result, clippy,
  the golden `git diff` audit, fail-proof mutations;
- version bump note.

## Files

- `docs/timing-exceptions.md`
- `docs/results.md`
- `docs/multi-clock.md`
- `tests/designs/README.md`
- `crates/grus-core/src/power.rs` (field doc only)
- `kurena/tasks/mc-xdomain/design/fixture.md` (audit; edit only if stale text is found)
