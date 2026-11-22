# Goal

Plan's last named block — batch the serial parser tickets
(`kurena/tasks/perf-profile/design/plan.md`: "batch as
one `perf-*-parse` ticket"):

| phase | measured (jpeg unless noted) | source |
|---|---|---|
| `sta.spef.read` | 297 ms (35 MB, ~118 MB/s); 167 ms picorv32-rcx | `readers/spef.rs` |
| `cong.load` / `sta.load.def` | 119–133 ms | `readers/def.rs` |
| `power.activity.load` (VCD) | ~479 ms, 96% of `power --vcd` gcd | `readers/vcd.rs` |

Tokenize/scan faster: byte-level scanning instead of
per-char/per-line work, `memchr`-style splitting where
cheap, avoid re-allocating strings for numeric fields
(`atoi_simd`-class approaches or manual parse), reuse
buffers. Keep it mechanical — no format-semantics changes.

## Equivalence

- **Bit-identical parse output**: every committed golden
  and reference-consuming test green; the parsed structures
  (`NetParasitics`, `DesignDb`, activity tables) must be
  identical — the strictest cheap proof: existing strict
  integration tests already compare downstream results
  per-field, so all of them staying green is the proof;
  record one intentional-break or unit round-trip check.
- Error paths unchanged (line-numbered rejects, §7.1/E0
  malformed-input behavior — those tests must stay green
  too).

## Measurement

Re-run the affected `measure.sh` cells (`sta` SPEF +
rcx, `congestion`, `power --vcd` gcd; plus `sta`
placement RC since `sta.load.def`/`sta.parasitics` feed
it), REPS=5, same machine. `docs/results.md` entry:
before/after per parser phase + cell wall, achieved vs
plan's ×2 estimate, residual named.

## Files

- `crates/grus-core/src/readers/{spef,def,vcd}.rs`
  (mechanical tokenizer/scan speedups only)
- `docs/results.md`, `docs/roadmap.md`,
  `kurena/tasks/perf-parsers/`
- `Cargo.toml` (patch bump)
