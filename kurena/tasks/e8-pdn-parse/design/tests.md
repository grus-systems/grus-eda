# e8-pdn-parse — Test plan

Tests live beside their unit under test (`def.rs`, `lef.rs`,
`pdn.rs` `#[cfg(test)]` modules), reuse `test_support.rs` helpers and
committed `tests/designs/` fixtures. Integration-style tests that need
design files fail loudly when absent (project rule).

## Parse-level unit tests (`def.rs`, `lef.rs`)

- **`lef::parses_via_blocks`** — mini-LEF with a `VIA` (three `LAYER`
  groups incl. one `TYPE CUT`): assert `ViaDef{lower,upper,cut,
  cut_count}` fields and `via_by_name` lookup; include a multi-`RECT`
  cut group to check `cut_count`.
- **`lef::via_unknown_layer_fails`** — `LAYER` inside `VIA` naming
  neither routing nor cut layer → `Err`.
- **`def::parses_specialnets`** — existing `gcd.placed.def` parse:
  `pdn_nets.len()==2`, names, `use_kind`, expected per-net
  `wire_rects`/`vias` counts (47/46 predicted, assert exactly),
  `PdnConn` shapes (`TopPin` + two `AllInstances`).
- **`def::via_layers_from_vias_section`** — gcd `via2_3…` resolves to
  `lower=met1, upper=met2, cut=via, cut_count=5` (ROWCOL 1×5);
  `via5_6…` → `cut_count=1` (no ROWCOL).
- **`def::renamed_via_still_resolves`** — synthetic DEF: copy gcd's
  VIAS record, rename the via and its `+ LAYERS` cut layer to a
  nonsense name defined in a mini-LEF; SPECIALNETS uses the renamed
  via → resolves to the declared triple, proving no name-pattern
  heuristic. Include the LEF `VIA`-fixed path (`+ VIA <lefname>`) as a
  second record.
- **`def::unknown_via_fails`**, **`def::via_layer_mismatch_fails`**
  (record layer ∉ {lower,upper}), **`def::malformed_conn_fails`** —
  each asserts the bail message path.
- **LEF skip-report fixture update** — the fixture containing
  `VIA via_a END via_a` now parses (zero groups tolerated, nothing
  recorded): drop `VIA` from `skipped` expectations, adjust the entry
  count.
- **`gcell::special_wire_rects_view`** — parse gcd, assert the
  accessor's rect multiset equals the union of `pdn_nets` wire rects
  (regression guard for the gcell consumer).

## Connectivity & attach (`pdn.rs`)

- **`gcd_attach_and_connect`** — golden asserts from `golden.md`:
  per-net `attached_terminals == csv_rows == 384`,
  `unreachable_pins.is_empty()`, `all_shapes_connected`,
  `connectivity_result` agreement, `(instance, terminal)` set
  equality, X/Y ≤ 1 DBU.
- **`gcd_orient_fixture_attach`** — run attach on `gcd.orient.def`
  (W/E/FW/FE coverage) so orientation claims are only made for
  exercised arms; assert the same 384 attached per net.
- **`picorv32_census`** — the `require()`-gated test; per-net 615/792
  counts, `attached == 10205`, `unreachable == 0`, census printed for
  `docs/results.md`.
- **Fail-proof (acceptance-mandated red→green)**:
  `pdn::dropped_via_edges_flag_unreachable` clones a `PdnNet`, strips
  every `PdnVia` (or only `cut == via`'s links — the single cut-layer
  variant) → `unreachable_pins` non-empty for the corresponding net,
  `all_shapes_connected` false. Additionally the *real* test
  `gcd_attach_and_connect` must be observed red with vias dropped at
  the source once during development (acceptance asks for the
  observed-break-then-restore ritual, not only a synthetic unit test) —
  record that observation in `docs/results.md`.
- **`degenerate_via_point`** — via `(x,y)` outside all rects on one
  side → `via_end` singleton node, `nodes > 1`, unconnected flagged.

## Verification commands

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Plus the spec-selected heavy regression if the acceptance keeps it:

```bash
cargo test --release -p grus-core sta
```

Timing for `docs/results.md`: report the *cold* parse of
`picorv32.placed.def` after deleting any cache (there is no
DesignDb bincode cache today — confirm and say so) and the
`cargo test` wall time.

## Files

- `crates/grus-core/src/readers/def.rs` (tests)
- `crates/grus-core/src/readers/lef.rs` (tests + skip fixture update)
- `crates/grus-core/src/pdn.rs` (tests)
- `crates/grus-core/src/congestion/gcell.rs` (regression assert)
- `tests/designs/` (fixtures read; `gcd.orient.def` reused)
- `docs/results.md`
