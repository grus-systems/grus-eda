# steps — implementation order and gates

One commit (~250 lines incl. tests + docs + fixture). The sky130
census was run during planning (`census.md`) — implementation needs no
container session; every golden number is already in the committed
`mba.activity.reference.json`.

## 1. `lib_db.rs` + `readers/liberty.rs` — sequential data pins

- `LibSeq { state_var, state_var_n: Option<SymbolId>, data_pin:
  Option<SymbolId> }` + `LibCell::seq: Option<LibSeq>` (api.md §1).
- `ff`/`latch` arms: parse the block body — capture `next_state`
  (ff) / `data_in` (latch), classify via `bool_expr::parse` (single
  `Var` → `Some(SymbolId)`, else `None`); keep the placeholder-pin
  push for state vars; all other attributes through
  `skip_statement_or_group` honoring its closed-body return (api.md §2).
- `CACHE_VERSION` 8 → 9 (LibCell layout changed).
- `power.rs::group_of_classifies_by_cell_kind` literal gains
  `seq: None`.
- Gate: `cargo test -p grus-core liberty` green (existing ff/latch
  parse tests unchanged — they assert flags/pins, not `seq`).

## 2. `activity.rs` — `seq_funcs` + unified `propagate` + `Unknown`

- `seq_function_table` per api.md §3 (driver on `seq.is_some()` cell;
  `data_pin` `None` → not a candidate; output function →
  verbatim/`1−P`; `pin_by_lib_name` → data net).
- `propagate_combinational` → `propagate` per api.md §4: candidate =
  `Default ∧ (comb_funcs[n] ∨ seq_funcs[n])`; deps = comb input nets
  or `[data_net]`; seq eval arm writes `Propagated` with copied
  `eff_toggles` and (possibly inverted) `prob_one`; taint unchanged.
- Post-pass: every `Default` → `Unknown {Some(0.0), 0.0}` (api.md §4).
- `ActivityOverrides::input_ports` field + seed arm between `global`
  and dump annotation (api.md §5).
- Doc comments: header fill-order + `Sequential`/`Unknown` origin
  notes (api.md §6); `power.rs` `driven_nets_without_activity` comment
  names `Unknown` only.
- Gate: `cargo test -p grus-core` — existing tests green except the
  mba test (updated in step 4 before running).

## 3. `tests/designs/microbench/mba_p40.sdc`

- Verbatim the generator's `/tmp/mba40.sdc` (period 40.0, all other
  lines identical to `mba.sdc`).

## 4. `power.rs` — the 12-session rework

Per golden.md:

- `sessions` → 12 entries (+ SDC column for `s_p40`; `s_input_ports`
  uses `input_ports: Some({1e7, 0.4})`).
- `match src` — `unknown` arm → strict `{d, P}` + label `unknown`.
- `EXPECTED_DIVERGENT` → `&[]`; drop the `s_global*` special-case
  expected set (empty everywhere).
- `origin_counts` `s_none` pin → `{clock 1, constant 4, input 3,
  propagated 6, unknown 4}`.
- Group `check` harness for all 12 sessions — measure first, assert at
  the recorded bound (golden.md §group columns).
- Keep: `PER_PIN_RESIDUAL`, `dead_annotation_entries` /
  `unresolvable_functions` asserts, the `s_partial`≡`s_ports` verbatim
  compare, test doc comment updated to "all 12 sessions".

## 5. Fail-proof (recorded in the entry)

- Seq producers dropped → `u_df/Q`/`qd` red (`unknown`/`0` vs
  `propagated`/`3e6`); restore.
- `s_ports` `u_df/Q` density corrupted → red; restore byte-identical.

## 6. Docs + version

- `docs/results.md` — `### 2026-11-22 — N7 — AP3: sequential activity
  rules (v0.2.17)`:
  - the 13-pin residual closed (`u_df/Q`+`qd` → `propagated`, 11 pins
    → `unknown`), per-mechanism before/after for net `q` and the cone;
  - the sky130 census table (census.md — 37/63 covered; residual
    families named; `test_cell` ff copies excluded like the parser);
  - the Q_N `1−P` inversion rule and where it applies (covered `*bp`
    cells; unmeasured on mba — measured-adjacent);
  - `prob_one: Some(0.0)`/`when` on `unknown` nets (chosen,
    unmeasured); `input_ports` precedence choices (beats dump, loses
    to global/clock — unmeasured where applicable);
  - group-column residuals per session (measured table replacing the
    AP2 `+4–95%` rows);
  - `driven_nets_without_activity` 5 → 4; `s_none` `origin_counts`
    move; `Default` now an impossible resolved origin;
  - moved goldens enumerated (`mb.power.json` `git diff` empty, every
    `*.reference.json` unchanged); fail-proof record; checks run.
- `docs/roadmap.md` N7 — extend the AP2 clause to `**AP3 done**`
  (v0.2.17 — seq Q=D copy, `unknown` cones, all 12 sessions verbatim);
  AP4 remains open.
- `Cargo.toml` `0.2.16 → 0.2.17`; `cargo check` refreshes
  `Cargo.lock`.

## 7. Final verify (all recorded in the entry)

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --release -p grus-core sta`

## Acceptance cross-check

| acceptance item | where |
|---|---|
| `LibCell` stores seq data pins; uncovered forms a named residual + census | steps 1, 6; `census.md` |
| seq outputs copy `{P,d}` origin `Propagated`; Q/Q_N rule recorded | steps 2, 6 (api.md §3–4) |
| fixed point; leftover `Default` → `unknown {0,0}` taints | step 2 (api.md §4) |
| `EXPECTED_DIVERGENT` == `{}`; 12 sessions verbatim; seq columns follow `a` incl. a2x | step 4 (golden.md) |
| group columns asserted at recorded bound | step 4, 6 |
| fail-proof recorded | step 5 |
| results.md entry + roadmap N7; moved goldens enumerated | step 6 |
| test trio + version bump same commit | steps 6–7 |

## Files

- `crates/grus-core/src/activity.rs` — `seq_funcs`, `propagate`,
  `Unknown` conversion, `input_ports`
- `crates/grus-core/src/readers/liberty.rs` — `ff`/`latch` body parse,
  `CACHE_VERSION` 9
- `crates/grus-core/src/lib_db.rs` — `LibSeq`, `LibCell::seq`
- `crates/grus-core/src/power.rs` — `LibCell` literal, test rework
- `tests/designs/microbench/mba_p40.sdc` — new
- `docs/results.md`, `docs/roadmap.md`, `Cargo.toml`, `Cargo.lock`
- `kurena/tasks/ap3-sequential-activity/design/` — this document set
