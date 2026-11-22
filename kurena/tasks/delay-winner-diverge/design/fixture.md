# Fixture changes — `tests/designs/sta_exceptions/exceptions.def`

Two stages, each followed by a full golden regeneration
(`steps.md`). Stage A is the goal's literal change; the corrected
mechanics in `overview.md` predict it does *not* split the winners
(`u_ff0` wins both by `s + g`), and measuring that prediction is
deliberate — it yields the measured `g` and `s` used to size stage B
and produces an honest "we ran it" record for `docs/results.md`.

## Stage A — mux data-pin swap (goal's change)

Two-line edit in `NETS`:

```
- d0 ( u_ff0 Q ) ( u_mux A1 ) + USE SIGNAL ;   # was A0
- d2 ( u_ff2 Q ) ( u_mux A0 ) + USE SIGNAL ;   # was A1
```

`u_ff0` now drives the higher-capacitance mux input (A1 0.001884 pF vs
A0 0.001558 pF), so `u_ff0` owns the larger pure path delay
(`d_A1 ≈ d_A0 + 9.9 ps`). Nothing else in the DEF moves.

**Predicted outcome (verify, don't assume)**: `pm2_both.u_ff1/D.max_arr`
≈ `pm2_prop.u_ff1/D.max_arr` ≈ `base_prop.u_ff1/D.max_arr` — `u_ff0`
wins the merged delay domain *and* the ordinary max arrival, so the
pairing is still undiscriminated. If the regenerated golden instead
shows `base_prop`'s `u_ff1/D` arr *above* `pm2_both`'s (arrival winner
≠ delay winner), the split already materialised — skip stage B and say
so in `docs/results.md`.

## Stage B — delay `u_ff2`'s launch past the delay gap

Requirement: `launch(u_ff2) - launch(u_ff0) > g ≈ 9.9 ps`, with a margin
comfortably above the 5 ps absolute tolerance (target ≥ 15 ps of
*excess*, i.e. total added clock delay ≥ ~25 ps over the current
`launch_c2 = launch_c1 - 8.5 ps`).

Primary recipe — insert one buffer stage on `u_ff2`'s clock net:

```
COMPONENTS 7 ;
  ... existing 6 ...
- u_clkb3 sky130_fd_sc_hd__clkbuf_2 + PLACED ( 9200 2720 ) N ;
END COMPONENTS
```

```
NETS 10 ;
  ... clk, c1, din, sel, d0, d1, y unchanged ...
- c2 ( u_clkb2 X ) ( u_clkb3 A ) + USE SIGNAL ;   # u_ff2 CLK removed
- c3 ( u_clkb3 X ) ( u_ff2 CLK ) + USE SIGNAL ;   # new net
END NETS
```

- `u_clkb3` at (9200, 2720): row `ROW_1`, site-aligned (9200 = 460 x 20),
  inside the die, not colliding with `u_clkb2` (4600,2720) or `u_mux`
  (11500,2720). Placement is number-irrelevant (zero wire delay in this
  flow) but must stay legal.
- `clkbuf_2` matches the existing buffers and adds one cell delay
  (~20-60 ps typical for `sky130_fd_sc_hd`) plus a second buffer's input
  cap on `c2`, comfortably overshooting `g`. If the regenerated numbers
  show `launch_ff2 - launch_ff0 - g < ~10 ps` of margin (check per
  `steps.md`), enlarge further: bump `u_clkb3` to `clkbuf_1` (slower
  cell) or add a second cascaded stage — never by editing expected
  values.
- Rejected alternatives: leaf loads on `c2` (need ~3 extra pins,
  sub-linear per-load delay, more regen iterations); moving `u_ff2/CLK`
  onto `c1` while `u_ff0` takes `c2` (rewire alone gives only the same
  ~8.5 ps in the right direction, still < `g`, so it would need a third
  load anyway); adding load on `c1` (goal's suggestion — correct for a
  *reversed* pairing `u_ff2`-delay/`u_ff0`-arrival, wrong for the
  acceptance pairing).

## Consequences the golden absorbs (not bugs)

- `u_ff2/CLK` moves behind a second buffer: its propagated launch and
  capture arrivals shift by ~`T_buf` — every propagated-clock scenario
  (`base_prop`, `pmd_*`, `pm2_*`) and every `u_ff2/D` endpoint row gets
  new numbers, and `u_ff2`'s path delay changes from `d_A1` to `d_A0`.
  All 72 scenarios must be re-measured; never carry values over.
- `sel`'s port-launched candidate is unaffected by the clock change. In
  `base_prop`, `u_ff2`'s boosted arrival (~`0.719 + T_buf - g`) should
  stay above `sel`'s (~≤ 0.72); if the regenerated `base_prop`
  `u_ff1/D` max arr looks like the `sel` path instead (arrival below
  the expected `u_ff2` budget), that still discriminates the pairing
  but weakens the "arrival winner is `u_ff2`" claim — enlarge `T_buf`
  so `u_ff2` clearly wins, per the margin rule above.
- Ideal-clock scenarios (`base`, `m2_*`, all `md_*`/`mc_*`/`fp_*`/`mhu_*`)
  see only the data-net swap: `u_ff0`-sourced candidates get slower by
  `g`, `u_ff2`-sourced ones faster. `m2_both` should then report
  `m2_max0`'s `u_ff1/D` numbers (delay winner `u_ff0` under a shared
  ideal launch).
- `din -> u_ff0/D` / `din -> u_ff2/D` endpoints, the `md_early` /
  `md_datapath` error records, and all SDC files are untouched.
