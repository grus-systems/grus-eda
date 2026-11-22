# e4-guide-parasitics — Tests

All in `sta/parasitics.rs`'s `#[cfg(test)] mod tests` plus the guide
module where noted (existing convention: `test_support::{design_dir,
require, sky130_hd}`, `require()` panics on missing inputs — no silent
skips, E0 discipline). `picorv32.placed.def` is gitignored/regenerated;
the kurena worktree seed covers it.

## Unit tests

- `resolve_guide_nets`: name→NetId mapping on a small design — raw DEF
  spellings (`net\[0\]`) and bare names both resolve; an unknown net
  name errors (strictness, not a skip).
- `estimate_guide_parasitics` on a synthetic fixture (reuse the
  `mini_tech`/`mini_design` shape from `guide.rs`'s tests — lift them to
  `pub(crate)` or build an equivalent fixture in parasitics tests):
  - a 2-pin net with a straight 3-GCell strip produces the expected
    link count, per-edge R, and `total_wire_cap_pf` within float noise
    of the hand-computed value under the resolved model settings;
  - a corner (strip on met1 + met2 over the corner GCell) attaches the
    two layers per the probed via rule;
  - a pin pair in one GCell (stubs only, no route) yields a tree, not a
    panic;
  - net with `< 2` pins or no boxes → `None`.
- SPEF round-trip: `write_spef` on the guide parasitics → `parse_spef`
  → `spef_parasitics` → per-net `total_wire_cap_pf` equal (within f32
  round-trip noise) and identical pin coverage. This is the "`sta
  --spef` can consume it" assertion — the same discipline as
  `test_pd_vs_spef_parasitics_agree`.

## Integration (committed references)

- `test_gcd_guide_parasitics_match_openroad`: `parse_guides` on
  `gcd.guide` → `resolve_guide_nets` → `estimate_guide_parasitics`;
  per-net compare vs `gcd.guide.caps.txt` under the metric of
  `metrics.md` (a). Prints the full stats table under `--nocapture`.
- `test_picorv32_guide_parasitics_match_openroad`: same on picorv32
  (`require()` on `picorv32.placed.def`/`picorv32.guide`/
  `picorv32.guide.caps.txt`).
- `test_guide_parasitics_deterministic`: build twice on gcd, assert
  identical `total_wire_cap_pf` per net (ordering/nondeterminism guard).
- The CLI arm is thin plumbing over `compute_parasitics`; cover it with
  one recorded hand-run on gcd (full
  `--lib --tlef --lef --def --sdc` flags):
  `sta --parasitics guide` (internal route), and
  `sta --parasitics guide --guide gcd.guide --spef-out g.spef` then
  `sta --spef g.spef` — recorded in `docs/results.md`, not a cargo test
  that shells the binary.

## Fail-proof (AGENTS.md §7.7)

After the integration tests pass, break the model on purpose, confirm
red, restore, and record the mutation + result in `docs/results.md`:

- drop the via links (or merge all layers into one) → gcd test must go
  red on the corner nets; **or**
- charge every pin stub a full GCell pitch of `li1` cap → per-net caps
  shift outside `max(5%, 1 fF)` on the stub-heavy small nets → red.

If a mutation stays green, that is a finding (the bound is too loose for
that aspect) — record it rather than hiding it behind a weaker check.

## Out-of-band verification (recorded, not cargo tests)

- `generate_ref.sh` step 5d in the pinned image on both designs; `git
  diff` on the caps goldens after a rerun must be empty.
- Direction (b): `route --guide-out` + both `check_guide_readback.sh`
  scripts + `guide_caps.py diff`; numbers recorded per `metrics.md`.
- `cargo test --workspace`, `cargo clippy --workspace --all-targets --
  -D warnings`, and `cargo test --release -p grus-core sta` — existing
  goldens byte-identical; nothing in this ticket may perturb STA
  output.
