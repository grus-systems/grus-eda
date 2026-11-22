# perf-par-slew — measurement

The before numbers already exist: `docs/results.md` 2026-11-22
"perf-profile baseline" (v0.3.2, same M4 mini, `REPS=5` warm-cache medians
from `kurena/tasks/perf-profile/measure.sh`). Reuse them verbatim — do not
re-measure the serial binary; the entry pairs "before (v0.3.2 baseline
entry)" with the new medians.

## Protocol

Same machine, same flags, release build (`cargo build --release`):

```sh
REPS=5 sh kurena/tasks/perf-profile/measure.sh /tmp/perf-par-slew.log
```

The committed matrix is the honest comparison — every cell the baseline
tabled gets an after number for free. If the full matrix is impractical,
the acceptance minimum is: `sta.placement`, `sta.spef` (+`sta.spef_rcx`
where the artifact exists), and `power.placement` on `picorv32`, `aes`,
`ibex`, `jpeg` — same `REPS=5`. gcd costs ~1 s total; include it if the
full script ran anyway. Extract wall medians per cell and the
`sta.prepare.clock.slew` / `sta.eval.walk` phase medians from the
`GRUS_PHASE_TIMING` lines in the log (the script sets the gate). Note:
`power.slews` nests `sta.prepare.clock.slew` — report the nested phase
under `power` separately, same convention as the baseline entry.

## Expectation and ceiling

Plan's Amdahl row (k = 3 conservative; `T' = T·(1 − s + s/k)`):

| design/cell | s (baseline share) | ceiling |
|---|---|---|
| jpeg `sta` placement/SPEF | 0.77–0.81 | 6.0 s → ~2.7 s (×2.2) |
| aes `sta` | 0.78 | 1480 → ~690 ms |
| picorv32 `sta` | 0.78 | 860 → ~400 ms |
| ibex `sta` | 0.50 | 670 → ~450 ms (fallback mode — report the fixup share if visible) |
| gcd `sta` | 0.41 | 50 ms — timer floor, expect "within noise" |
| jpeg `power` placement | ~0.79 of `power.slews` | inherits — ~6.1 s → ~3.0 s |

Residual to name in the entry, decomposed from the phase lines: serial
per-level residue (seeding, apply pass, fixup, <32-pin levels), level
imbalance, memory-bound DMP lookups (`k` < cores), and the
`topological_levels` retry inside the phase on fallback designs.

## `docs/results.md` entry skeleton

Append at the end, same format as the baseline entry:

```markdown
### 2026-11-22 — perf-par-slew: parallel propagate_slew (v0.3.3)

`propagate_slew` level loop now computes per pin in parallel and applies
in level order (same shape as `sta.eval.walk`), plus a serial per-level
replay of same-level-earlier seq arcs — required only on the
include_seq=false fallback (ibex), a no-op scan otherwise. Design:
`kurena/tasks/perf-par-slew/design/`.

**Before/after wall** — `median (min–max)` ms, REPS=5, same machine/flags
as the baseline entry:

| cell | before | after | speedup |
|---|---|---|---|
| sta placement RC — picorv32/aes/ibex/jpeg | … | … | … |
| sta routed SPEF — … | | | |
| power placement RC — … | | | |

**Phase**: `sta.prepare.clock.slew` jpeg 4840 → ___ ms (×__); share of
sta wall __% → __%. Residual named: ___.

**Equivalence**: all committed goldens untouched (`git status` clean);
`cargo test --workspace` / clippy / `cargo test --release -p grus-core
sta` green; `--json` `cmp`-clean pre/post on ___; `RAYON_NUM_THREADS`
1/4/10 `cmp`-clean on ___. Fail-proof: ___ → ___ tests red → restored.
Fixup probe: fires on ___ (or: no committed design exercises it —
defensive).

**What was run**: `REPS=5 sh kurena/tasks/perf-profile/measure.sh
/tmp/perf-par-slew.log` (raw log not committed, script is).
```

## `docs/roadmap.md` N8 update

One line: `perf-par-slew` moves from candidate to done with the measured
jpeg number; keep `perf-slewonly-power` (now larger relative share),
`perf-legal-passes`, parser tickets in order.

## Version

`Cargo.toml` `[workspace.package] version` `0.3.2` → `0.3.3` (patch —
internal speedup, no behavior change), `Cargo.lock` follows; same commit
as the functional change.
