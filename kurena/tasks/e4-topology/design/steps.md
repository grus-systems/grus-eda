# Implementation steps

Measure first, then land the two emission changes together (the `*P`
fix is only effective once bend vias connect the strip graph — the port
rung anchors to a strip node that may itself sit on a dead `met1`
island), then re-measure everything.

1. **Census tooling + baseline.** Add `tests/designs/guide_islands.py`
   (`probe.md` §1 definitions) and run it on `gcd.guide.ours.spef`,
   `gcd.guide.spef`, and the picorv32 equivalents → record the baseline
   tables (dead `*P` lists, dead-component sizes, self-loop histogram)
   as `docs/results.md` work-in-progress. Collect the OpenROAD
   bterm-coverage evidence (`probe.md` §3) — `gcd.guide` `resp_val` /
   `req_rdy` blocks + the 7 reference dead `*P`s classified.

2. **Bend via-stacks** (`congestion/guide.rs`, `bend-vias.md`): collect
   the non-pin H∩V covered cells per net, append `[met1@B, met2@B]`
   pairs in sorted order after the `met2` strip section / before
   unbound stub groups. `write_guides` untouched (builder order already
   preserved). Update `docs/e4-guides.md` §7.

3. **Port rung stacks** (`congestion/guide.rs`, `port-stubs.md`):
   replace `stub_chain`/`stub_extra` for bound cells with the
   contiguous stack rule + the before-wire-ascending /
   end-cell-`(low desc, high asc)` ordering. `StubCtx::group_before` /
   `group_after` signatures follow the new stack split. Update
   `docs/e4-guides.md` §6–7.

4. **Host regeneration + census**: `generate_guide_ours_spef.sh` on both
   designs (release) → `guide_islands.py` on the new SPEFs →
   before/after tables. Iterate on the emission rules only while a dead
   `*P` or multi-node island has an unnamed mechanism; name any residual
   class per net.

5. **Container re-measurement** (pinned image):
   - `check_guide_readback.sh` both designs → `read_guides` accepts;
     `guide_islands.py` on the read-back SPEFs (OpenROAD's own engine
     must report the same dead `*P` set on our boxes).
   - `generate_ref.sh` step 9c both designs → new
     `<d>.guide.sta.reference.json`; `git diff` all non-guide goldens
     empty.
   - Commit `gcd.guide.ours.spef` + `gcd.guide.sta.reference.json`
     (< 1 MB); picorv32 artifacts stay gitignored.

6. **Gates + floors**: `cargo test -p grus-core guide` — e4-1 share ≥
   0.70 / delta ≤ 15% (record new numbers), e4-2 269/269 + 6744/6744,
   decomposition table → update `Tol::GCD_GUIDE` /
   `Tol::PICORV32_GUIDE` to the measured residual (tighten only; widening
   needs a named cause) and rewrite their doc comments — they currently
   name `resp_val`/island-era causes that this ticket removes.
   `PICORV32_GUIDE_ENGINE` must not move silently.

7. **Docs + fail-proof + version**: `docs/results.md` entry (census
   before/after, bterm-coverage evidence, dominant island class named,
   decomposition table, verdict on E4 criterion-4), `docs/status.md` +
   `docs/roadmap.md` honest status, the two mutations from `tests.md`
   recorded, patch bump `[workspace.package] version`
   (0.3.12 → 0.3.13) in the same commit — regenerate
   `*.guide.ours.spef` **after** the bump so `*VERSION` is final.

8. **Verify**: `cargo test --workspace`, `cargo clippy --workspace
   --all-targets -- -D warnings`, `cargo test --release -p grus-core
   sta` (`sta-regression` verify, requires container + seeded picorv32
   artifacts).

## Files

- `crates/grus-core/src/congestion/guide.rs`
- `crates/grus-core/src/sta/parasitics.rs` (census helper / test only)
- `crates/grus-core/src/sta/analysis.rs`
- `tests/designs/guide_islands.py`
- `tests/designs/gcd/gcd.guide.ours.spef`
- `tests/designs/gcd/gcd.guide.sta.reference.json`
- `tests/designs/picorv32/` (gitignored artifacts)
- `docs/e4-guides.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
