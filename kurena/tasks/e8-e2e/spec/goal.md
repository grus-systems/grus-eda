# e8-e2e — IR-drop end-to-end gate vs PDNSim golden

E8 ticket 5 (after `e8-solve`). The stage's done-when comparison:
our voltage map vs the committed PDNSim `embedded` goldens on both
nets, both designs.

## Done-when (from roadmap E8)

- Worst-case IR drop within `max(5%, 5 mV)` of reference
- Per-instance voltages within `max(5%, 1 mV)` of reference

Applied per net: VPWR compares drops below supply; VGND compares
bounces above ground (the reference CSV's `Voltage` column is the
node's absolute potential — compare like-for-like absolute values so
both nets are one code path; the *drop/bounce* scalars are reported
with sign convention named in `docs/results.md`).

## Scope

1. Comparison harness (`tests/designs/compare_irdrop.py` or Rust test
   reading both CSVs): per-instance-terminal `|V_ours − V_ref|` max,
   median, P99; worst-drop delta; avg-drop delta; total-power delta.
   Row join key = (Instance, Terminal); assert identical row sets
   before comparing values.
2. Integration test `test_irdrop_matches_reference` gated on the
   committed goldens: gcd must PASS the done-when numbers;
   picorv32 runs and records (floor if it misses — named cause only).
3. Decomposition if gcd misses: re-run our solver fed with the
   reference's own per-instance powers (`generate_irdrop_ref.sh`
   regenerates `inst_power.tcl` inside the container — committed,
   reproducible). That isolates power-estimator residual from
   mesh/solver residual. Record both axes like the e4/e5
   decompositions.
4. Honesty gate: per-net maps, worst pins named (top-5 deviations with
   coordinates), no tolerance widening without a named residual.
5. `docs/results.md`: full table both designs + solver/model notes;
   `docs/status.md` E8 row; `docs/roadmap.md` E8 checked or residual
   named.

## Files

- `tests/designs/compare_irdrop.py` or equivalent Rust-side comparator
- `crates/grus-core/tests/` integration test
- `docs/results.md`, `docs/status.md`, `docs/roadmap.md`
