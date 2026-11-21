# steps — implementation order and gates

One commit (~500–600 lines incl. tests + docs — bigger than the AP1
ticket because the pin-namespace match rides along; each step compiles
green before the next).

## 1. `crates/grus-core/src/bool_expr.rs` + `lib.rs`

- `BoolExpr`, `parse`, `prob`, `sensitivity` per `evaluator.md`; unit
  tests in-module. `pub mod bool_expr;` in `lib.rs`.
- Gate: `cargo test -p grus-core bool_expr` green; clippy-clean.

## 2. `readers/vcd.rs` — `pin_signals`

- `SignalDecl.path` (scope chain joined); `VcdActivity.pin_signals`
  emitted for every decl incl. shadowed-alias entries; vector `path[bit]`
  expansion mirrors `signals`. `signals` untouched.
- Gate: existing vcd tests green unchanged; add a small scoped-decl test
  (`tb/dut/u_t/Z` → key `tb/dut/u_t/Z`).

## 3. `readers/saif.rs` — `pin_signals` emit

- Top-scope resolved nets: `signals[net]` unchanged; cell-driven nets
  also emit `pin_signals["{driver_cell}/{driver_pin}"]`. Nested-instance
  signals emit `pin_signals["{inst}/{leaf}"]` (instances after top-scope;
  unmatched scope → count, emit nothing).
- Gate: `test_saif_power_mb_matches_opensta` and the `mbw` test stay
  green **unchanged** — `activity.signals` shape is preserved, and the
  new map only feeds `resolve_activity`'s matcher (identical net set on
  `mb`/`mbw`).

## 4. `activity.rs` + `design_db.rs` — matcher + propagation

- Move `sibling_pin_by_lib_name` → `DesignDb::pin_by_lib_name`
  (`pub(crate)`; switch the two `power.rs` callsites).
- `annotation_net_of` + the `HashMap<NetId, SignalActivity>` annotation
  map; the `vcd.signals.get(name)` arm reads the map. Precedence chain
  untouched.
- Comb-candidate DAG + Kahn fill per `api.md` §2; `unresolvable_functions`,
  `dead_annotation_entries` on `ActivityResolution`.
- Gate: `cargo check` green; `cargo test --workspace` — the `mba` test
  stays green **as written** (`s_none`/`s_input` divergent pins are still
  divergent — the new `propagated` resolution only changes their origin
  label, which the current test does not assert… it does assert the
  divergent *set*, unchanged). `mb`/`mbw` green unchanged = the
  byte-identical proof for the matcher swap.

## 5. `power.rs` — test extension

- Sessions table gains VCD paths; `STRICT` gains `propagated` handling
  under the mapping rules; `PER_PIN_RESIDUAL` = `[("s_full","y")]`;
  `EXPECTED_DIVERGENT` → the 13-pin set; `s_xz`/`s_partial2` included if
  verbatim; `origin_counts` pinned for one session; group columns per
  `golden.md`.
- Gate: `cargo test -p grus-core power` green; then the two fail-proof
  breaks (propagation off → red; corrupted propagated golden → red;
  restored), recorded.

## 6. Docs + version

- `results.md` entry (see below), `roadmap.md` N7 line → AP2 done,
  `Cargo.toml` `0.2.14 → 0.2.15`, `cargo check` for `Cargo.lock`.
- `mb.power.json` regenerated — expected `git diff` empty; `git diff`
  empty on every `*.reference.json`.
- `git status` — only the `## Files` list moved.

## 7. Final verify (all recorded in the entry)

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --release -p grus-core sta`

## results.md entry contents

- Landed model: evaluator (Shannon, repeated literals), level order
  choice, taint rule, pin-namespace annotation (+ why the spec's `## Files`
  list gained `vcd.rs`/`saif.rs`), the 13-pin `EXPECTED_DIVERGENT` note
  (`qd` shares net `q`'s residual).
- Per-session group-column residuals (the AP3 set: net `q` + 4 unknown
  nets) — measured numbers.
- `driven_nets_without_activity`/`origin_counts` movement per session
  (e.g. `s_none`: `default 10→5`, `propagated 0→5`; `mb` VCD path:
  driven-without-activity stays 0).
- `unresolvable_functions`/`dead_annotation_entries` counts on `mba` and
  the sky130 `function` coverage measurement.
- Moved goldens: none (assert `git diff` empty on all); `s_partial2`
  bonus coverage if it landed.
- Fail-proof record; verify-trio output.

## Acceptance cross-check

| acceptance item | where |
|---|---|
| evaluator `P(f)`/`d(f)` exact under independence, repeated literals | step 1, evaluator.md |
| missing/unparseable functions fall back without panic | step 4 (`unresolvable_functions`) |
| combinational level order; seq outputs never produced | step 4 (Kahn; `!is_sequential && !is_latch` gate) |
| `Default` input keeps `Default` output (taint) | step 4 |
| `Propagated` nets match golden `{density,prob}` at strict bound on the six sessions | step 5, golden.md |
| `EXPECTED_DIVERGENT` verbatim | step 5 (13 pins — `qd` included; see golden.md note) |
| fail-proof recorded | step 5 |
| `docs/results.md` entry (residuals, origin-count movement, moved-golden explanation, roadmap N7) | step 6 |
| verify trio + version bump same commit | steps 6–7 |

## Files

- `crates/grus-core/src/bool_expr.rs` + `crates/grus-core/src/lib.rs` — step 1
- `crates/grus-core/src/readers/vcd.rs` — step 2
- `crates/grus-core/src/readers/saif.rs` — step 3
- `crates/grus-core/src/activity.rs` + `crates/grus-core/src/design_db.rs` — step 4
- `crates/grus-core/src/power.rs` — steps 4–5 (helper callsites, test)
- `tests/designs/microbench/mb.power.json` — step 6 regeneration (expected
  byte-identical)
- `docs/results.md`, `docs/roadmap.md`, `Cargo.toml`, `Cargo.lock` — step 6
- `kurena/tasks/ap2-combinational-propagation/design/` — this document set
