# TimingGraph: flat CSR instead of Vec<Vec> / BTreeMap

## Goal

`README.md` Goal 3 lists packing the remaining `Vec<Vec<u32>>` / `BTreeMap` in
the timing graph into flat CSR arrays. `TimingGraph.edges_from`,
`LevelizedGraph.edges_to`, `LevelizedGraph.levels`, and
`checks: BTreeMap<u32, RegisterCheck>` become CSR / sorted-vec structures.
**Measured impact is small** (`build_graph` + `topological_levels` = ~15 ms of
a ~4.5 s jpeg STA); this ticket is structural cleanup and runs last, only if
tickets 01–03 leave appetite.

## Files
- crates/grus-core/src/sta/analysis.rs

## Interface

Internal types only (`TimingGraph`, `LevelizedGraph` are private). `run_sta`,
`run_sta_with_state`, `StaReport`, `EndpointReport`, `SkipReport` signatures
and output unchanged — endpoint order must stay sorted by pin name as today.

## Acceptance

- `cargo test --release -p grus-core sta::` green with identical reports.
- `cargo clippy --all-targets -- -D warnings` clean.
- No `Vec<Vec<...>>` or `BTreeMap` left inside `TimingGraph` /
  `LevelizedGraph` — that includes `levels` (`level_offsets: Vec<u32>` +
  `level_pins: Vec<PinId>`) and any per-pin edge lists. Iteration order must
  be deterministic (sort by pin id where order is observable).

## Notes

- `edges_from` is built by pushing edge indices in cell/arc scan order; when
  flattening to CSR, fill counts first then scatter — the scatter order is
  already deterministic (scan order), so no sort is needed, but keep it that
  way and say so in a comment only if non-obvious.
- Some unit tests in `analysis.rs` index `graph.edges_from` / `edges_to`
  directly; update those accesses to the CSR accessors in the same change.
- `checks` iteration feeds `collect_endpoints`; today `BTreeMap` gives
  ascending `data_id` order before the final `entries.sort_by(pin name)` —
  keep any deterministic order (e.g. sorted `Vec<(u32, RegisterCheck)>`); the
  final sort makes it moot, but do not rely on that silently.
- `edges_to` values are `(PinId, lib_arc)` pairs — pack as two parallel
  `Vec<u32>` CSR slices or a single `Vec<(u32,u32)>` with offsets.
