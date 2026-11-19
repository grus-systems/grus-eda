# SAIF power input — design overview

## Context

`grus-eda power` estimates leakage/switching/internal power from VCD switching
activity (`readers/vcd.rs` → `VcdActivity` → `power::estimate_power`), validated
against OpenROAD `report_power` on `tests/designs/microbench/mb.*` (N7,
`docs/results.md` 2026-11-18). Flows that emit `.saif` cannot drive the
estimator today. This task adds a SAIF reader, a `--saif` flag, a `mb.saif`
fixture converted from `mb.vcd`, and an OpenSTA-SAIF golden compared at the
strictest honest tolerance.

## Measured OpenSTA semantics (probed 2026-11-19, LibreLane 3.0.16)

All findings below come from `openroad`/`sta` runs inside
`ghcr.io/librelane/librelane:3.0.16`; details and the required reproduction
runs are in `probe.md`. They decide the reader's shape and the golden's
tolerance — they are facts already measured, but `results.md` must record the
runs that produce the committed numbers (per `AGENTS.md` §7 rule 1).

1. **Resolution model**: `read_saif -scope <path>` selects an `INSTANCE` subtree.
   Only `NET`-grouped signals annotate anything; names resolve to **pins/ports
   of that instance scope**, not to nets. Top scope → top design ports only
   (`clk`, `a`, `y` annotated; internal net `q` silently dropped). An
   `(INSTANCE _d_ ...)` scope annotates `_d_`'s pins (`D`, `CLK`, `Q` → 3 pins).
   `PORT` groups annotate **nothing** in this build. Unmatched scopes and
   signal names are dropped with no warning and no count. `VIRTUAL_INSTANCE`
   is a parse error (STA-0169) after partial annotation.
2. **Field semantics**: `TC` scales switching power linearly (TC 4→40 = 10x).
   `T0`/`T1` duty changes **state-dependent leakage only** (internal/switching
   unchanged). `IG` and `TX` measured no effect on `report_power`. Field values
   are integers; floats are syntax errors. Unannotated pins get statistical
   activity propagation (annotating `a` alone ≈ full annotation on mb).
3. **SAIF-vs-VCD on mb**: SAIF total 2.2552e-6 vs VCD 2.25e-6 (hand-converted
   probe file); per-group deltas < 4%. `estimate_parasitics -placement` before
   `report_power` changed nothing on mb.
4. **No SAIF emitter exists in the container**: verilator `--trace-saif` needs
   `make`+`gcc` (absent), iverilog dumps VCD only (ignores `.saif` suffix),
   yosys 0.62 has no `vcd2saif`. `mb.saif` must be produced by a committed
   conversion script (python3 is in the container) — see `fixture.md`.

## Design decision

`estimate_power` consumes **per-net (rise, fall) counts + `duration_s`** —
exactly `VcdActivity` already holds. The SAIF reader parses to its own
`SaifActivity` (scope tree + per-signal SAIF fields), then a design-aware
resolver produces a `VcdActivity` keyed by net name. `estimate_power` keeps its
signature; power math is untouched. This matches the goal: "the activity
structure `estimate_power` already consumes".

Duty (`T0`/`T1`) is parsed and carried but not consumed: our leakage model is
per-cell flat (`leakage_power_w`), not state-dependent. The measured effect of
duty on OpenSTA's state-dependent leakage (~15% of a ~1e-11 term on mb) is
recorded as a residual divergence, not modeled.

## Rise/fall split

SAIF gives `TC` (total clean toggles), not per-direction counts. Rule:
`rise = fall = TC/2` when even; when odd, start ≠ end and the extra edge is
the last toggle — a `rise` iff the signal started low. The start state is
inferred from durations: the majority-time state is likelier the start
state, so the extra edge goes to `rise` iff `t0 >= t1`, else `fall`. This is
still a heuristic on duty-skewed signals (bounded at ±1 edge per net; the
limitation is stated honestly in `results.md`).

`mb` *does* exercise the rule: `clk`'s TC is 199 (toggles at every 10 ns
step from #10 to #1990), with `t0 = 1000 ns`, `t1 = 990 ns` → inferred
start-low → (100 rises, 99 falls), exactly the `mb.vcd` counts. Every
annotated net reconstructs its VCD counts, so the SAIF-vs-VCD
exact-equality check holds on this fixture.

## Resolution rules (ours)

At the selected scope (default: auto-descend, see `api.md`):

- `NET`/`PORT` group signal name → design net by name; else top-level port →
  its net. (Ours annotates `q` where OpenSTA drops it — our exact count vs
  their propagated value is a recorded residual.)
- Nested `(INSTANCE name)` → design cell by name; its signals →
  `<inst>/<pin>` → the pin's net. Same net annotated twice → last wins
  (values agree on mb.saif anyway).
- `saif_unmatched_scopes` counts instance scopes matching no design cell;
  `saif_unmatched_signals` counts signal names matching no net/port/pin.
  Both surface in `PowerDiagnostics`, the JSON report, and the stderr line —
  nothing is silently dropped (OpenSTA's silent drop is documented, not
  copied).

## Tolerance

`STRICT` on this task means the golden-comparison test style with named
per-field bounds (`POWER_SAIF`), set to the tightest honest bounds after
measurement. Expected floor from probes: internal ≤ 2%, leakage ≤ 2%,
total ≤ 1%; switching needs ≤ 5% on the Sequential group (known ~3.7%
residual vs OpenSTA — root cause must be **named** in `results.md` before
the bound is loosened, per `AGENTS.md` §7 rule 5; likely a pin-cap or
wire-load modeling delta, not an activity defect, since it exists on the
VCD path too).

## Risks

- **mb.saif hand-vs-converted**: spec requires values from a tool run.
  Mitigation: `vcd2saif.py` (committed) computes T0/T1/TX/TZ/TB/TC from the
  mb.vcd waveform; `generate_saif_ref.sh` runs it + the OpenSTA Tcl in the
  container, emitting `mb.saif` and `mb.saif.reference.json` — nothing typed
  by hand.
- **`q` divergence**: OpenSTA drops the `q` NET entry (pin-name resolution)
  and propagates instead. On mb propagated ≈ exact; on other designs it could
  diverge like gcd's. Recorded in results.md; same caveat class as VCD.
- **Clap arg change**: `--vcd` becomes `Option` + `conflicts_with`; a missing
  both-args case must fail loudly at CLI parse.

## Files

- `crates/grus-core/src/readers/saif.rs` (new)
- `crates/grus-core/src/readers/mod.rs`
- `crates/grus-core/src/power.rs`
- `crates/grus-eda/src/main.rs`
- `tests/designs/microbench/` (`vcd2saif.py`, `generate_saif_ref.sh`,
  `mb.saif`, `mb.saif.reference.json`, `mb.saif.openroad.txt`;
  `mb.openroad.txt` stays as the VCD raw report;
  `mb.power.json` regenerated for renamed diagnostics)
- `docs/results.md`
- `docs/roadmap.md`
- `README.md` (only if the power CLI surface is documented there — check;
  it is not today, so likely untouched)
- `Cargo.toml` (workspace version bump, same commit)
- `Cargo.lock`
