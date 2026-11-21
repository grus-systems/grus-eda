# steps — implementation order and gates

One commit (~300 lines incl. test + docs). Order chosen so each step
compiles green before the next.

## 1. `crates/grus-core/src/activity.rs` + `lib.rs`

- Write the module (`api.md` types + `resolve_activity` + constant-cone
  worklist). `pub mod activity;` in `lib.rs` (no glob re-export).
- Gate: `cargo check -p grus-core` green; clippy-clean (`BTreeMap` import,
  no dead code — `ActivityOrigin::{Propagated,Sequential,Unknown}` are
  reachable via the label/`origin_counts` path or `#[allow]`-justified:
  preferred — include them in the `label()` match so they compile used).

## 2. `power.rs` — the swap

- `use crate::activity::{resolve_activity, ActivityOverrides, ResolvedActivity};`
- `PowerCx.net_activity: Vec<ResolvedActivity>`; `net_activity()`/`net_prob_one()`
  through it; `cx.duration_s = resolution.window_s`.
- `estimate_power` signature gains `overrides: &ActivityOverrides`; fill
  `diagnostics.activity_origin_counts` from the resolution;
  `driven_nets_without_activity` counts `Default|Unknown` origins on
  driven nets. Update the `PowerDiagnostics` doc comments and module doc.
- Fix existing test call sites (`&ActivityOverrides::default()`).
- Gate: `cargo test -p grus-core power` — `test_power_mb_vcd_matches_opensta`
  and `test_power_mb_saif_matches_reference` stay green *unchanged*
  (the bit-identical proof for annotated runs); `mbw`/`sta` tests green.

## 3. `main.rs` — no-annotation CLI

- Drop `required_unless_present`; `(None, None)` → `VcdActivity::default()`;
  the `Power` line's activity branch; origin split line; parse test.
- Gate: `grus-eda power --lib … --tlef … --lef … --def mba.def --sdc mba.sdc`
  runs and prints sane numbers (no annotation file needed).

## 4. The golden test

- `test_activity_defaults_mba_matches_opensta` per `golden.md`: pin→net
  map, four sessions, strict sets, `EXPECTED_DIVERGENT` (28 pins), group
  bounds on `s_global*`, recorded-only deltas on `s_none`/`s_input`.
- Gate: test green; **then** the two fail-proof breaks recorded
  (seed pass off → red; corrupted golden density → red; both restored).

## 5. Docs + version + regenerated golden

- `results.md` entry (`docs.md` outline), `roadmap.md` N7 line,
  `Cargo.toml` 0.2.14, `cargo check` to refresh `Cargo.lock`.
- Regenerate `mb.power.json` via the recorded CLI command; `git diff`
  must show only `activity_origin_counts` — anything else is a bug, stop.
- `git diff` empty on all `*.reference.json`.

## 6. Final verify (all in the results entry)

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --release -p grus-core sta`
- `git status` — only the `## Files` list moved.

## Acceptance cross-check

| acceptance item | where |
|---|---|
| `ResolvedActivity` + `ActivityOrigin`, accessors through it | step 2 |
| measured precedence, all six classes | step 1, golden.md table |
| representation + `duration_s` recorded | step 5, docs.md |
| per-origin `PowerDiagnostics` counts | step 2 |
| `driven_nets_without_activity` = genuinely unresolved | step 2 |
| `power` works with no `--vcd`/`--saif` | step 3 |
| `s_global`/`s_global_density` strict, `s_none`/`s_input` strict+divergence table | step 4 |
| fully-annotated byte-identical | step 5 (`mb.power.json` diff = one field) |
| fail-proof recorded | step 4 |
| version bump same commit | step 5 |
| verify trio | step 6 |
