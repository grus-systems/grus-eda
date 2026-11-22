# perf-parsers — implementation steps

Three independent parsers, one ticket: land each parser's rewrite as its
own gated step so a regression bisects to one file. `cargo test
--workspace` green at every gate; the step-2/4/6 `cmp` runs are the
incremental identical-output evidence (verification §2).

## 0. Before-log (no code change)

```sh
cargo build --release
REPS=5 sh kurena/tasks/perf-profile/measure.sh /tmp/perf-parsers-before.log
# identical-output references (verification §2):
GRUS_PHASE_TIMING= target/release/grus-eda sta --lib $LIB --tlef $TLEF --lef $LEF \
  --def tests/designs/jpeg/jpeg.placed.def --sdc tests/designs/jpeg/jpeg.sdc \
  --spef tests/designs/jpeg/jpeg.routed.spef --json /tmp/sta-spef.before.json
# (same shape for picorv32 rcx spef, sta.placement, congestion, power --vcd
#  — see verification.md §2; stdout runs redirect to /tmp/*.before.txt)
```

(`$PDK`/`$LIB`/`$TLEF`/`$LEF` as in `measure.sh`.) Copy the pre-change
binary aside (`cp target/release/grus-eda /tmp/grus-eda.before`) if step
runs want to interleave without rebuilding.

## 1. `fast-float` dependency + shared alias (no behavior change)

- Add `fast-float` to `[workspace.dependencies]` and `grus-core`
  (maturity: fast-float 2.x is years old; justify in commit message).
- `type FastMap` alias inside each touched file (private, 1 line each —
  no cross-file imports).
- **Gate**: builds; full suite green (nothing consumed yet).

## 2. SPEF (`spef.md` S1–S5)

Order inside the file: S2's `u32` NAME_MAP + `Cow` `resolve` first
(signature change ripples least), then S1 token scratch, S3 ascii
split, S4 fast-float, S5 map hashers.
- **Gate**: suite green (incl. every `load_spef` consumer test); SPEF
  `cmp` runs vs `/tmp/*.before.*` clean; quick sanity
  `GRUS_PHASE_TIMING=1` `sta.spef.jpeg` shows `sta.spef.read` down
  before moving on (record the number).

## 3. DEF (`def.md` D1–D5)

`read_conn_group` + scratch statements first (D1), then `Cow` (D2),
hashers (D3), attribute iterator (D4), probe de-dup (D5).
- **Gate**: suite green (incl. per-pin orientation goldens + DEF
  roundtrips); `congestion.jpeg`/`sta.placement.jpeg` `cmp` vs
  before clean.

## 4. VCD (`vcd.md` V1–V6)

`get_mut`/`record_edge` de-alloc (V1–V2), `bit_names` (V3), pad
iterator (V4), `FastMap` + boundary conversion (V5), byte dispatch
(V6).
- **Gate**: suite green (incl. `power --vcd` goldens); `power.vcd.gcd`
  stdout `cmp` vs before clean.

## 5. Fail-proof runs

The three mutations of verification §3, each → red → restored green,
recorded for `docs/results.md`.

## 6. Measure + document

- `cargo build --release`; `REPS=5 sh
  kurena/tasks/perf-profile/measure.sh /tmp/perf-parsers-after.log`
  (full matrix, idle machine).
- `docs/results.md` entry per `measurement.md` skeleton: phase table,
  cell-wall table, achieved-vs-×2, named residuals, equivalence +
  `cmp` + fail-proof record, "what was run".
- `docs/roadmap.md` N8 update (measurement.md).
- `[workspace.package] version` 0.3.5 → 0.3.6 + `Cargo.lock` refresh,
  same commit as the functional change.
- Final: `cargo test --workspace`, `cargo clippy --workspace
  --all-targets -- -D warnings`; sta-regression leg if the environment
  supports it (`cargo test --release -p grus-core sta`).

## Suggested commit split

1 + 2 (dep + SPEF), 3 (DEF), 4 (VCD), 6 (results/roadmap/version) —
each gate is its own green suite, so any order within the split works.
