# perf-legal-passes — verification

The spec's equivalence bar is *not* bit-identity: legality + HPWL
not-worse + determinism + green goldens. Passes 1/2 are nonetheless
bit-identical by construction ([`algorithm.md`](algorithm.md) §3–4);
pass 3 may diverge through its dominating candidate set. E0-review
discipline: differential tests prove the cache values, and each test
must be shown able to fail.

## 1. New unit tests (no PDK — reuse `random_pass1_db` / `build_pass1_db`)

**`test_incremental_extrema_matches_full_rebuild`** — the core gate on
`update_net_extrema`:

- For ~300 seeds: build a random small design; maintain `net_ext`;
  apply scripted moves — single-pin x-slides (pass-2/3 shape,
  `move_y = false`), pair exchanges including orientation flips
  (pass-1 shape, `move_y = true`), multi-pin-per-cell nets, IO pins,
  ties at extrema (the existing fixtures already cluster coordinates
  on a small grid for this).
- After **every** move: rebuild every net's `NetExtrema` via
  `build_net_extrema` and assert all 8 slot *values* equal the
  maintained cache's, and `cached_hpwl` equality per net. Pin *tags*
  are deliberately not asserted (multi-way ties may hold a different
  pin with the same value — see algorithm §2).
- Assert `rebuilds > 0` over the sweep — the rescan branch must be
  exercised (mirrors the F5 `total_fallbacks > 0` discipline).

**`test_pass3_candidates_hit_site_argmin`** — the breakpoint
completeness proof:

- Random `net_spans` lists (including `o_lo > o_hi` constant nets),
  random `[left_limit, right_limit]` on a site grid.
- Brute-force `eval_x_span` over **every** site in the interval vs.
  the min over the candidate set (limits + `snap_down`/`snap_up` of
  each kink target). Assert equality of the minimum value over
  ~10k random instances. This is the test that fails if a needed
  candidate (e.g. `snap_up`) is dropped.

**Kept green**: `differential_sweep` /
`test_cached_pass1_eval_matches_full_scan_on_random_designs` /
`test_cached_pass1_eval_falls_back_when_both_top2_pins_move` /
`test_pass1_*` — they gate `evaluate_trial_swap_ro` and the extrema
semantics unchanged.

## 2. Integration gate (the spec's acceptance)

- `test_{gcd,aes,jpeg,ibex,picorv32}_place_and_legalize`: each asserts
  `verify_legality`, `|final_hpwl − placed_hpwl_dbu(reference)| <= 5%`,
  and the DEF write/reparse roundtrip — unchanged, must stay green.
  These are the "verify_legality on all committed designs + 5% golden"
  acceptance bullets.
- `cargo test --workspace`, `cargo clippy --workspace --all-targets
  -- -D warnings`.

## 3. HPWL before/after (recorded, gated)

Per design, `final_hpwl` (dbu) is read from the `place` stdout line in
the measure logs — before from the step-0 pre-change log, after from
the post-change matrix. Recorded in `docs/results.md` as a
before/after/Δ% table:

- Δ > +0.1% on any design → named + justified in the entry, or the
  pass-3 candidate change is revisited (see §6).
- Any |gap vs `*.reference.json` placed_hpwl_dbu| > 5% → hard fail
  (already asserted by §2 tests).
- Improvements and bit-identical designs are recorded with the same
  prominence.

## 4. Determinism

- Two release `grus-eda place` runs on jpeg and picorv32 → `cmp` the
  two `--out-def` files. Record clean.
- `RAYON_NUM_THREADS=1` vs default `place` on jpeg → `cmp` out-def.
  (The only `par_iter` is the one-time extrema build — an ordered
  collect — so this is expected clean; run it anyway and record.)

## 5. Fail-proof mutations (each recorded, each restored)

1. **Insert rule**: change `v < lo[1].val` to `v <= lo[1].val` (or
   drop the `lo[1]` update) → §1 extrema differential test red.
2. **Membership**: skip the `hi` slot check → extrema differential
   test red on a move of a max-extremal pin.
3. **Candidate completeness**: drop `snap_up` from the kink candidates
   → §1 argmin test red.
4. **Owner exclusion**: pass `0` (or `u32::MAX`, which wrongly
   excludes IO pins) instead of `c` in pass 3's
   `surviving_extremum_owner` → wrong `o_lo`/`o_hi` → E2 tests red
   (HPWL gap or legality). Record which.

## 6. Honesty notes

- Bit-identity is *expected* for passes 1/2 and *not required* overall.
  If a `cmp` of before/after `out-def`s shows a diff attributable to
  pass 1 or 2, that is a bug signal worth investigating even though
  the bar allows it — record what was found.
- If pass 3's candidate change regresses a design >0.1%: first check
  whether the regression is real (re-measure) then either (a) justify
  it in `docs/results.md` per the spec, or (b) add a cheap
  median-proxy candidate (e.g. snap of the per-net `(o_lo+o_hi)/2`
  midpoint set, O(nets)) to recover the heuristic — decision recorded.
- The `refine_extrema_rebuilds` counter is printed into the
  `results.md` entry (temporary `eprintln` during measurement is
  acceptable but the committed field is preferred — F5 precedent).
- Claim coverage only for what ran: if any of the five designs is not
  re-run, say which.
