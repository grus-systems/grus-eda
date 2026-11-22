# docs — `results.md` entry, `roadmap.md`, commit record

## `docs/results.md` — new dated entry (under the N7 activity-propagation section)

Title shape: `### 2026-… N7 — AP1: resolved activity + measured seed defaults`.

Required content, in the established entry format (what ran / table /
named residual / check):

- **Representation**: `ResolvedActivity` carries a count over `window_s`
  (`eff_toggles`), not a rate — annotated nets keep `sa.eff_toggles()`
  verbatim so `switching_w`/`pin_internal_power` arithmetic is untouched
  and `mb.power.json` watt columns stay bit-identical; seeded nets
  synthesize `density × window_s`. The defined conversion is
  `count = rate × window_s`; the rejected alternative (f64 rate, drop
  `/duration_s`) would reorder annotated math and risk ulp moves on the
  committed JSON.
- **`duration_s` semantics**: `window_s = vcd.duration_s` when a dump is
  loaded and `> 0`, else `1.0` — counts equal absolute densities
  numerically and the window cancels in `switching_w`. A `0`-duration VCD
  is absolute-rate mode. `PowerReport.duration_s` reports that window.
- **Seed order landed**: constant → clock → global → annotated →
  input/undriven → leftover `Default` (= effective input seed, interim
  until AP2/AP3). Constant-through-buffer is the fixed-point cone pass
  (value-free: all inputs constant ⇒ output constant), not gate-function
  evaluation.
- **Per-net vs per-pin**: OpenSTA holds activity per pin; ours per net.
  Uniform across net pins in all four AP1 sessions; the `s_full`
  port-`y` (`vcd`) vs `u_xor/X` (`propagated`) split is AP4 scope.
- **Golden coverage**: `s_global`/`s_global_density` all-43-pin strict +
  group bounds (measured residual and chosen bound recorded); `s_none`/
  `s_input` 15 seeded pins strict, 28-pin expected-divergence table
  (11 `unknown`, 17 `propagated` — list lives in the test as
  `EXPECTED_DIVERGENT`); group columns of `s_none`/`s_input` measured
  and recorded but deliberately unbounded (interim fill ≠ propagation).
- **Moved goldens**: `mb.power.json` regenerated — name the diff
  (`activity_origin_counts` field only; watt columns bit-identical, or
  the named divergence). `git diff` clean on every other `*.reference.json`.
- **Diagnostics changes**: `activity_origin_counts` (per-net split —
  OpenSTA's `report_activity_annotation` is per-pin, recorded difference);
  `driven_nets_without_activity` now counts `Default`/`Unknown`-origin
  driven nets instead of absent-annotation nets — it *rises* on
  partial-VCD fixtures where it previously counted nothing (mb/mbw stay
  0); `conditional_power_blocks` no longer counts unannotated literal
  nets (seed probabilities make `when` literals expressible — the
  N7-anticipated change).
- **No-annotation CLI**: `grus-eda power` without `--vcd`/`--saif` works;
  `power:` line prints `activity: defaults`; origin split line added.
  No `-global`/`-input` CLI flags (API only).
- **Fail-proof record**: which break was injected (seed pass off → which
  assertion went red; corrupted golden pin → red) and restored.
- **Limits named**: mixed-input gates on constant cones → leftover
  (unmeasured); cell-driven clock-tree nets → `Default` not `Clock`
  (unmeasured); constant `prob_one` = 0 read (HI truth value unmeasured);
  leftover `Default` is interim and changes in AP2/AP3.
- **Checks run**: `cargo test --workspace`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo test --release -p grus-core sta`,
  `git diff` on all `*.reference.json`.

## `docs/roadmap.md`

N7 "still open" line: mark AP1 landed — e.g.
`statistical activity propagation — semantics measured 2026-11-21;
sequenced AP1–AP4. AP1 done (resolved-activity skeleton + measured
defaults; mba `s_global`/`s_global_density` strict, `s_none`/`s_input`
seeded-strict + divergence table). AP2 combinational propagation, AP3
sequential, AP4 precedence/xz/diagnostics remain.`

## `Cargo.toml` / `Cargo.lock`

`0.2.13` → `0.2.14` in the same commit (feature → patch while 0.x).

## Commit message

`feat:` subject; body lists the representation choice + reason, the seed
order, the `window_s` definition, the diagnostics semantic changes, and
names the moved golden. Do not claim OpenSTA propagation parity —
the message states AP1 is the skeleton + defaults, AP2–AP4 remain.
