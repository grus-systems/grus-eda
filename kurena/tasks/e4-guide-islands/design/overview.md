# e4-guide-islands — Overview

Fix the dominant e2e residual recorded in `docs/results.md` (2026-11-22,
`e4-sta-e2e`): **island semantics**. Two coupled changes, one ticket because
the e2e numbers only move when both sides land together.

## Problem

OpenROAD's `estimate_parasitics -global_routing` emits the segment graph
as-is: components unreachable from the driver stay **electrically dead** —
their nodes keep `*CAP` entries, unpaired zero-length stubs become self-loop
resistors (`_132_:7 _132_:7 0.0093`), island-internal wire/via links stay
real resistors (`_132_:8 _132_:9 0.0093`), and **no resistor bridges an
island into the driver's component** (measured on committed
`tests/designs/gcd/gcd.guide.spef`, net `_132_`).

Our `estimate_guide_parasitics` must produce a single `NetRcTree`, so it
currently links each disconnected component into the driver tree through a
`SPEF_STUB_RES_KOHM` (1e-6 kΩ) edge (`parasitics.rs` BFS tail). The edge is
transparent as a resistor, but it makes the whole island subtree's real wire
RC part of the Elmore path → parasitics-axis median **+201.7 ps (gcd)** /
**+1,408.0 ps (picorv32)**.

Separately, our guide **file** produces ~4.3x more unpaired stubs than
OpenROAD's own (gcd: ours 212 island nets / 1,389 self-loops vs reference
152 / 325, measured via `check_guide_readback.sh`), because
`write_guides`/`build_guides` sort boxes layer-major — every stub becomes
list-adjacent only to same-layer boxes and `updateVias` (consecutive-pair,
same-point, L+1 rule) can never pair it.

## Approach

1. **Model side** (`sta/parasitics.rs`): `NetRcTree` gains a dead-component
   representation — island nodes keep their caps (per-net `*D_NET` and the
   e4-2 cap gate stay identical) and internal tree edges, but carry a `dead`
   marker instead of a `SPEF_STUB_RES_KOHM` bridge. `write_spef` emits
   self-loop resistors for unpaired stubs and real resistors inside island
   components; `elmore_ns`/`pi_model` treat dead pins/caps the way OpenSTA
   treats them (no wire delay, no driver load contribution). `spef_net_tree`
   learns the same semantics so our own `sta --spef` consumer path accepts
   the emitted file (and, as a side effect, OpenROAD's own guide SPEFs).
   Details: `island-model.md`.

2. **Guide side** (`congestion/guide.rs`): emit boxes in a pairing-friendly
   order — strips are split at pin GCells and each pin's `[access, met1]`
   stub pair is interleaved next to the strip box whose wire endpoint
   touches the pin's cell, the way `updateVias` can pair. The layer-major
   `write_guides` sort is replaced by the builder's deterministic emission
   order. Details: `stub-pairing.md`.

3. **Re-measurement**: regenerate `<d>.guide.ours.spef`
   (`generate_guide_ours_spef.sh`), re-run `generate_ref.sh` step 9c in the
   pinned image for `<d>.guide.sta.reference.json` (the three golden blocks
   all move), re-run both `check_guide_readback.sh` for the new self-loop
   census, and record before/after in `docs/results.md`. Details:
   `probe.md`, `steps.md`, `tests.md`, `docs.md`.

## Non-goals

- No change to `updateVias`/`boxToGlobalRouting` semantics — they are the
  measured reference rule and stay exactly as documented in
  `docs/e4-guides.md` §10.
- No new layers (`met3+` banding stays deferred), no GCell-grid change.
- `EST-0026` warned-pin / nearest-node fallback stays as documented.

## Files

- `crates/grus-core/src/sta/parasitics.rs`
- `crates/grus-core/src/congestion/guide.rs`
- `crates/grus-core/src/sta/analysis.rs`
- `tests/designs/gcd/gcd.guide.ours.spef`
- `tests/designs/gcd/gcd.guide.sta.reference.json`
- `tests/designs/picorv32/`
- `docs/e4-guides.md`
- `docs/results.md`
- `docs/status.md`
- `Cargo.toml`
