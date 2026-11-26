# Documentation, goldens, versioning

## `docs/e4-guides.md`

- §7 (determinism): box order is now **semantic** — `write_guides` emits
  the builder's pairing-aware order, not a layer-major sort; document the
  stub-interleave rules (start/interior/end/uncovered).
- §10: document dead-island semantics — unreachable components keep `*CAP`
  and internal `*RES`, unpaired stubs emit `n n r` self-loops, no
  inter-component bridge; dead pins get zero wire delay; update the
  `SPEF_STUB_RES_KOHM` paragraph and the `spef_parasitics` bail note (the
  reader now accepts dead pins; `_057_` no longer bails).
- §11: update the e2e entry with the new decomposition outcome.

## `docs/results.md` — new dated entry

- Stub census before/after (ours vs OpenROAD, by R value and layer);
  dominant class named; residual split inherent vs fixed.
- Island-model change summary + the pin-cap decision and its evidence.
- e4-1 topology numbers after splitting (share, delta), e4-2 gate status
  (must stay 100%), e4-3 read-back result, decomposition table before/
  after, new `Tol` floors with justification.
- What was run: exact script steps, container command, release runs,
  `git diff` status of every golden (non-guide goldens byte-identical).

## `docs/status.md`

- E4 section: honest status — whether the strict criterion now passes or
  which named residual remains (topology axis expected dominant).

## Goldens (per AGENTS.md rules)

- Regenerated via the committed scripts, never hand-edited:
  `generate_guide_ours_spef.sh` (host, release) and `generate_ref.sh`
  step 9c (pinned container).
- Commit if < 1 MB: `gcd.guide.ours.spef`, `gcd.guide.sta.reference.json`.
- Regenerate-not-commit (> 1 MB, gitignored): `picorv32.guide.ours.spef`,
  `picorv32.guide.sta.reference.json`, `picorv32.guide.spef`.
- `git diff` on all other goldens must be empty after the reruns; name the
  exact script steps executed.

## Versioning

Patch bump `[workspace.package] version` (0.3.11 → 0.3.12) in the same
commit as the functional change; `write_spef` stamps `*VERSION` from it —
regenerate `*.guide.ours.spef` **after** the bump so the committed SPEF
carries the final version.
