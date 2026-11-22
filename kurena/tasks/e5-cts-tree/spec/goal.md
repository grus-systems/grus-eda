# Goal

E5 ticket 3: the clock-tree constructor — consume the
committed TritonCTS references (`e5-cts-ref`) and the
`DesignDb` mutation API (`e5-insert-api`, v0.4.0) to
build a balanced buffer tree on a design's clock nets
and emit a CTS DEF.

## Work

- `grus-eda cts --tlef --lef --lib --def --sdc
  --def-out <d>.cts.ours.def [--report <json>]`:
  - Identify clock nets from the SDC clocks' sources /
    propagated-clock roots (reuse the STA clock model —
    do not re-parse SDC).
  - Decompose each clock net into buffer-driven
    segments: a depth-balanced tree (sinks bucketed by
    position, e.g. recursive bisection or PD-tree
    backbone — record the choice; TritonCTS reference:
    gcd 5 bufs depth 2 / picorv32 197 bufs depth 4, so
    depth and buffer class must be in that league, not
    one-buffer-per-sink).
  - Buffer selection from the scoped list (`clkbuf_16`
    root; `clkbuf_8/4/2` tree) by fanout/wireload —
    record the sizing rule.
  - Insert via `insert_buffer_on_net`; seed positions
    near the driven cluster; `legalize_placement` +
    `verify_legality`.
  - Inserted cells land `Unplaced` → seed → legalize;
    emit `write_def_full`.
- Report JSON: inserted buffer count per type, sink
  count, tree depth, clock-net HPWL — the criterion-4
  honesty-gate fields matching
  `<d>.cts.reference.json` schema where practical.
- Skew/insertion sanity inside `cargo test` using the
  existing propagated-clock STA: on gcd the constructed
  tree's clock-pin arrival spread should sit in the
  tens-of-ps band, insertion delay in the same order as
  the reference 0.31 ns (informational assertions —
  the strict `max(5 ps, 1%)` comparison vs the
  TritonCTS DEF is ticket 4 `e5-cts-e2e`).

## Done when

`grus-eda cts` produces a legal DEF on gcd (and
picorv32) with a plausible balanced tree; the report
JSON is committed-companion to `docs/results.md`;
propagated-clock STA runs on the output without new
errors. The e2e criterion comparison is **not** this
ticket.

## Files

- `crates/grus-core/src/cts/` (new module — tree
  constructor + report), `crates/grus-eda/src/main.rs`
  (`cts` subcommand, following `route`/`power` CLI
  conventions)
- `crates/grus-core/src/design_db.rs` only if a missing
  accessor surfaces
- `docs/results.md`, `docs/status.md`, `docs/roadmap.md`
  (per-ticket note), `docs/e5-cts.md` (new stage doc —
  algorithm + report schema)
- `Cargo.toml` (minor — new command/feature)
