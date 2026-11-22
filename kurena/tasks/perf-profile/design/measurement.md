# perf-profile — measurement protocol and results entry

What to run, how many times, and the shape of the `docs/results.md` entry.

## Machine context (record verbatim in the entry)

- Host: Mac mini, Apple M4, 10 cores (4P + 6E), 16 GB RAM
- OS: macOS, Darwin 27.2.0 (record `sw_vers` output)
- Toolchain: `rustc 1.100.0` (record `rustc --version` + `cargo build --release`)
- Binary: `target/release/grus-eda` at the ticket commit
- Rep policy: ≥5 wall-clock reps per (design, command, mode); report
  **min / median / max**. Rep 1 of each binary is a cold-filesystem read;
  report it separately if it diverges, do not silently drop it.
- Cache policy: the Liberty binary cache warms on first use; all table rows
  are **warm-cache** runs. If a cold-parse figure is reported, the cache file
  must be deleted first and the row labelled cold (`AGENTS.md` §7 rule 10).

## PDK inputs

`$HOME/.ciel/sky130A/libs.ref/sky130_fd_sc_hd/` (same convention as
`test_support.rs`): `lib/sky130_fd_sc_hd__tt_025C_1v80.lib`,
`techlef/sky130_fd_sc_hd__nom.tlef`, `lef/sky130_fd_sc_hd.lef`.

## Run matrix

Designs: `gcd`, `picorv32`, `aes`, `ibex`, `jpeg` — all generated artifacts
(`*.floorplan.def`, `*.placed.def`, `*.routed.spef`, `picorv32.rcx.spef`) are
already seeded in the worktree; missing pieces regenerate via each design's
`generate_ref.sh` (do not regenerate just for this ticket — record absence).

| command | modes | designs |
|---|---|---|
| `sta` | zero-wire; `--parasitics placement`; `--spef <d>.routed.spef` (+ `gcd.rcx.spef`/`picorv32.rcx.spef` where present) | all 5 |
| `power` | no-annotation defaults, `--parasitics placement` | all 5 |
| `power` | `--vcd tests/designs/gcd/gcd.vcd` if the file exists | gcd only |
| `congestion` | `--def <d>.placed.def` | all 5 |
| `place` | `--def <d>.floorplan.def --out-def /tmp/<d>.out.def` | all 5 |

Exact command line per cell (substitute `$LIB`/`$TLEF`/`$LEF`, `$D`, `$DEF`,
`$SDC`, `$SPEF`):

```sh
GRUS_PHASE_TIMING=1 /usr/bin/time -l target/release/grus-eda sta \
  --lib $LIB --tlef $TLEF --lef $LEF --def $DEF --sdc $SDC \
  [--parasitics placement | --spef $SPEF] --json /tmp/<d>.sta.json
GRUS_PHASE_TIMING=1 /usr/bin/time -l target/release/grus-eda power \
  --lib $LIB --tlef $TLEF --lef $LEF --def $DEF --sdc $SDC --parasitics placement
GRUS_PHASE_TIMING=1 /usr/bin/time -l target/release/grus-eda congestion \
  --tlef $TLEF --lef $LEF --def $DEF
GRUS_PHASE_TIMING=1 /usr/bin/time -l target/release/grus-eda place \
  --tlef $TLEF --lef $LEF --def <d>.floorplan.def --out-def /tmp/<d>.out.def
```

`/usr/bin/time -l` gives wall seconds + `maximum resident set size` (peak RSS)
on stderr; phase lines ride the same stream. Commit the driver as
`kurena/tasks/perf-profile/measure.sh` (loops designs × modes × reps, writes
one raw log) so the table is reproducible verbatim.

## Symbol-level confirmation

The repo's proven profiler is macOS `sample` (E3.6 §2, Phase 2 §1):
`sample <pid> <window>` — or `sample -file` on a named process. Take one trace
of the **top hotspot** the phase table names (expected: `sta.eval.walk` under
placement RC on jpeg — the Phase-2 winner, but the table decides). Collapsed
stacks go into the results entry. `xctrace` exists on the mini but is not the
recorded tool; `samply`/`cargo-instruments` absent — not installed.

## `docs/results.md` entry skeleton

Dated `### 2026-11-22 — perf-profile baseline (<version>)`, following the
file's conventions:

1. **Machine context** block (above).
2. **Wall-clock table**: rows = design (cells/nets/endpoints counts), columns =
   command×mode; each cell `median (min–max)` ms.
3. **Phase breakdown**: per command, per design — phase name → ms and share %;
   `power` rows show `sta.*` share inside it separately (nested attribution).
4. **Ranked hotspots**: top phases by absolute share across the matrix, with
   the `sample` collapsed-stack summary of #1.
5. **Peak RSS** note: `/usr/bin/time -l` max RSS per command on jpeg (the
   largest design) + any design that spikes.
6. **What was run**: the exact `measure.sh` invocation, rep policy, which cells
   were skipped and why; goldens untouched (`cargo test --workspace` green,
   gate proof transcripts' tails).

## Honesty rules carried over

- Numbers come from the committed `measure.sh` run as written (§7 rule 1).
- A phase share that is unmeasurable (below timer noise or inside untimed
  glue ≥5%) is named, not smoothed into a neighbor.
- Median-of-reps is the table figure; a bimodal rep set (P-core/E-core
  scheduling) is stated, not averaged away.
