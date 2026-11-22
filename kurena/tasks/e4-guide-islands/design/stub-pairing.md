# Reducing unpaired stubs at the source — `build_guides` ordering

## Root cause (verified by inspection, to be confirmed by the census in `probe.md`)

`updateVias` pairs only **consecutive** segments at the **same point** on
adjacent layers (`docs/e4-guides.md` §10). `build_guides` collects boxes in
a `BTreeSet` and `write_guides` re-sorts by `(layer, yl, xl, xh, yh)`, so
every `li1` stub is list-adjacent only to `li1` boxes and every `met1`
stub only to `met1` boxes — pairing is structurally impossible. OpenROAD's
own `gcd.guide` interleaves stubs with strips at the same cell
(`_000_`: `li1@A, met1@A, met2 strip, met1@B, li1@B`), which is why its
read-back yields 325 self-loops vs our 1,389.

Expected dominant classes before the fix (census will confirm):

- **C1 li1 stubs** (self-loop `0.0093`): unpairable under layer-major sort;
  needs a same-point `met1` neighbor in the segment list.
- **C2 met1 stubs** (`0.0045`): needs a same-point `met2` neighbor.
- **C3 stubs at cells interior to a merged strip**: our maximal-run merge
  puts the pin cell mid-strip, where no wire-seg endpoint exists — OpenROAD
  emits per-segment boxes that naturally terminate at pin cells.
- **C4 inherent residuals**: pin cells with no `L+1` coverage at all (pure
  met1 path for an `li1`/`met1` pin pair, `met2`/`met3` I/O access stubs,
  route-end pins). Same class OpenROAD's own 325 loops contain; name it
  with evidence, don't chase it.

## Ordering algorithm in `build_guides`

Per net, build an **ordered** `Vec<GuideBox>` (the `BTreeSet` sort order is
dropped for strips/stubs; dedup still applies):

1. Compute H (`met1`) and V (`met2`) runs as today, but **split each run at
   every pin GCell of the net lying on that line** so a strip box always
   ends or starts at a covered pin cell (bounded extra boxes: ≤ 1 split per
   pin per strip; the e4-1 ±15% box-delta gate has headroom — gcd +2.4%,
   picorv32 −6.2% at baseline).
2. Emit the ordered box stream; for each pin-cell group `P` (dedup across
   pins sharing a GCell):
   - `P` **interior to a strip** (now split into prefix/suffix):
     `[prefix, access@P, met1@P, suffix]` — both stubs pair
     (`access→met1` then `met1→met2` on the suffix's first wire seg).
   - `P` at a strip's **start** cell: `[access@P, met1@P]` directly **before**
     the strip box — same 0-residual outcome.
   - `P` at a strip's **end** cell: `[met1@P, access@P]` directly **after**
     the strip box — mirrors OpenROAD's own tail convention (`_000_` cell
     B): the met1 stub pairs, the access stub stays a self-loop. One
     residual, same class as the reference.
   - `P` covered by **no** `L+1` strip: append `[access@P, met1@P]` — the
     met1 stub stays a self-loop; inherent C4 class, documented.
   - `access == met1` collapses the pair through dedup; `access > met1`
     (I/O `met2`/`met3` stubs) emits `met1` first then `access` (the
     higher access stays a self-loop, as in OpenROAD).
3. `write_guides` stops re-sorting: write `NetGuide.boxes` in builder
   order, dedup preserving first occurrence. `parse_guides` already keeps
   file order; `test_guide_emit_deterministic` still passes because the
   build order is deterministic (BTree iteration + `net_pins` order).

## Pairing algebra (verified against `updateVias`)

- `[li1@P, met1@P, met2-wire@P→…]` → li1→met1 via, met1→met2 via (0 loops).
- `[met2-wire …→@P, met1@P, li1@P]` → met1→met2 via + li1 self-loop
  (OpenROAD's own `_000_` tail result — reference-equal residual).
- `[li1@P, met1@P]` alone (no met2 coverage) → met1 self-loop (C4).

## Constraints

- e4-1 gates (`test_*_guide_matches_openroad`) are order-insensitive (set
  intersection) but box-count-sensitive: splitting at pin cells raises the
  count — assert the delta stays within ±15% on both designs and record the
  new value.
- Guide `*D_NET` parity: splitting a strip does not change covered cells,
  so per-net caps are unchanged; the e4-2 gate must stay 269/269 and
  6744/6744.
- `resolve_guide_nets`, `escape_guide_name`, `guide_net_name` unchanged.
