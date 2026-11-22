# Goal

E4 ticket 3 — the closing criterion (done-when 4,
`kurena/tasks/next-stage-scoping/design/done-when.md`):
end-to-end STA on **our guide parasitics** vs OpenSTA on
the `read_guides`-driven `-global_routing` SPEF.

Comparison chain: `grus-eda route --guide-out` →
`estimate_guide_parasitics` → `write_spef` → our `sta
--spef`  vs  committed guide SPEF → OpenSTA
`report_checks`/arrivals on `gcd.placed.def`/`gcd.sdc`
(and picorv32 equivalent).

## Gate

- gcd: every endpoint's arrival and slack within
  `max(5 ps, 1%)` — E3's strict bound.
- picorv32: floors follow the existing `routed_spef`
  rows in `docs/status.md` (arrival floor 6 ps, slack
  40 ps, slew 10%, TNS 2.5%) — record actual values.
- Reference artifacts via committed script: OpenSTA on
  the read_guides path producing the endpoint reference
  JSON (same pinned image), asserted by the strict
  integration test — not hand-written numbers.

## Work

- Wire the guide-parasitics SPEF into the `sta` path
  (already consumable via `--spef` per e4-2; add the
  committed `*.guide.ours.spef` references + the OpenSTA
  endpoint reference JSONs).
- Endpoint parity: same endpoint set, same fields
  (arrival/required/slack), same comparison harness the
  routed-SPEF tests use — reuse, don't fork.
- If divergence appears: per-§7.5 root cause first —
  name whether it's guide topology (ticket-1 residual),
  parasitics (ticket-2's island/stub caveats), or the
  `EST-0026` warned-pin dangling caps — not a looser
  tolerance without a named cause.

## Measurement

`docs/results.md`: probe conditions, per-endpoint tables
(worst diffs, share inside bound), the (a)-chain vs the
OpenSTA reference, what remains (named).

## Files

- `tests/designs/{gcd,picorv32}/` (`*.guide.ours.spef`
  committed outputs or regenerate-scripts + OpenSTA
  endpoint reference JSONs + generate script steps)
- `crates/grus-core/src/sta/` only if a hook is missing
  (prefer wiring through the existing `--spef` path)
- `docs/results.md`, `docs/roadmap.md`, `docs/e4-guides.md`
- `Cargo.toml` (bump per change size)
