# xdomain-deferrals — Doc updates

## 1. `docs/results.md` — dated entry (two parts, may land in one)

Probe part (commit (a)):

- Header: date, fixture (`sta_clocks`), reference tool
  (`openroad`/`sta` in `ghcr.io/librelane/librelane:3.0.16`), what ran
  (`probe.sh` whole script, rerun-count for byte stability).
- Per item: scenario name, the verbatim OpenSTA evidence lines
  (`u_fy/D` unconstrained listing / `u_ln/D` `time borrowed` block /
  `t2_lcm*` table-warning-error-timeout), and the verdict
  (implements / stays deferred).
- Item 3 cites the already-committed `probe_t2_waverot.txt` +
  `t2_waverot` golden rows as its artifact.
- Our-side numbers for implemented items: per-endpoint arr/req vs golden
  at STRICT, worst |Δ|; for kept deferrals the pinned skip text.
- State exactly which steps of `probe.sh` ran (AGENTS.md §7.9).

Implementation part (commit (b)/(c)):

- What changed (`hold_edge_pair` reformulation; any item-2 latch code),
  the STRICT compare numbers, the new pin asserts, the fail-proof
  mutation run and restore.

## 2. `docs/multi-clock.md`

- §1: add the measured rules the probes produced — at minimum the
  capture-driven hold-pair rule (`t2_waverot.u_fy/D`: boundary launch
  edge pairs with the largest capture edge at-or-before it; equivalently
  "smallest launch edge at-or-after each capture edge") as a §1.7
  refinement; item-2/item-4 outcomes as they measure.
- §2 deferral list: rewrite the four-item sentence — each item becomes
  either a resolved line (moved to "Now evaluated" with its scenario
  citation) or a kept deferral with its probe-artifact pointer and
  "defensive/unreachable" or "OpenSTA also skips" status.

## 3. `tests/designs/README.md`

`sta_clocks` section: update the scenario count and deferral sentence
(the "remaining `CrossDomain` deferrals ... pinned as named skips"
clause) to reflect the resolution; name the new `t2_*` probe scenarios
and their `MEASURE_ONLY`/`XDEFER` status.

## 4. `docs/roadmap.md`

The pointer block (~lines 332-341) listing the four `CrossDomain`
deferrals — update to the post-resolution status (implemented items get
a version citation; kept deferrals keep the named list).

## 5. Version

`Cargo.toml` `[workspace.package] version` `0.2.11` → `0.2.12` (patch —
feature completion, no interface break), `Cargo.lock` refreshed via
`cargo check`, in the same commit as the functional change (AGENTS.md §5).

## 6. Not touched

`README.md`, `docs/e3-sta.md`, `docs/status.md`, `docs/path-delays.md` —
none mention the four deferrals by name; revisit only if an
implementation branch changes user-visible `--strict`/`--json` output.
