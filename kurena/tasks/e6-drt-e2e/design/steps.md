# e6-drt-e2e — steps (r4, from checkpoint b372d5e)

Scripts landed, mask model landed, self-spacing closure + union-exact
cover + `RECT` emission landed, routed DEF regenerated and committed.
**The golden was never re-measured** — the committed
`gcd.drt.ours.reference.json` still reads the pre-fix 514 against a
superseded DEF sha. This revision is verification-completion in the
memo order: close the three evidenced residual classes, re-emit,
measure in-container, pin, document, gate.

Container: image `ghcr.io/librelane/librelane:3.0.16` is cached
locally; start `colima start --memory 12 --cpu 8` if the daemon is
down. Probe artifacts from the r3 root-cause work (`who*.tcl`,
`region.mag`, `gcd.whofix.def` in `tests/designs/gcd/runs/drt-ours/`)
are in the gitignored runs dir — keep for reference; name them in
`docs/results.md` so the evidence trail is reproducible.

## A. Bounded residual closure (`residuals.md`) — no container needed

All three fixes are evidenced in the committed DEF/emit log; each
lands with a unit fixture or an existing-test re-pin, and the emit
is re-measured by self-DRC (`grus-eda drt --self-drc`, seconds) —
not Magic — before touching the container.

1. **Fill dedup** (`residuals.md` §1): `fills` deduplicated before
   commit; `notch_fills` re-counts. Fixture: collinear three-piece
   notch → one fill.
2. **Patch same-net guard** (`residuals.md` §2): confirm `_007_` is
   the `area_patches=1` net (temporary instrumentation or
   edge-coordinate match); `patch_component` admission then requires
   merge-or-`≥ s` vs the net's own other components. If the site is
   *not* the patch, chase the secondary suspect (non-default via
   candidate scan) — the site must be attributed either way.
3. **Void-closure pass** (`residuals.md` §3): `enclosed_voids` over
   committed pieces + fills per layer, under-`min_enclosed_area`
   voids filled all-or-nothing under the existing admission scans;
   unfillable → `route_db.hole_residuals` + `  hole_residual` line.
4. **Census contract** (`scripts.md`): `guided=` gains
   `notch_residuals=<n>` (and `hole_residuals=<n>` if §3 ships);
   emit binary and check-script regex land in the same change
   (version-locked). Keep-list gains the self-DRC `rule=` + site
   lines so `cli_lines` carries residual evidence.
5. `_099_` re-check (`residuals.md` §4): if the residual line
   persists after 1–3, it is the pinned named residual — record the
   blocking class once (occupancy probe or `drc why`), no reroute.
6. `cargo test --workspace` between legs; existing tests that pin
   pre-fix geometry re-pin to measured values only (`gate.md` §4).

## B. Re-emit + quick gate

7. `generate_drt_ours.sh gcd` → committed
   `gcd.drt.ours.routed.def` regenerated (fills dedup'd, void fills
   present, patch guard applied). `--verify` arm covers
   byte-stability; the emit census goes into `docs/results.md`.
8. `grus-eda drt --self-drc` on the new DEF: `findings_total`
   target 0 or exactly the named residuals — anything else is an
   unattributed site → §5 probe loop in `residuals.md` before the
   container run (the fast local check is the iteration vehicle;
   the container is the authority, not the iterator).

## C. Container re-measure + golden commit

9. `check_drt_ours.sh gcd` inside the pinned image →
   `lef_expressible` per-rule ours/placed/routed_ref + `magic_only`
   decomposition + `unclassified_fired` + `sanity_control`. Every
   nonzero `ours` must map to a named site (`residuals.md` §5);
   unexplained → `drc why` + flattened dump, root-cause, fix-or-name.
10. Commit `gcd.drt.ours.reference.json` iff the outcome is
    zero-or-named-residuals **and** `input_def_sha_prefix` matches
    the committed DEF — the stale-514 file is superseded, not
    amended. Never rebaseline: a RED gate is chased, not committed.

## D. picorv32 leg — deferred (r4 memo)

11. `generate_drt_ours.sh picorv32` / `check_drt_ours.sh picorv32`
    stay un-run this revision per the memo; `docs/results.md` states
    the deferral explicitly (E0 rule: say what ran). The ignored
    gate test keeps the schema joins ready (`gate.md` §3);
    `picorv32.drt.ours.reference.json` is *not* committed — the
    acceptance's "both designs" item is honestly open, flagged on
    the card.

## E. Gate tests + docs + verify

12. `drt.rs` additions + re-pins per `gate.md`:
    `test_e2e_ours_reference_gcd`, `test_e2e_ours_def_deterministic_gcd`,
    `test_emit_roundtrip_gcd` unrouted pin, `test_emit_selfdrc_gcd`
    firing pin, `PINNED_LEF_RESIDUALS` if any.
13. `docs/results.md` full dated entry per `docs.md` (trajectory
    ~3025 → 847 → 514 → final; the three-site decomposition with
    mechanisms; unrouted 73→81 explained and all 81 named; the
    `notch_fills` dedup'd count; delta tables; picorv32 deferral;
    what-ran per E0 rule 9).
    `docs/e6-drt.md` e2e section + self-DRC scope correction;
    `docs/status.md` + `docs/roadmap.md` honest E6 line.
14. `[workspace.package] version` bump in root `Cargo.toml` in the
    same commit as the functional change (`0.12.6` → patch —
    repair/gate class).
15. `cargo test --workspace` + `cargo clippy --workspace -- -D
    warnings` clean before the card.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-eda/src/main.rs`
- `crates/grus-core/tests/drt.rs`
- `tests/designs/check_drt_ours.sh`
- `tests/designs/generate_drt_ours.sh`
- `tests/designs/gcd/gcd.drt.ours.routed.def`
- `tests/designs/gcd/gcd.drt.ours.reference.json`
- `tests/designs/picorv32/picorv32.drt.ours.reference.json` (deferred)
- `docs/results.md`, `docs/e6-drt.md`, `docs/status.md`, `docs/roadmap.md`
- `Cargo.toml`
