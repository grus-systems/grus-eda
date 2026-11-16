# Ticket 01 — picorv32 E1/E2 OpenROAD goldens

## Goal

`docs/status.md` lists "Add `picorv32` E1/E2 OpenROAD goldens" under Phase 3.
Today picorv32 is an E3-only fixture: it has no `*.reference.json` (so
`test_picorv32_place_and_legalize` cannot check HPWL against OpenROAD) and no
`*.util65.def` / `*.gcell_usage.txt` (so there is no E1 congestion-map
comparison). Bring it to parity with aes/jpeg/ibex.

## Files

- `tests/designs/picorv32/generate_ref.sh` — add steps 6, 7, 7a–7f, 8
  (mirroring `tests/designs/aes/generate_ref.sh`; picorv32's existing steps
  1–5b and 9/9b stay byte-identical in behaviour).
- `tests/designs/picorv32/picorv32.reference.json` — new committed golden.
- `tests/designs/picorv32/picorv32.util65.def`, `picorv32.gcell_usage.txt`,
  `picorv32.orient.def`, `picorv32.orient.pins.txt` — generated artifacts.
- `.gitignore` — ignore the >1 MB artifacts, same style as the aes block.
- `.github/workflows/ci.yml` — add the new artifacts to the
  `reference-inputs` upload list.
- `crates/grus-core/src/congestion/route.rs` — add
  `test_picorv32_congestion_map` (clone of `test_ibex_congestion_map`).
- `crates/grus-core/src/placement/legalize.rs` — extend
  `test_picorv32_place_and_legalize` with the `placed_hpwl_dbu` gap assert.
- `tests/designs/README.md`, `docs/status.md`, `docs/results.md` — records.

## Script steps (adapted from aes; differences noted)

- **Step 6** verilog counts on `picorv32.synth.v`, `link_design picorv32`
  (top module name; aes uses `aes_cipher_top`, gcd `gcd`).
- **Step 7** orient fixture on `picorv32.placed.def` (all 8 orientations) +
  `getAvgXY` dump. Needed only for `orient_hpwl_dbu` in reference.json.
- **Step 7a** `place_variant.tcl` with `UTIL=65 DENS=0.73` (the same pair aes
  uses; `initialize_floorplan -utilization 65 -aspect_ratio 1.0
  -core_space 5.52 -site unithd`, `global_placement -density 0.73`,
  `detailed_placement`) writing `picorv32.util65.def`.
- **Step 7b** GCell grid counts on `picorv32.util65.def`: raw/orig tracks,
  then `set_global_routing_layer_adjustment met1-met5 0.3` and derated tracks.
  **Decision**: uniform `0.3`, the aes value — picorv32 has no ORFS config to
  inherit from and the adjustment is a dump convention, not a model input;
  the Rust test uses the same `adj` so any fixed value is self-consistent.
- **Step 7c** routed-demand query (`grt_total_edge_demand`), same Tcl as aes.
- **Step 7d** layer-adjustment sweep `alphas {0.50 0.70 0.80 0.85 0.90}` on
  `picorv32.util65.def` with `-allow_congestion`.
- **Step 7e** util sweep: `utils {65}` only (aes/ibex/jpeg convention; gcd's
  4-point sweep exists because gcd feeds `test_gcd_util_sweep_ranking`, which
  picorv32 does not).
- **Step 7f** per-edge usage/capacity dump to `picorv32.gcell_usage.txt`
  (identical Tcl to aes step 7f, paths renamed).
- **Step 8** `picorv32.reference.json` via live OpenROAD/OpenSTA queries —
  same field list as `aes.reference.json`, `"design": "picorv32"`. Reads
  `picorv32.placed.def` + `picorv32.sdc`; `placed_spef_*` reads
  `picorv32.routed.spef` (the placement-estimate SPEF, as on the other
  designs).

Ordering constraint (same as aes): step 7a must run before 7b because 7b reads
`picorv32.util65.def`; steps 6–8 slot in after step 5b's DRC gate and before
the existing step 9 STA dump, so a dirty route still aborts early.

## Rust tests

- `test_picorv32_congestion_map` in `congestion/route.rs`: copy
  `test_ibex_congestion_map`, paths `picorv32.util65.def` /
  `picorv32.gcell_usage.txt`, `adj = [0.0, 0.3, 0.3, 0.3, 0.3, 0.3]`. Same
  assertions (`rho >= 0.60`, `rho >= null + 0.15`, `overlap >= 0.50` per
  direction). If it fails, record the measured numbers in `docs/results.md`
  and `#[ignore]` it with the numbers in the reason — do **not** loosen
  thresholds (F2 precedent: jpeg/ibex).
- `test_picorv32_place_and_legalize`: add the `placed_hpwl_dbu` read and the
  `|gap| <= 0.05` assert used by aes/jpeg/ibex. picorv32 GP converges in ~227
  iterations (< 300) per `docs/status.md`.

## Acceptance

- `generate_ref.sh` rerun as written reproduces every artifact; the pre-existing
  picorv32 files (`placed.def`, `routed.spef`, `rcx.spef`,
  `sta.reference.json`, `drc.rpt`) are byte-identical afterwards (`git status`
  clean for them; sha256 recorded in `tests/designs/README.md`).
- `cargo test --release -p grus-core` green; `cargo clippy --all-targets --
  -D warnings` clean.
- `docs/results.md` entry with date, design, reference values, our values,
  differences, runtimes — including any `#[ignore]`d outcome.
- `.gitignore` covers every generated artifact over 1 MB; committed files are
  under 1 MB.
