# e4-guides-emit — Tests

All in `guide.rs`'s `#[cfg(test)] mod tests` (existing convention:
`test_support::{design_dir, require, sky130_hd}`, `require()` panics on
missing inputs — no silent skips, E0 discipline).

## Unit tests

- `parse_guides` on a small hand-written fixture: multiple nets, several
  boxes per net, parenthesised block form; asserts names, rect coords and
  layer ids round-trip. Negative case: an unknown layer name and a
  malformed box line each error.
- `write_guides` → `parse_guides` round-trip on a synthetic `NetGuide`
  list: byte-level format spot-checks (`name` / `(` / `xlo ylo xhi yhi
  layer` / `)`) plus parsed equality.
- Strip merging on a small hand-built `GCellGrid` + segments: contiguous
  H edges merge to one box; an L turn produces met1 + met2 coverage on the
  corner cell; re-traversed edges dedup.
- Stub rules: instance pin → `li1` + `met1` stub at its GCell; a pin-pair
  in one GCell dedups; I/O pin → `pin_io_layer` + `met1`.

## Integration (committed references)

- `test_gcd_guide_matches_openroad`: `route_design` on `gcd.placed.def` →
  `build_guides`; `parse_guides` on committed `gcd.guide`; metric gates of
  `metrics.md` (box-weighted overlap ≥ 0.70; total within 15%; per-net
  stats printed).
- `test_picorv32_guide_matches_openroad`: same on `picorv32.placed.def` +
  `picorv32.guide`. Both inputs are gitignored/regenerated —
  `require()` panics with the "run `generate_ref.sh`" message if absent
  (consistent with existing picorv32 tests; the kurena worktree seed
  covers it).
- `test_guide_emit_deterministic`: full pipeline (`route_design` +
  `write_guides` to a `String`) twice on `gcd.placed.def`; assert
  byte-identical output. This catches nondeterministic iteration or
  sort-order drift.

## Fail-proof

After the integration tests pass, prove they can fail (AGENTS.md §7.7),
and record the mutation + restore in `docs/results.md`:

- drop pin-access stubs from `build_guides` (or force all boxes onto
  `met1`) → gcd overlap share and/or total box count must go red →
  restore, green again.
- If a bound happens to stay green under the mutation, that is itself a
  finding: the bound is too loose for that aspect — record it rather than
  inventing a weaker mutation to hide it.

## Out-of-band verification (recorded, not cargo tests)

- `tests/designs/gcd/check_guide_readback.sh` in the pinned container:
  `read_guides` on our emitted file must exit clean; then
  `estimate_parasitics -global_routing` exercised on the read-back
  guides. Results recorded in `docs/results.md` including which commands
  ran.
- `cargo test --workspace`, `cargo clippy --workspace --all-targets --
  -D warnings`, and the release `sta` regression (`cargo test --release
  -p grus-core sta`) — goldens otherwise byte-identical; nothing in this
  ticket may perturb STA output.
