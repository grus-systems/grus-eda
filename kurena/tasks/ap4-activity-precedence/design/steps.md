# steps — revision order and gates

Base `22b96d6` (v0.3.0) is committed; this is the review-memo
revision, one commit (~60–120 lines mostly docs). Order chosen so
every artifact the docs must quote exists before the docs are written.

## 1. `power.rs` — `PowerDiagnostics` dead fields

Per api.md §7:

- `PowerDiagnostics` gains `dead_annotation_entries: usize` and
  `dead_user_entries: usize` (doc comments: mirrors of the
  `ActivityResolution` counters).
- `estimate_power` populates both in the `PowerCx` init next to
  `activity_origin_counts`.
- Gate: `cargo check -p grus-core` — the `..Default::default()` init
  keeps the struct literal compiling; `Serialize` carries them into
  `--json` automatically.

## 2. `main.rs` — `activity:` line appends the counters

Per api.md §7: `println!("activity: {split}")` gains
`dead_annotation={}` `dead_user={}` after the split, unconditional;
update the `report_activity_annotation` comment. Gate: build the
release binary and run the gcd `--vcd` command — quote whatever the
line prints for the entry (expected `dead_annotation=1286
dead_user=0` per the v0.3.0 record; if it prints a different number,
the VCD census in gcd.md §provenance is the independent check — a
divergence there is a bug to chase, not a typo to overwrite).

## 3. `power.rs` test — `via_pins` dead entry

Per golden.md: add `(pin_obj["u_or/A"], setting(1e7, 0.4))` to the
`via_pins` `pins` vec; assert `via_pins.dead_user_entries == 1` and
the verbatim per-net compare to `s_input_ports` stands.
Gate: `cargo test -p grus-core power` green.

## 4. Re-runs (capture verbatim output for the entry)

- Our CLI: the gcd `--vcd` command and the no-annotation variant
  (gcd.md §inputs) — capture `activity:` and `power:` lines.
- `power_vcd_check.tcl` in the LibreLane container (gcd.md
  §card-question (a)) — capture `Annotated N pin activities`, the
  `STA-1453` warning, the `report_power` table. If the container
  cannot run, that fact is what the entry records.
- `gcd.vcd` regen (gcd.md §provenance recipe) — only if iverilog is
  available; otherwise the entry records "carried 2026-11-18
  artifact, recipe unexecuted".
- `mb.power.json` regen — `git diff` must be exactly the two new
  diagnostics keys.
- Fail-proof (golden.md): `dead_user_entries` computation forced to
  `0` → `dead pins entry counted` red; restore, green.

## 5. Docs

Per docs.md:

- `docs/results.md` — revision block inside the AP4 entry (card
  answers, provenance, diagnostics lines, re-verified table),
  Moved-goldens correction, residual-notes restatement, updated
  checks.
- `docs/roadmap.md` — stale N7 paragraph replaced.
- `tests/designs/README.md` — gcd `power/` bullet.

## 6. Version + final verify

- `Cargo.toml` `0.3.0 → 0.3.1`; `cargo check` refreshes `Cargo.lock`.
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --release -p grus-core sta`
- All commands and outcomes recorded in the revision block.

## Acceptance cross-check (original spec, met by base + R1)

| acceptance item | where |
|---|---|
| `-input_ports`/`-pins` resolvable via API, origin `user`; `-activity` vs `-density` at call site, `s_p40` pinned; precedence user < global/clock/constant, > input | api.md §1–§4 (landed) |
| `s_input_ports` + `s_p40` per-pin `{d,P,source}` verbatim; 12 sessions match; fail-proof both new arms | golden.md (landed); R1 dead-entry fail-proof |
| gcd `report_power` re-comparison in `results.md` — moved/diverged named | landed table; R1 revision block (gcd.md) |
| N7/roadmap updated; label + `unknown` residuals recorded | docs.md §2 (R1: stale paragraph fixed, residuals enumerated) |
| `cargo test --workspace` + clippy + `sta` release; version bump same commit | steps 6 |

## Files

- `crates/grus-core/src/power.rs` — `PowerDiagnostics` fields,
  `estimate_power` population, `via_pins` dead entry
- `crates/grus-eda/src/main.rs` — `activity:` line counters
- `docs/results.md` — AP4 entry revision
- `docs/roadmap.md` — stale paragraph rewrite
- `tests/designs/README.md` — gcd `power/` bullet
- `tests/designs/microbench/mb.power.json` — regenerated, named diff
- `Cargo.toml` (0.3.1), `Cargo.lock`
- `kurena/tasks/ap4-activity-precedence/design/` — this document set
