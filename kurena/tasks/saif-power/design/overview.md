# SAIF power input — design overview

## Revision note

This revision addresses the rejected-review memo. Four corrections to the
committed fixture/scripts/record — the reader and CLI design are unchanged:

1. **`generate_saif_ref.sh` `igtx` probe targets `y`, not clk.** The probe did
   `text.replace("(TX 0)", "(TX 40)", 1)`, which hits the *first* `(TX 0)` in
   file order — `clk`'s line — while `docs/results.md` recorded the probe as
   "on `y`". Probe mutations must be anchored to a named signal's line; see
   `fixture.md`/`probe.md`.
2. **`mb.saif` clk `TC 200` → odd.** `vcd2saif.py` gains `--window-end`;
   aggregating over `[10, 1990]` drops clk's final `#2000` fall, giving
   `TC 199` so the odd-TC rise/fall split is golden-covered, not only
   unit-tested. Consequence: the SAIF window is 10 ns shorter than the VCD
   window — recorded honestly (`probe.md` P3/P4).
3. **Record the WARNING-line filter.** The committed `mb.saif.openroad.txt`
   is `openroad` stdout filtered by `grep -v` (INFO/WARNING/banner/blank
   lines) — the script comment saying "whole stdout" and the results entry
   must say filtered, not verbatim.
4. **`cargo test --workspace` in the results entry.** The "Checks run" line
   listed only package-scoped test runs; the acceptance requires the full
   workspace suite recorded.

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
runs that produce the committed numbers (per `AGENTS.md` §7 rule 1), and each
probe variant must be recorded against the signal it actually mutates.

1. **Resolution model**: `read_saif -scope <path>` selects an `INSTANCE` subtree.
   Only `NET`-grouped signals annotate anything; names resolve to **pins/ports
   of that instance scope**, not to nets. Top scope → top design ports only
   (`clk`, `a`, `y` annotated; internal net `q` silently dropped). An
   `(INSTANCE _d_ ...)` scope annotates `_d_`'s pins (`D`, `CLK`, `Q` → 3 pins).
   `PORT` groups annotate **nothing** in this build. Unmatched scopes and
   signal names are dropped with no warning and no count. `VIRTUAL_INSTANCE`
   is a parse error (STA-0169) after partial annotation.
2. **Field semantics**: `TC` scales switching power linearly (TC ×10 =
   switching ×10). `T0`/`T1` duty changes **state-dependent leakage only** on
   the early probes — on the committed fixture's run the swap left every
   column identical (see `results.md`); either way internal/switching do not
   consume duty. `IG` and `TX` measured no effect on `report_power`. Field
   values are integers; floats are syntax errors. Unannotated pins get
   statistical activity propagation.
3. **SAIF-vs-VCD on mb**: SAIF total `2.2552e-6` vs VCD `2.2541e-6` (~0.05%) on
   the previous `[10,2000]` fixture; the truncated-window regen re-measures
   this. `read_vcd` and `read_saif` feed different annotation paths, so a
   small OpenSTA-side delta is expected even with identical activity.
4. **No SAIF emitter exists in the container**: verilator `--trace-saif` needs
   `make`+`gcc` (absent), iverilog dumps VCD only, yosys 0.62 has no
   `vcd2saif`. `mb.saif` is produced by the committed `vcd2saif.py`
   (python3 is in the container) — see `fixture.md`.

## Design decision

`estimate_power` consumes **per-net (rise, fall) counts + `duration_s`** —
exactly `VcdActivity` already holds. The SAIF reader parses to its own
`SaifActivity` (scope tree + per-signal SAIF fields), then a design-aware
resolver produces a `VcdActivity` keyed by net name. `estimate_power` keeps its
signature; power math is untouched. This matches the goal: "the activity
structure `estimate_power` already consumes".

Duty (`T0`/`T1`) is parsed and carried but not consumed by the power model:
our leakage is per-cell flat (`leakage_power_w`), not state-dependent. Duty
still drives the odd-TC split inference below.

## Rise/fall split

SAIF gives `TC` (total clean toggles), not per-direction counts. Rule:
`rise = fall = TC/2` when even; when odd, start ≠ end and the extra edge is
the last toggle — a `rise` iff the signal started low. The start state is
inferred from durations: the majority-time state is likelier the start
state, so the extra edge goes to `rise` iff `t0 >= t1`, else `fall`. **A
duty tie (`t0 == t1`) resolves to `rise`** — the start-low assumption; on
mb's clk this happens to be the truth. This is still a heuristic on
duty-skewed signals (bounded at ±1 edge per net; limitation stated in
`results.md`).

`mb` exercises the rule: with `--window-end 1990` the SAIF window is
`[10, 1990]` (`DURATION 1980` ns). `clk` toggles at every 10 ns step from
`#10` to `#1990` → `TC 199`, and its duty degenerates to `T0 = T1 = 990` →
the tie-break sends the extra edge to `rise` → `(100, 99)`, exactly the
in-window VCD counts. The `#2000` fall lies outside the SAIF window, so the
resolved `clk` differs from the full-file VCD count by exactly one fall —
asserted per-net in the test, recorded in `results.md`.

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
per-field bounds, set to the tightest honest bounds after measurement.
Bounds measured on the current fixture: internal ≤ 2%, switching ≤ 5%,
total ≤ 1%, leakage ≤ 25% with a `1.5e-12` W absolute floor (the
Combinational leakage term is pW-class — ~23% relative but ~1e-12 W
absolute). The Sequential-switching ~3.7% residual is the same
driver-pin-cap delta the VCD path carries — root cause named in
`docs/results.md`, not an activity defect.

## Risks

- **mb.saif hand-vs-converted**: spec requires values from a tool run.
  `vcd2saif.py` (committed) computes T0/T1/TX/TZ/TB/TC from the mb.vcd
  waveform; `generate_saif_ref.sh` runs it + the OpenSTA Tcl in the
  container, emitting `mb.saif` and `mb.saif.reference.json` — nothing typed
  by hand, including the `--window-end` truncation.
- **`q` divergence**: OpenSTA drops the `q` NET entry (pin-name resolution)
  and propagates instead. On mb propagated ≈ exact; on other designs it could
  diverge like gcd's. Recorded in results.md; same caveat class as VCD.
- **Clap arg change**: `--vcd` became `Option` + `conflicts_with`; a missing
  both-args case fails loudly at CLI parse.
- **Probe positional fragility** (the rejected-review bug): probe variants
  must mutate fields via regexes anchored on the signal *name*, never
  positional `replace` on shared substrings like `(TX 0)` — the same T0/T1
  values appear on the `y` net and `_i_`'s `Y` pin line.

## Files

- `crates/grus-core/src/readers/saif.rs` (exists; test expectations updated)
- `crates/grus-core/src/power.rs` (test updated: per-net comparison)
- `tests/designs/microbench/` (`vcd2saif.py`, `generate_saif_ref.sh`,
  `mb.saif`, `mb.saif.reference.json`, `mb.saif.openroad.txt` —
  regenerated; `mb.power.json` unchanged)
- `docs/results.md` (2026-11-19 entry corrected)
- `docs/roadmap.md` (N7 wording if the window description changes)
- `crates/grus-core/src/readers/mod.rs` (unchanged)
- `crates/grus-eda/src/main.rs` (unchanged)
- `README.md` (unchanged — power CLI surface not documented there)
- `Cargo.toml` / `Cargo.lock` (no further bump: `0.1.96` already carries
  this in-flight change)
