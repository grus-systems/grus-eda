# deferred-check-arcs — Documentation updates

All prose English, `docs/` style. Branch-dependent wording: write the
`results.md` entry at probe time while the transcripts are at hand, then
make the `roadmap.md`/`e3-sta.md`/`README.md` edits match the measured
verdict in the implementation commit.

## 1. `docs/results.md` — dated entry

Title: `## <run date> — deferred check arcs: non_seq/three_state/preserve
probe + fixture`. Contents per `probe.md` "Recording format": committed
script, census table (`overview.md`), Q0 surface list, per-type verdict
with verbatim evidence lines, our-side `--json` skip block + `--strict`
verdict, and which parts of `generate_ref.sh` ran. If an evaluated type was
implemented, append the comparison table (golden vs ours, per-mode, worst
|Δ|) and the fail-proof note — same shape as the `min_pulse_width` entry.

## 2. `docs/roadmap.md` — N3 "Still deferred" tail + R6 residual line

- N3's final bullet (`Still deferred: non_seq_*, three_state_*, preserve,
  and async Clear/Preset stay counted …`): replace the type list with the
  post-verdict set; each type that stayed deferred gets its measured
  justification inline (e.g. "OpenSTA reports no `non_seq` rows on any
  surface — `report_check_types -data_setup` has no such flag / emits zero
  rows (measured …)" — verbatim the measured phrasing). Types that moved to
  evaluation get a sentence like the `min_pulse_width` one (version,
  fixture, surface, STRICT).
- R6's named-residual bullet already defers `non_seq_*` to this item's
  verdict — update it to the final disposition and drop the "future OpenSTA
  version re-measurement" clause only for the types this probe closed;
  keep it for `preserve` (unmeasurable on this lib).

## 3. `docs/e3-sta.md`

The line ~228 "…`min_pulse_width` is a separate evaluated check family …
and other non-delay arcs are deferred and counted." — name the deferred set
explicitly post-verdict (and name any newly evaluated family). If a data-
check family was added, one sentence on its model goes next to the
`pulse_width_checks` sentence.

## 4. `README.md` — skip paragraph

The deferred list in the research-prototype paragraph currently reads
"(`non_seq_*`, `three_state_*`, `preserve`, async `Clear`/`Preset`, and
`min_pulse_width` arcs whose pin is off the clock network …)". Update the
parenthesised list to the post-verdict set; if a new evaluated family
exists, name it alongside "clock-pin `min_pulse_width` arcs are evaluated".

## 5. `tests/designs/README.md` — sta_edge bullet

Add a `check_arcs` line in the `async_arcs` style: hand-written,
`sky130_fd_sc_hd` only; `dfbbp`/`dfbbn` flops carrying the `non_seq_*`
`SET_B`/`RESET_B` pair plus `clear`/`preset`, and `ebufn`/`einvn`/`einvp`
tristate drivers feeding capture flops; `te_slow.sdc` is the three_state
propagation discriminator; golden + `probe_check_arcs_*.txt` from the
committed `generate_ref.sh`.

## 6. `docs/results.md` census note

The census table in `overview.md` is the acceptance's "lib census recorded"
item — restate it in the dated entry with the re-count date so the record
lives in `docs/`, not only in `kurena/tasks/`.

## 7. Version

`[workspace.package] version` `0.2.10` → `0.2.11` in root `Cargo.toml`;
`Cargo.lock` refreshed by `cargo check`. Same commit as the functional
change (patch bump — feature+fixture per AGENTS.md §5). If the probe yields
the all-deferred outcome the change is test+docs only — still patch.
