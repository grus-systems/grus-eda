# Goal

E5 ticket 4 (final): score our CTS DEF against the
committed TritonCTS references per the scoping
done-when (`next-stage-scoping/design/done-when.md` §E5).

## Criteria (per design)

1. **Skew** within `max(25 ps, 25%)` of TritonCTS —
   gcd ref ≈ 0 ps (25 ps floor operative); ours
   measured spread 2.2 ps. picorv32 ref −34.16 ps;
   ours 70.6 ps — evaluate honestly.
2. **Max insertion delay** within 15% — gcd ref
   0.313–0.315 ns vs ours 0.304 ns; picorv32 ref
   0.757–0.793 ns vs ours 0.73–0.80 ns.
3. **End-to-end STA**: `sta` propagated-clock on our
   `<d>.cts.ours.def` vs the committed
   `<d>.cts.sta.reference.json` golden (OpenSTA on the
   TritonCTS DEF) — endpoint arrival/required/slack
   within `max(5 ps, 1%)` on gcd; picorv32 on its
   recorded floors.
4. **Honesty gate**: inserted buffer count and
   clock-net wirelength reported next to TritonCTS
   (informational — tree shape is legitimately free).

## Work

- Emit our CTS DEFs deterministically (`grus-eda cts
  --def-out tests/designs/<d>/<d>.cts.ours.def`; commit
  gcd's if < 1 MB, gitignore picorv32's like the
  reference does).
- Comparison test `<d>_cts_e2e`: run propagated STA on
  our DEF, deserialize the committed STA golden, assert
  per-endpoint within bound — same harness shape as the
  E4 guide-STA comparison.
- Record the four criteria in `docs/results.md` with
  per-endpoint tables; decompose any failure into
  tree-shape vs engine vs reference-setup axes —
  no unexplained loosening (floors only via named
  cause + evidence).
- E5 verdict stated in `docs/status.md`: met / named
  residual per criterion per design.

## Files

- `crates/grus-core/src/cts/` (comparison test +
  whatever scoring glue it needs)
- `tests/designs/<d>/<d>.cts.ours.def` + comparison
  script if a new one is warranted (prefer reusing
  `generate_cts_ref.sh` conventions)
- `docs/results.md`, `docs/status.md`,
  `docs/e5-cts.md`, `docs/roadmap.md` (E5 verdict)
- `Cargo.toml` (patch or minor — the change's size
  decides; record the choice)
