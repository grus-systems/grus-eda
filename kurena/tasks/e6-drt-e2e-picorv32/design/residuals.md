# e6-drt-e2e-picorv32 — residual decomposition protocol (r1)

> **r2 status: landed** (`8496158`/`9788e53`, v0.12.9) — the
> `li.3`/`met1.2` residuals decomposed and pinned per this
> protocol. Kept as record. Its §4 suspect list was a
> pre-registration; the unrouted-census numbers it anticipated are
> superseded by the `recount.md` measured census (notably: the
> met3-bridge class appears — 138 nets — not "does not appear").

The leg's outcome is measured, not presumed. Whatever Magic finds
on `picorv32.drt.ours.routed.def` decomposes like the gcd leg did:
per-rule deltas → named sites → mechanism class → fix-or-pin.
This file is the protocol and the known suspect list; it does not
pre-decide the numbers.

## 1. What counts as a finding

From the golden's `magic` block (vs `placed_baseline` and
`routed_ref`):

- **`lef_expressible` `ours != 0`** — the gate's strict arm is
  `ours == placed`; `ours > 0` with `placed == 0` is an ours-only
  residual (the pin arm's shape). `ours == placed > 0` is
  inherited, not a finding.
- **`magic_only` row `inherited == false`** — a Magic-deck-only
  rule where ours diverges from placed (the gcd legs' nwell rows
  were all `ours == placed`; picorv32's routed reference carries
  `nwell.4` 8,084 / `nwell.2a` 2,756 / `nwell.1` 242 and
  cell-master paint is placement-invariant, so `ours == placed`
  there is itself evidence).
- **`unclassified_fired` non-allowlist names** — step E already
  `fail`s before writing the golden (`ALLOWED_UNCLASSIFIED`
  abort); a new unclassified name is a deck change or a new
  violation class, never a silent set growth.
- **`self_drc findings_total > 0`** in `cli_lines` — our own
  50-rule subset's verdict; pair each subset finding with its
  Magic counterpart (the `rule=`/`[bbox]` evidence lines are in
  `cli_lines` by the keep-list).

## 2. Site anatomy (the `who*.tcl` protocol)

The gcd leg's procedure, verbatim — probes live in the gitignored
`runs/drt-ours/` and are named in `docs/results.md`:

1. `drc listall why` bboxes in `ours_magic.log` are Magic internal
   units; ×5 = DEF DBU (`box` takes lambda = fragment/2).
2. `select area <bbox>` + `what` on the loaded layout names the
   tiles on both sides of the error; a flattened `.mag` dump
   (e.g. `picorv32_flat.mag`) resolves which subcell paint is
   ours vs cell-master vs foreign.
3. `drc why` on the selected region confirms the rule id on the
   exact site.
4. Join back to `cli_lines`: `unrouted`, `stub_outside_cover`,
   `area_residual`, `notch_residual`, `hole_residual`, `rule=…`
   evidence lines name the net/layer/region our own model already
   flagged.

## 3. Mechanism classes (gcd's X-taxonomy)

- **X1** — foreign/invisible geometry: the partner edge is
  cell-master or PDK paint our model legitimately cannot see.
  → document; not a router bug.
- **X2** — missing model feature: geometry we should have priced
  but the model lacks (gcd's pre-hull-mask classes).
  → follow-up ticket, named.
- **X3** — inside our model: our own paint vs our own paint, the
  model had everything needed (all 7 gcd `met1.2` sites — the
  `host_open` carve truncation).
  → fix if the fix is small and mechanism-named.

## 4. Known suspect list (pre-registered, not presumed)

Measured on earlier picorv32 runs; whether each still fires is
what the leg answers:

- **3 named `li.6` `area_residuals`** (`_02450_`, `_02905_`,
  `_04490_` — v0.12.6 emit): min-area islands with no in-region
  node to extend over. If still present they appear verbatim in
  `cli_lines` `area_residual` — already named, the cleanest pin
  candidate (`li.6`, count matching the line count).
- **116 `unrouted`** (v0.12.6): 43-net met3-bridge class
  (≤met2 guide union disconnected, met3+ rejoins — the §6
  edge-domain gap) + 65-net met2-connected class (stranded/OBS-
  sealed pockets, occupancy blocks) + 8 PDN/io-port-premark
  additions. Re-classify against the fresh `unrouted_kinds` /
  `unrouted_blockers` census — the e2e pin-access work moved the
  classes on gcd and will move them here.
- **592 `region_escape` pins** (355 inst + 237 io — the
  `drt-guide-escape` follow-up's input): expect a `projected=`
  count in the census; the escape mechanism's absence is a named
  class, not a silent `Unattachable`.
- **`transit_extend` hull-grid scan cost / guide-region escape
  review items** — scale-sensitive; record `route_ms` and the
  `hull_masked` census as the measurement they need.
- **`met3.6`/`li.6` min-area stubs** — the v0.12.6 patch lifted
  39/42; the current closure/min-area code may produce different
  stubs (or none).
- **`stub_outside_cover` census size** — expect an order of
  magnitude over gcd's 142.

## 5. Fix-or-pin, and the escalation arm

- **Pin** (designed arm): mechanism named, fix belongs to a
  follow-up. Entry in per-design `PINNED_LEF_RESIDUALS`
  (`gate.md` §2) + site anatomy + mechanism in `docs/results.md`.
  The pin asserts `ours == pin && placed == 0`; a rule where
  `placed` also fires cannot be pinned (it is `inherited`, and a
  pin would be *wrong*, not tolerated).
- **Fix in-ticket** (narrow): only when X3-mechanism-named and
  the fix is smaller than the decomposition write-up. Any
  emit-path change re-runs `emit-leg.md` §2–§3 in full — the
  golden must be generated on the post-fix DEF (no hand-edits,
  no golden patch). Given the emit leg is hours, prefer pinning
  over fixing inside this ticket; that preference is recorded.
- **Escalate** (red without a name): findings too numerous to
  decompose per-site, or no mechanism after the `who*` probes.
  The ticket's outcome is then the *measurement itself*: commit
  the golden, leave `test_e2e_ours_reference_picorv32`
  `#[ignore]`d with the real state in `docs/results.md`
  (red recorded, not rebaselined — spec's "red is a valid
  measurement"), and put the census + named-where-known classes
  on the card for the human to route (mass-pin vs fix ticket).
  Do not ship an un-pinned red test — `cargo test --workspace`
  is the verify gate.
- **`unrouted` connectivity cost** is never a gate failure —
  the golden's `ours_router.unrouted` is the named list and the
  joins assert internal consistency, not smallness.

## Files

- `tests/designs/picorv32/runs/` (gitignored — `who*.tcl`,
  `picorv32_flat.mag`, probe logs)
- `crates/grus-core/tests/drt.rs` (pin list only)
- `docs/results.md`
