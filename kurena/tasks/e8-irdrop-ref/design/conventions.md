# e8-irdrop-ref — Current-source conventions

The E8 done-when's open item: PDNSim can take per-instance current draw
two ways, and the golden ticket must pin which one our engine mirrors —
with the measured delta between them as evidence.

## Convention (a) — `embedded`

`analyze_power_grid` with no `set_pdnsim_inst_power` calls. PDNSim
computes each instance's power itself through OpenSTA's power analysis
(the command doc: `set_pdnsim_inst_power` "should only be used when
needing to override the computed power"). P-IRD's 1.04 mW / 0.591 mV
numbers are this convention.

## Convention (b) — `sta_per_instance`

Every instance's power set explicitly via

```tcl
set_pdnsim_inst_power -inst <name> -power <total_watts>
```

fed by `report_power -instances [get_cells *] -digits 12` **produced in
the same container run** (script.md step A2 → step B.5 → step C). The
feed uses the report's Total column in watts — internal + switching +
leakage — which is the quantity PDNSim converts to current draw at the
rail voltage.

Why this is the expected mirror: our N7 `grus-eda power` already emits
per-instance power from the same Liberty model + activity inputs, so
the E8 engine's natural architecture is "compute per-instance power
with our own estimator, attach as current sources" — i.e. convention
(b)'s structure with our numbers substituted for OpenSTA's. Pinning (b)
means the golden compares like-for-like: the reference's explicit-feed
result vs. our explicit-feed result, with the estimator difference
isolated as its own measurable quantity later.

## What the golden must establish

1. **Both measurements exist.** Per net (`VPWR`, `VGND`), per
   convention: full metric block + voltage CSV (`golden.md`).
2. **The delta.** `delta` block per net: `sta_per_instance − embedded`
   on worst drop, average drop, total power, average voltage; plus
   `voltage_maps_identical` (byte-equality of the two CSVs).
   - If the delta is ~0 (plausible: both use OpenSTA per-instance
     power, so the explicit feed may reproduce the embedded estimate
     almost exactly), that is itself the key evidence — the choice of
     convention then costs nothing and the E8 engine can mirror (b)
     knowing it reproduces the reference's default regime.
   - If nonzero, the delta quantifies how much of the reference's
     result is the estimator's convention rather than the solver's —
     the number our engine must budget for when it substitutes N7
     power.
3. **The pinned choice.** `convention_mirrored` in the JSON +
   `docs/results.md` records the decision and the measured delta as its
   justification.

## Decision rule

Mirror `sta_per_instance` unless the measurement shows it is
**unrepresentable** — e.g. the `report_power -instances` surface cannot
cover the instance set (abort path in `script.md` step B.4) or
`set_pdnsim_inst_power` errors on real instance names. In that case the
script fails loudly, convention (a) stands alone in the committed
artifacts that did emit, and the fallback is named in
`docs/results.md`. Choosing (a) while (b) measured clean needs a
written reason in the commit — it contradicts the engine's planned
architecture.

## Relationship to the scoping gate

Done-when criterion 3 (`done-when.md` §E8) asks: what drives the
reference's 1.04 mW total — the embedded estimate or explicit
annotation? This ticket answers it with numbers: the embedded total,
the `report_power` total, the explicit-feed total, and the resulting
drop deltas, all in one golden. Note the input difference from P-IRD:
that probe did not read the SDC, while this script does (matching the
other flows' read set) — if the embedded total drifts from 1.04 mW,
that is a measured correction to the scoping number and is recorded in
`docs/results.md`, not smoothed over.
