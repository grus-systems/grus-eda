# perf-parsers — measurement

## Protocol

Same machine (M4 mini), `GRUS_PHASE_TIMING=1`, REPS=5, idle machine —
the committed driver, run twice in the same session:

```sh
cargo build --release        # pre-change binary
REPS=5 sh kurena/tasks/perf-profile/measure.sh /tmp/perf-parsers-before.log

# … implement …
cargo build --release        # post-change binary
REPS=5 sh kurena/tasks/perf-profile/measure.sh /tmp/perf-parsers-after.log
```

`measure.sh` has no cell filter, so run the whole matrix both times —
the untouched cells (`place.*`, `sta.zerowire` internals,
`power.placement` internals) are the honest no-regression control, and
running the committed script as written is the reproducibility rule
(AGENTS §7.1). Binary-only swap, same inputs — `target/release/grus-eda`
before is rebuilt in step 0 before any source edit.

## Cells that carry the target phases

| measure.sh cell | phase(s) this ticket moves |
|---|---|
| `sta.spef.{gcd,picorv32,aes,ibex,jpeg}` | `sta.spef.read`, `sta.load.def` |
| `sta.spef_rcx.{gcd,picorv32}` | `sta.spef.read` (rcx NAME_MAP density) |
| `sta.zerowire.*`, `sta.placement.*` | `sta.load.def` |
| `congestion.*` | `cong.load` (LEF + DEF — LEF share is residual) |
| `power.vcd.gcd` | `power.activity.load` |
| `power.zerowire.*`, `power.placement.*` | `power.load.def` |

Phase medians are the tabled numbers (min–max in parens), matching the
baseline entry's format. DEF-parse improvement is reported through
`sta.load.def` directly; `cong.load`'s residual is named as "LEF
tech-db load" (untouched `lef.rs`).

## Ceiling

Plan ranks 5–7, `k` ≈ 2 each:

| phase | baseline | ×2 target |
|---|---|---|
| `sta.spef.read` jpeg | 297 ms | ~150 ms |
| `sta.spef.read` picorv32-rcx | 167 ms | ~85 ms |
| `cong.load` jpeg | 119 ms | ~85 ms (DEF share only — LEF stays) |
| `power.activity.load` gcd | ~479–860 ms | ~250–430 ms |

Cell-wall ceilings are small by Amdahl (sta.spef.jpeg −2.4%,
congestion 210 → ~150 ms, vcd cell 910 → ~530 ms per plan.md); report
per-phase and per-cell speedups vs the ×2 estimate, residual named —
per the acceptance table format.

## `docs/results.md` entry skeleton

```
### 2026-11-22 — perf-parsers: de-allocating SPEF/DEF/VCD scans (v0.3.6)

Mechanical scan/tokenize work in the three serial readers: per-line and
per-group `Vec`/`String` churn removed (reused scratch, `Cow` unescape,
`get_mut`-first map access, precomputed VCD bit names), NAME_MAP rekeyed
to u32, `fast-float` for SPEF numerics, `xxh3` on the per-line hash
maps (`owner`, `decls`, `activity`, connection name→id). No format-
semantics changes; `split_ascii_whitespace` note: <one line>.
Design: `kurena/tasks/perf-parsers/design/`.

**Phase before/after** — median (min–max) ms, REPS=5, M4 mini:
| phase | cell | before | after | speedup |
| `sta.spef.read` | jpeg / picorv32-rcx / … | … |
| `sta.load.def` | jpeg sta.placement | … |
| `cong.load` | jpeg | … | (LEF share residual) |
| `power.activity.load` | power.vcd.gcd | … |

**Cell wall**: `sta.spef.jpeg` … → …; `congestion.jpeg` 210 → …;
`power.vcd.gcd` … → ….

**Achieved vs. plan**: `k`=2 target per phase → achieved …; residual:
(1) `cong.load`'s LEF share, (2) read+UTF-8 validate, (3)
`db.strings.intern` SipHash (out of file scope), (4) per-connection
pin-name linear match, (5) public-map conversions / `pin_signals` ….

**Equivalence**: `cargo test --workspace` (n, m ignored), clippy clean,
<sta-regression leg if run>. Identical-output: `cmp` clean on
`--json`/stdout for `sta.spef.jpeg`, `sta.spef_rcx.picorv32`,
`congestion.jpeg`, `sta.placement.jpeg`, `power.vcd.gcd` before-vs-
after binary. Error paths: all line-numbered reject + truncation tests
green. Fail-proof: <3 mutations, test red, restored>.

**What was run**: `REPS=5 sh kurena/tasks/perf-profile/measure.sh
/tmp/perf-parsers-{before,after}.log` full matrix both ways, idle M4
mini, same session. Raw logs not committed; the script is.
```

## `docs/roadmap.md` N8 update

Strike `perf-spef-parse`/`perf-def-parse`/`perf-vcd-parse` (landed as
the batched `perf-parsers`) with the achieved per-phase numbers; the
remaining dominant phases are `sta.eval.walk`/`sta.slew.walk` (deferred
in plan.md rank 4) and `place.gp.*` — keep "candidates, not
commitments" phrasing.
