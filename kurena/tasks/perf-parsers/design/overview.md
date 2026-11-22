# perf-parsers — mechanical speedups for the SPEF/DEF/VCD readers

Speed up the three serial text readers named by
`kurena/tasks/perf-profile/design/plan.md` ranks 5–7 (batched as one
`perf-*-parse` ticket because the work is the same pattern three times):

| phase | measured (v0.3.2 baseline) | reader |
|---|---|---|
| `sta.spef.read` | 297 ms jpeg (36.9 MB, ~118 MB/s); 167 ms picorv32-rcx (8.4 MB) | `readers/spef.rs` `parse_spef` |
| `cong.load` | 119 ms jpeg — LEF tech-db + DEF in one phase; DEF share is the target | `readers/def.rs` `parse_def_bytes` |
| `sta.load.def` | 2–5% of `sta` wall on the big DEFs | same `parse_def_bytes` |
| `power.activity.load` | ~479–860 ms `power --vcd` gcd (94–96% of the cell) | `readers/vcd.rs` `parse_vcd` |

The dominant costs are mechanical, not algorithmic: per-line/per-record
heap allocations (`Vec<&str>` token buffers, per-connection `( comp pin )`
group vectors, `String` temporaries for lookups and name resolution),
SipHash-map lookups on short keys inside the per-line loop, and per-edge
`String` clones. Plan estimate `k` ≈ 2 per phase.

## What lands

All changes stay inside the three reader files plus docs/version:

- **SPEF** (`spef.md`): no per-line `Vec`, `u32`-keyed NAME_MAP,
  `Cow`-based unescape/`resolve`, ASCII whitespace split, `fast-float`
  for the numeric fields, xxh3 for the hot `owner`/`name_map` maps.
- **DEF** (`def.md`): scratch-buffer statements in record loops, inline
  `( comp pin )` group reader in NETS (no `Vec`), `Cow` unescape for
  connection/pin/net names, xxh3 name→id maps, iterator-shaped
  `split_attributes`.
- **VCD** (`vcd.md`): no per-change `id.to_string()`/`decl.name.clone()`/
  `format!("{}[{}]")`/`Vec<u8>` allocations, precomputed per-bit names,
  `get_mut`-first map access, xxh3 on the private hot maps.

Public type shapes are unchanged (`Spef`, `SpefNet`, `DesignDb`,
`VcdActivity`, `SignalActivity`): `saif.rs` constructs `VcdActivity` with
default-hasher `HashMap`s literally, so `signals`/`pin_signals`/`nets`
keep `std::collections::HashMap<_, _>` — internal fast maps are converted
at the boundary where one exists (see `vcd.md` §V5).

## Invariants

- **Identical parse output**: `NetParasitics`, `DesignDb`, and the
  activity tables must be the same structures for the same inputs. The
  strict integration tests already compare downstream results per field —
  all of them staying green is the proof; one before/after-binary output
  comparison and one intentional-break run are recorded
  ([`verification.md`](verification.md)).
- **Error paths unchanged**: every line-numbered `bail!` keeps its
  message and line accounting; the truncation tests in `spef.rs`/`def.rs`
  and the malformed-input tests keep passing. Semantic edges that a
  mechanical rewrite could shift are enumerated per file
  (§"error-path checklist" in each parser doc).
- **No format-semantics changes**: accepted/rejected input sets are
  identical on all inputs the test corpus exercises; two deliberate
  lexer-level notes are called out explicitly (`split_ascii_whitespace`
  in `spef.md` §S3, the `read_conn_group` error mapping in `def.md` §D1).
- **Phase keys stable**: `sta.spef.read`, `cong.load`, `sta.load.def`,
  `power.activity.load` keep measuring the same scopes — `cong.load`
  stays LEF+DEF in one scope, so the LEF share is a named residual
  ([`measurement.md`](measurement.md)).

## Out of scope

- `sta.parasitics` (`spef_parasitics` tree building, already `par_iter`),
  `saif.rs`, `lef.rs` (LEF parse inside `cong.load` is a named residual),
  `verilog.rs`/`yosys_json.rs`, `liberty.rs`.
- `StringInterner` (`db.strings.intern`, SipHash) — outside the allowed
  file set; residual.
- No `rayon`/mmap restructure: `memmap2` precedent exists in `lef.rs` but
  read+UTF-8-validate is ≤ ~15 ms on these inputs; list as residual.

## Files

- `crates/grus-core/src/readers/spef.rs`
- `crates/grus-core/src/readers/def.rs`
- `crates/grus-core/src/readers/vcd.rs`
- `crates/grus-core/Cargo.toml` (add `fast-float`)
- `Cargo.toml` (workspace `fast-float` dep + `[workspace.package]` patch bump)
- `Cargo.lock`
- `docs/results.md`
- `docs/roadmap.md`
- `kurena/tasks/perf-parsers/design/`
