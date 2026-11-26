# Measurement plan

All reference numbers come from the pinned image
`ghcr.io/librelane/librelane:3.0.16` (`colima start --memory 12 --cpu 8`,
tools invoked as `sta`/`openroad`). Ours-side artifacts come from the
release binary via `tests/designs/generate_guide_ours_spef.sh`.

## 1. Canonical census — `tests/designs/guide_islands.py`

New committed script (mirrors `guide_caps.py` conventions — the ad-hoc
union-find used for the `e4-guide-islands` census becomes reproducible).
Input: any SPEF with `*CONN`/`*RES` sections (our emitted
`<d>.guide.ours.spef`, the reference `<d>.guide.spef`, and the OpenROAD
read-back `runs/guide-check/<d>.ours.guide.spef`).

Definitions (write them in the script docstring; the numbers are only
meaningful under exactly these rules):

- Build a union-find over the node names appearing in each net's `*RES`;
  add an edge per **non-self-loop** resistor (`a != b`). Self-loop
  resistors (`n n r`) are counted separately by R value — a stub on
  layer `L` records the LEF `RESISTANCE` of the cut above it (`0.0093` =
  stub on `li1`/mcon, `0.0045` = `met1`/via1, `0.00341` = `met2` or
  `met3`/via2·via3 share the value, `0.00038` = `met4`/via4).
- `*I` and `*P` names from `*CONN` are nodes too; the **driver root** is
  the first `*I` with direction `O`, else the first `*P` with direction
  `I` (input port driving the net), else the first conn name that
  appears in a non-self-loop `*RES`.
- A **dead component** is a union-find component not containing the
  driver root. Report per design: nets with ≥1 dead component, total
  dead components, components split by size (`1` node vs `≥2` = the
  multi-node detour-island class), and the **dead `*P` list** (`*P`
  names inside a dead component, per net).

Usage: `guide_islands.py <spef>` prints the census table + dead `*P`
net list.

## 2. Baseline (already measured for this design; re-run for the record)

gcd `gcd.guide.ours.spef` vs `gcd.guide.spef` (ad-hoc union-find run
while planning this ticket; exact counts shift with the driver-seed
rule — the committed script's definitions are canonical):

| source | dead `*P` | nets w/ dead comps | multi-node comps |
|---|---|---|---|
| ours (v0.3.12) | 12 (`req_rdy`, `resp_msg[1-6,8,9,11,15]`, `resp_val`) | 173 | 126 |
| reference | 7 (`resp_msg[1,7,8,9,12,13,15]`) | 152 | 54 |

picorv32 baseline (recorded in `docs/results.md` `e4-guide-islands`):
ours 4,393 nets w/ dead comps, 1,754 nets w/ multi-node detour islands;
reference 4,013 / 604.

## 3. How OpenROAD covers bterm cells (recorded probe)

Evidence to collect for `docs/results.md` — no container run needed
beyond the existing committed guides:

- `gcd.guide` `resp_val` block (committed): the port cell
  (`0 41400 6900 55200`-adjacent, `+ LAYER met3` in the DEF) is covered
  by a `met2` strip plus stacked single-cell `met2`+`met3` boxes —
  consecutive same-point zls that pair `met2→met3`, so the port's conn
  node sits on the driver component; its SPEF shows `resp_val` landing
  on connected node `resp_val:2` with a `resp_val:2 resp_val:2` self-loop
  (residual stub on a live node — the pattern ours must mirror).
- `gcd.guide` `req_rdy` block: same pattern (`met1`+`met2`+`met3`
  coverage at the port-adjacent cells).
- For each reference dead `*P` (7 on gcd): classify why its conn-layer
  node is dead (unpaired access stub on an island — same class ours
  must reproduce as the residual, not eliminate below the reference
  count artificially).
- Read-back check (`check_guide_readback.sh` +
  `guide_islands.py` on `runs/guide-check/<d>.ours.guide.spef`):
  confirms OpenROAD's own read-back of OUR guide puts the same `*P`s on
  dead islands — i.e., the residual is emission coverage, not our
  reader.

## 4. Fix-side re-measurement (after the changes)

- `generate_guide_ours_spef.sh <d>` both designs → new guide +
  `<d>.guide.ours.spef`; `guide_islands.py` → dead `*P` (target: 0, or
  ≤ reference count with per-net evidence), dead-component census
  (target: multi-node islands ≈ 0 or all named), self-loop census
  (must not regress beyond the recorded bound — bend stacks add ~1
  `met1`-class self-loop per bend by design).
- `check_guide_readback.sh` both designs → `read_guides` still accepts;
  census the read-back SPEF the same way (OpenROAD's own engine on our
  boxes must agree with our model's census).
- e4-1 gates re-run (`test_*_guide_matches_openroad`): weighted share
  ≥ 0.70, box delta ≤ 15% — bend pairs and rung stacks add boxes;
  record the new delta (baseline `-7.0%` / `-13.0%`, headroom exists).
- e4-2 cap gates re-run: rung/bend boxes are zero-length → `*D_NET`
  byte-identical → `269/269` + `6744/6744` must hold.
- `generate_ref.sh` step 9c both designs → new
  `<d>.guide.sta.reference.json`; `git diff` all non-guide goldens must
  be empty.
- `test_<d>_guide_sta_decomposition --nocapture` → new four-row table;
  acceptance: gcd topology max < 250.8 ps, picorv32 < 626.66 ps, or the
  remaining extreme re-attributed to a named mechanism with per-net
  evidence.
- Re-measure `Tol::GCD_GUIDE` / `Tol::PICORV32_GUIDE` on the new
  extremes (tighten to the measured residual; widen only with a named
  cause).

## 5. What was run

Record in `docs/results.md`: exact script steps rerun, the container
command, release-binary runs, every census table (before/after, ours/
reference/read-back), per-net dead-`*P` evidence, and every golden's
`git diff` status.
