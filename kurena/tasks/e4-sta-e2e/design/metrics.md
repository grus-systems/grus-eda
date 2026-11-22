# e4-sta-e2e — Measurement and attribution

What the `docs/results.md` entry must contain (dated
`e4-sta-e2e` entry, following the e4-guide-parasitics entry's shape).

## Probe conditions

- Pinned image `ghcr.io/librelane/librelane:3.0.16`; the three
  `openroad` invocations of `generate_ref.sh` step 9c per design.
- Our side: `generate_guide_ours_spef.sh` — `target/release/grus-eda
  route --guide-out` + `sta --parasitics guide --guide --spef-out`
  (release build; record binary version from `write_spef`'s
  `*VERSION` line).
- `EST-0026` warning counts on the `guide_ours_readback` run
  (gcd / picorv32 — the e4-2 read-back counted 11 / 249; re-count from
  the step-9c log).
- Whether `read_spef` accepted our `write_spef` output on both designs
  (the interop check), verbatim.

## Per-endpoint tables

Per design:

- **Gate**: `our sta --spef <d>.guide.ours.spef` vs `guide_spef` — the
  harness's per-field max relative diff plus the worst-5 endpoints by
  absolute arrival/slack diff (pin name, ours, ref, diff), and the share
  of endpoints inside `max(5 ps, 1%)` (gcd) / the picorv32 floors.
  Include the `wns_*`/`tns_*` diffs the harness prints.
- **Decomposition** (from the decomposition tests' prints): worst-5 per
  axis —
  - engine: our sta − `guide_ours_spef`;
  - parasitics: `guide_ours_spef` − `guide_ours_readback`;
  - topology: `guide_ours_readback` − `guide_spef`;
  each row naming its cause axis.
- Endpoint count equality (asserted by the harness — state it).

## Attribution rule (§7.5)

Any endpoint outside the gate bound gets a named cause before the entry
closes; read the decomposition table as:

- engine diff large → STA-engine residual (compare with `routed_spef`'s
  recorded residuals — should be the same band);
- parasitics diff large → e4-2 model residual (island caps, stub
  treatment, `EST-0026` attach-vs-dangle);
- topology diff large → e4-1 guide residual (coverage/strip/layer
  band);
- if only the total is large with all three axes small, that is a new
  interaction — investigate, do not loosen.

Tolerance is only ever revisited after the cause is named in
`docs/results.md`; a named-cause bound widening, if needed, is stated
there with the number, not smuggled into `Tol`.

## What-else to record

- Fail-proof mutation + restore result.
- Runtime: `sta --parasitics guide` and `sta --spef` wall times on both
  designs (`GRUS_PHASE_TIMING` breakdown for `sta.parasitics` /
  `sta.spef.read`).
- "What was run" per §7.9: which steps of which scripts, in which
  environment (host release build vs container), and which artifacts
  were regenerated vs reused.

## Docs sync

- `docs/e4-guides.md` §1 table: the E4 e2e row moves from "deferred to
  the next E4 ticket" to the measured verdict; §10 or a new short §11
  states the three-block golden design and which cell is gated vs
  diagnostic (keep in step with code, §7.8).
- `docs/status.md` E4 row: criterion-4 verdict, tolerances used, and
  refresh "known gaps" (island nets in OpenROAD's own guide SPEF remain
  unreadable by `spef_parasitics`; `EST-0026` attach-vs-dangle residual;
  met3–met5 banding still absent from our guides).
- `docs/roadmap.md` E4 progress line: criterion 4 measured; the
  remaining open item becomes guide-quality improvements (layer band,
  coverage), not the e2e harness.
