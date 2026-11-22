# perf-legal-passes — implementation steps

Order matters: the shared-cache mechanism and its differential test
land before any pass is rewired, so each later step is gated by an
already-proven helper. Each step ends with `cargo test --workspace`
green; PDK tests additionally need `cargo test --release -p grus-core
placement::`.

## 0. Before-log (no code change)

```sh
cargo build --release
# measure.sh has no cell filter — run its five place commands by hand:
for d in gcd picorv32 aes ibex jpeg; do
  for i in 1 2 3 4 5; do
    GRUS_PHASE_TIMING=1 /usr/bin/time -l target/release/grus-eda place \
      --tlef "$PDK/techlef/sky130_fd_sc_hd__nom.tlef" \
      --lef  "$PDK/lef/sky130_fd_sc_hd.lef" \
      --def  "tests/designs/$d/$d.floorplan.def" \
      --out-def "/tmp/$d.out.def" >> /tmp/perf-legal-before.log 2>&1
  done
  cp "/tmp/$d.out.def" "/tmp/$d.before.def"
done
```

($PDK = `$PDK_ROOT:-$HOME/.ciel/sky130A/libs.ref/sky130_fd_sc_hd` as in
`measure.sh`.) The `.before.def` files are the cmp reference for
steps 2–5 and the HPWL-before source for the results table.

## 1. Helpers + cache plumbing (no behavior change)

- Add `update_net_extrema`, `surviving_extremum_owner`,
  `net_other_x_extrema`, and a `cell_pins_on_net` group-slicer
  (algorithm §2).
- Build `net_ext` once inside `place.legal.pre`; thread `&mut
  net_ext` into `equal_width_swap_pass_fast` (signature change; test
  wrapper builds its own and forwards); delete the per-round build.
- Pass 1 accept path: replace `build_net_extrema_fast` per incident
  net with `update_net_extrema(.., move_y = true)` over
  re-enumerated moved pins (algorithm §3.3).
- `LegalizeResult.refine_extrema_rebuilds` field + accumulation.
- New `test_incremental_extrema_matches_full_rebuild`
  (verification §1).
- **Gate**: full suite green; `place` on gcd + picorv32 → `cmp` vs
  `/tmp/{gcd,picorv32}.before.def` — bit-identical expected; if not,
  find the divergence before proceeding (verification §6).

## 2. Pass 2 evaluation rewrite

- Swap the per-incident-net `net_pins` scan for the extrema-based
  eval (algorithm §4): before from `ext` axis-0, after via
  `surviving_extremum_owner` + moved `pin_x + dx`, fallback
  `net_other_x_extrema`; on accept, `update_net_extrema(.., false)`
  for the pair's incident nets.
- **Gate**: suite green; `place` out-defs still `cmp`-clean vs
  `.before.def` on all five designs (pass 3 untouched → bit-identical
  still expected).

## 3. Pass 3 evaluation + candidate rewrite

- Span construction via extrema + own-pin groups (algorithm §5);
  candidate set = limits + `snap_down`/`snap_up` of each net's two
  kink targets; `other_xs_buf` deleted; accept →
  `update_net_extrema(.., false)` + `refresh_cell_pin_x`.
- New `test_pass3_candidates_hit_site_argmin` (verification §1).
- **Gate**: suite green; HPWL per design vs `.before.def`
  `final_hpwl` — table the deltas; any >+0.1% regression →
  verification §6 path.

## 4. Pass 1 width-keyed band index

- `BandEntry` → `(width, x, mi, cell)`; partition-point range on
  `(w1, x±25_000)`; `band_insert`/`band_remove` on the rekeyed tuple
  (algorithm §3.2).
- **Gate**: suite green; out-defs `cmp`-clean vs the step-3 output on
  all five designs (identical candidate set → bit-identical expected).

## 5. Fail-proof runs

The four mutations of verification §5, each restored green, each
recorded for `docs/results.md`.

## 6. Measure + document

- `cargo build --release`; `REPS=5 sh
  kurena/tasks/perf-profile/measure.sh /tmp/perf-legal-passes.log`
  on an idle machine (full matrix).
- `docs/results.md` entry per [`measurement.md`](measurement.md)
  skeleton — wall table, phase table, per-design HPWL Δ table,
  determinism record, fail-proof record, rebuild count.
- `docs/roadmap.md` N8 + `docs/e2-placement.md` §4 sync
  (measurement.md).
- `[workspace.package] version` 0.3.4 → 0.3.5 in `Cargo.toml`
  (+ `Cargo.lock` refresh via `cargo check`), same commit as the
  functional change.
- Final: `cargo test --workspace`, `cargo clippy --workspace
  --all-targets -- -D warnings` clean.

## Suggested commit split

The pipeline commits per stage; if split, the natural units are
steps 1 (helpers+plumbing), 2+4 (passes 1/2 — bit-identical), and
3+6 (pass 3 + measurement/docs/version).
