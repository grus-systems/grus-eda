# e6-drt-via — Test plan

`crates/grus-core/tests/drt.rs` additions plus in-module unit tests.
Loud failures on missing inputs (`require`, `panic!`), determinism
double-runs, §7.7 fail-proofs — the file's established conventions.

## Integration tests (`tests/drt.rs`)

### Helpers

`load(name)` already returns `(tech, db, tracks, report)`; the via
tests build on it: `let table = via_table(&tech, &db).unwrap();` and
reuse `load_guide` + `build_route_graphs`. New helper:

```rust
/// `variants.baseline.vias_per_cut_layer` (+ `via_names_meaning`) of
/// the committed `<d>.drt.reference.json` — `serde_json::Value`
/// (already a dependency), `require`'d so a missing file is a loud
/// failure. Cut layer of a via name comes from `via_names_meaning`
/// (`"mcon"` etc.) or, for DEF vias, `def_viarule:`-prefixed names.
fn load_drt_ref(name: &str) -> serde_json::Value;
```

### `test_via_candidate_table`

Per design (gcd, picorv32):

- `table.gaps` asserted empty; all five pairs have `cut.is_some()`.
- Candidate counts per pair pinned `{5, 6, 6, 6, 6}` — five LEF defs
  per pair, plus the four DEF `VIAS` generated records on pairs 1–4
  (both placed DEFs declare the same `via2_3_*`/`via3_4_*`/
  `via4_5_*`/`via5_6_*`; li1–met1 gets none). `unmatched` empty,
  printed if not.
- Default names pinned `{L1M1_PR, M1M2_PR, M2M3_PR, M3M4_PR,
  M4M5_PR}` — drift in the ranking trips on the name, not just a
  count.
- **Reference join**: `vias_per_cut_layer` → per cut layer the
  majority name and full used-name set; `via_names_meaning` maps
  names → cut layer; `cut_layer_by_name → above` maps cut → pair.
  Asserts:
  - via / via2 / via3 pairs: our default **==** reference majority
    (`M1M2_PR` / `M2M3_PR` / `M3M4_PR`).
  - mcon: reference majority is `L1M1_PR_MR`, our default
    `L1M1_PR` — the named divergence is pinned *by kind*: assert
    both names, **and** assert the two `via_defs` carry identical
    `rects` (per-element equality) so a future deck change that
    differentiates them fails loudly.
  - via4: no `M4M5*` key in the census (asserted) — our `M4M5_PR`
    is a policy pick with no reference evidence; printed as such.
- Print the per-pair table (candidates, default, ref majority,
  ref used-set) for `results.md`.

### `test_via_edge_census`

Per design: `build_route_graphs` on the guide join, then:

- `total_via_edges > 0` on both designs (nonzero evidence).
- `via_edges_per_pair` printed; **met2–met3 asserted `== 0`** — the
  named edge-domain tech gap (`via-edges.md` §2). met4–met5 printed
  (≤ shared-crossing bound: 1 gcd / 36 picorv32 before coverage).
- Every `ViaEdge` endpoint sanity: `nodes[e.lower].layer + 1 ==
  nodes[e.upper].layer` and identical `(ix→x, iy→y)` coordinates —
  a per-element check, not a total (§7.5).
- Consistency: `gaps` empty AND per-pair edges present on pairs the
  reference transitions (li1–met1, met1–met2, met3–met4 nonzero on
  gcd — pin after first run; picorv32 same expectation).
- Determinism: second `build_route_graphs` → identical `via_edges`
  per net and identical `via_edges_per_pair` (folds into the
  existing determinism style; could merge into
  `test_graph_determinism`'s asserts).
- The full per-pair line (`candidates / via_edges / default /
  ref_majority`) is the census print the acceptance asks for.

### `test_via_edge_fixture` (synthetic LEF/DEF/guide — no real inputs)

Fixture: `met1` tracks `X/Y 0 DO 3 STEP 1000` ({0,1000,2000});
`met2` tracks `X/Y 0 DO 2 STEP 1500` ({0,1500}) → `(0,0)` is the
only shared crossing. LEF adds `LAYER via12 TYPE CUT` + a `VIA mv12`
(met1/via12/met2). DEF: two layers' `TRACKS`, a `VIAS 1` record
(`- gen + VIARULE r + CUTSIZE 100 100 + LAYERS met1 via12 met2 +
CUTSPACING 0 0 + ENCLOSURE 10 10 10 10` — exercises the
`def_vias` retention end-to-end through `parse_def_bytes`), one
cell + io pin joined by net `n1`; guide text covers
`[0,0,2000,2000]` on both layers.

1. **Shared crossing only**: exactly 1 via edge, between the
   `(met1,0,0)` node and the `(met2,0,0)` node — assert node
   coordinates at both endpoints are `(0,0)`.
2. **Off-crossing nodes have no via edge**: `(met1,1000,1000)` —
   a met1 crossing but off every met2 grid line — touches no
   `ViaEdge`; conversely `(met2,1500,1500)` (off every met1 line)
   touches none. This is the acceptance's fixture item.
3. **Coverage gate**: remove the met2 guide box → 0 via edges
   (crossing exists, no node); restore → 1 (fail-proof
   red→green).
4. **Gap pair emits no candidate**: drop `mv12` from the fixture
   LEF → `pairs[0]` is a `ViaGap` named `(met1, met2)` — table-side
   fail-proof.

## In-module unit tests

- `via.rs`: triple match incl. a non-adjacent def → `unmatched`;
  ranking picks the preferred-aligned variant on a two-via fixture
  (mirror `M1M2_PR` vs `M1M2_PR_R` rect shapes) and file-order on a
  tie; `Generated` record → candidate with `expand_via` rects
  (reuses the committed `via2_3` fixture shape).
- `graph.rs`: via-edge emission on a 2-layer hand-built region —
  shared coordinate covered both layers → edge; covered on one →
  none; endpoint ordering `lower < upper`.

## Workspace gates

`cargo test -p grus-core --test drt -- --nocapture` (census output
for `results.md`), `cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`.
`picorv32.placed.def` must exist — regenerate via
`tests/designs/picorv32/generate_ref.sh` if absent (container run,
loud failure otherwise).

## Files

- `crates/grus-core/tests/drt.rs`
- `crates/grus-core/src/drt/via.rs`
- `crates/grus-core/src/drt/graph.rs`
