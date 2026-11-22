# Measurement plan

All reference numbers come from the pinned image
`ghcr.io/librelane/librelane:3.0.16` (`colima start --memory 12 --cpu 8`,
tools invoked as `sta`/`openroad`). Ours-side artifacts come from the
release binary via `tests/designs/generate_guide_ours_spef.sh`.

## 1. Stub census (before the fix — captures the baseline to beat)

- Ours-side: `check_guide_readback.sh` on
  `runs/guide-check/<d>.ours.guide` (both designs) → read-back SPEF →
  count `n n r` self-loop resistors grouped by R value and by layer:
  `0.0093` = unpaired `li1` stub, `0.0045` = unpaired `met1` stub,
  `0.00341` = unpaired `met2` stub, `0.00038` = met3. Count island nets
  (nets containing ≥ 1 self-loop). Baseline recorded: gcd **212 / 1,389**.
- Reference: same census on `gcd.guide.spef` (committed) and
  `picorv32.guide.spef` (regenerated) → baseline **152 / 325** (gcd).
- The dominant class is named from the R-value histogram (expectation:
  li1 + met1 stubs; see `stub-pairing.md`).

## 2. Model-side verification (after the island change, before guide change)

- `cargo test -p grus-core guide` — the e4-2 cap gates
  (`test_<d>_guide_parasitics_match_openroad`) must still read
  `269/269` and `6744/6744` inside `max(5%, 1 fF)`; `*D_NET` parity is the
  guard that dead-node caps are still summed.
- New unit fixture asserting dead semantics directly (see `tests.md`):
  a disconnected component keeps its caps in `total_wire_cap_pf`, its pin
  gets Elmore `0.0`, and `write_spef` emits no inter-component resistor —
  instead a `n n r` self-loop with the stub's cut-layer R.

## 3. OpenSTA pin-cap question (decision rule for `island-model.md`)

- After our SPEF is regenerated with dead semantics, `generate_ref.sh`
  step 9c produces the new `guide_ours_spef` block (OpenSTA reading our
  file). If the parasitics/engine axes show a systematic residual only on
  nets whose sinks are dead, evaluate the pin-cap-injection alternative
  (inject dead-pin cap at node 0) and re-measure. Record the chosen
  semantics and the numbers that decided it.

## 4. Guide-side re-measurement (after the ordering change)

- `generate_guide_ours_spef.sh <d>` (release) → new guide +
  `<d>.guide.ours.spef`.
- `check_guide_readback.sh` both designs → new self-loop census → before/
  after table in `docs/results.md` (212/1,389 → target: named reduction;
  residual split C4-inherent vs remaining order artifacts).
- e4-1 topology gates re-run (`test_*_guide_matches_openroad`): weighted
  share ≥ 0.70 and box delta ≤ 15% must still hold (split strips add
  boxes — record the new delta).

## 5. e2e re-measurement (the gate)

- `generate_ref.sh` step 9c on both designs in the image — all three
  golden blocks (`guide_spef`, `guide_ours_spef`, `guide_ours_readback`)
  regenerate because our guide and SPEF changed; commit the small ones
  (gcd `*.guide.sta.reference.json`, `*.guide.ours.spef`), regenerate
  picorv32's locally (gitignored, > 1 MB).
- `test_<d>_guide_sta_decomposition --nocapture` → new four-row table.
  Acceptance: the parasitics-axis median must collapse materially toward
  the engine band (gcd ~1–7 ps, picorv32 ~17–56 ps); the total residual
  then re-attributes to topology/engine per the numbers.
- Re-tighten `Tol::GCD_GUIDE` / `Tol::PICORV32_GUIDE` to the newly measured
  residual (floors, not passes — honest bounds only after the numbers are
  in). `PICORV32_GUIDE_ENGINE` should not need widening; if it does, name
  the cause.

## 6. What was run

Record in `docs/results.md`: exact steps rerun (script step numbers), the
container command, the release-binary runs, the census numbers, and every
golden's `git diff` status (non-guide goldens must be byte-identical).
