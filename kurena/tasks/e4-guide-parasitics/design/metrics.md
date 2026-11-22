# e4-guide-parasitics — Metrics (done-when 3)

Exact definitions as the test and the `guide_caps.py diff` compute them,
so the cargo assertions and the `docs/results.md` entry measure the same
thing. Criterion text: `kurena/tasks/next-stage-scoping/design/done-when.md`
E4 row 3.

## Shared definitions

- Per-net cap, ours: `NetRcTree::total_wire_cap_pf` from
  `estimate_guide_parasitics` on the net's guide boxes.
- Per-net cap, reference: the `*D_NET` total from the committed
  `<d>.guide.caps.txt` (or, for (b), from the `*.ours.guide.spef`
  extracted by the same `guide_caps.py extract`).
- Net set: nets present in the reference caps file that our design
  resolves (`resolve_guide_nets`) with a driver and ≥ 2 pins — the same
  usable-net filter `spef_parasitics` applies. A reference net missing
  on our side counts as a failing row at `rel = ∞` and is listed by
  name; it is never dropped from the denominator.
- Error per net: `diff = ours_pf − ref_pf`; bound per net:
  `within = |diff| <= max(0.05 * ref_pf, 1e-3)` (pF — `max(5%, 1 fF)`).

## Direction (a) — our model on their guides (the gate)

Per design (`gcd`, `picorv32`):

- `parse_guides` the committed `<d>.guide` → `resolve_guide_nets` →
  `estimate_guide_parasitics`; compare `total_wire_cap_pf` per net
  against `<d>.guide.caps.txt`.
- **Gate: every compared net within `max(5%, 1 fF)`.** If the landed
  model leaves a residual class (e.g. a systematic off-by-one-pitch on
  stubs), the test asserts instead `within_fraction >= FLOOR` where
  `FLOOR` is the measured value — set only with the root cause named in
  `docs/results.md` (AGENTS.md §7.5: loosen a tolerance only after the
  root cause is named; per-net, not totals).
- Always printed (test output + results entry): total `Σ ours` vs
  `Σ ref` (anchor: gcd ref ≈ 0.7397 pF), fraction inside bound, median /
  p90 / max |rel diff|, count outside bound, worst-5 nets by |diff|
  with names and their box counts, and the sign split (over vs under).
- Also reported per layer band for diagnosis: Σ implied length per
  layer vs the reference's, if the probe shows per-layer RC — the
  quickest way to see which layer's model is off when a net fails.

## Direction (b) — their model on our guides (recorded, not gated)

Per design:

1. `grus-eda route --guide-out runs/guide-check/<d>.ours.guide` on
   `<d>.placed.def` (deterministic — byte-identical reruns, asserted by
   the e4-1 determinism test).
2. `tests/designs/<d>/check_guide_readback.sh runs/guide-check/<d>.ours.guide`
   → `runs/guide-check/<d>.ours.guide.spef` inside the pinned image.
3. `guide_caps.py extract` on that SPEF, then `guide_caps.py diff` vs
   `<d>.guide.caps.txt` — two OpenROAD SPEFs differing only in guide
   input, so every residual is guide quality downstream, not our RC
   code.
- Recorded: same stats as (a) plus the (b) totals next to (a)'s — the
  interesting readout is how much of (b)'s residual is already present
  as guide topology divergence (e4-1's per-net share) rather than
  parasitics error.

## `docs/results.md` entry shape

One dated entry `e4-guide-parasitics` recording, in existing style:

- the probe block first (`probe.md` §"What gets recorded") — the
  `*D_NET` structure answers and the `_000_` decomposition;
- commands run: `generate_ref.sh` step 5d per design (naming the step,
  §7.9), each `route`/`check_guide_readback.sh`/`guide_caps.py`
  invocation, and which cargo tests ran;
- per design and direction: the stats above in a table
  (`| design | nets | within max(5%,1fF) | median | p90 | max | Σ ours | Σ ref |`),
  the worst-5 nets with root cause per net (stub handling, layer RC,
  uncovered pin fallback, topology divergence for (b));
- the gate verdict for (a) with the actual fraction; if < 100%, the
  named exclusion/residual class;
- runtime of `estimate_guide_parasitics` per design (from `--nocapture`
  or a `grus-eda sta --parasitics guide` timing line);
- the fail-proof mutation that was applied and restored (`tests.md`);
- explicit note that (b) is recorded-not-gated, and what was run vs not
  (§7.9).
