# Documentation, goldens, versioning

## `docs/e4-guides.md`

- §6 (pin-access stubs): document the rung-stack rule — bound pin cells
  emit a contiguous zls stack anchored on the covering strip's layer so
  access layers at/above the strip chain into the wire node (replaces
  the chain/extra split description for the `high` case; `low`/end-cell
  conventions unchanged).
- §7 (determinism/order): add the bend via-stack rule — non-pin H∩V
  cells get a consecutive `[met1, met2]` pair appended after the `met2`
  section; box order stays semantic.
- §10: note that OpenROAD's own guides cover bterm cells on their access
  layers (`resp_val`/`req_rdy` evidence) and that our rung stacks are
  the documented approximation under a `met1`/`met2`-only band; record
  the residual dead-`*P` class.

## `docs/results.md` — new dated entry

- Dead-`*P` census before/after (ours vs reference vs OpenROAD
  read-back of ours), per-net residual list with the class named.
- OpenROAD bterm-coverage probe: how `gcd.guide` covers port cells
  (access-layer coverage chained to the route), the 7 reference dead
  `*P`s classified.
- Detour-island census before/after; dominant disconnected-component
  class named (non-pin H∩V cells without a bridging zls); residual —
  if any — attributed as route-path divergence with per-net evidence.
- Self-loop census before/after (bend `met2` self-loops on live nodes
  are an expected rise; state the bound).
- e4-1 share/delta after the coverage changes, e4-2 gate status,
  e4-3 read-back result, decomposition table before/after, new `Tol`
  floors with justification, and the explicit E4 criterion-4 verdict
  (met / named residual).
- What was run: exact script steps, container command, release runs,
  `git diff` status of every golden (non-guide goldens byte-identical).

## `docs/status.md` / `docs/roadmap.md`

- E4 row: honest criterion-4 status — met, or which named residual
  remains (expected: route-path divergence, not emission).
- `roadmap.md` E4 section: one progress bullet (per-ticket note).

## Goldens (per AGENTS.md rules)

- Regenerated via committed scripts only: `generate_guide_ours_spef.sh`
  (host, release) and `generate_ref.sh` step 9c (pinned container).
- Commit if < 1 MB: `gcd.guide.ours.spef`,
  `gcd.guide.sta.reference.json`, `tests/designs/guide_islands.py`.
- Regenerate-not-commit (> 1 MB, gitignored):
  `picorv32.guide.ours.spef`, `picorv32.guide.sta.reference.json`.
- `git diff` on all other goldens must be empty after the reruns; name
  the exact script steps executed.

## Versioning

Patch bump `[workspace.package] version` (0.3.12 → 0.3.13) in the same
commit as the functional change; `write_spef` stamps `*VERSION` — bump
before the final `generate_guide_ours_spef.sh` rerun so the committed
SPEF carries the final version.
